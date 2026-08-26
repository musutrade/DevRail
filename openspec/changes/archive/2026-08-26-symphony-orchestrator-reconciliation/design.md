## Context

当前 DevRail 已有按优先级轮询的任务调度、PostgreSQL 并发领取和 Harness Supervisor 基础闭环。本变更需要在现有 `backend` worker、run 状态、事件、审批和审计边界上增加可靠性语义；必须遵守 `AGENTS.md` 的分层、组织数据范围、脱敏和禁止绕过 Clippy 的约束。动机和范围见 `proposal.md`，行为契约见 `specs/symphony-orchestrator/spec.md`。

## Goals / Non-Goals

**Goals:**

- 在多 worker 和进程重启场景下保持单任务单活动 run 和稳定 attempt 幂等。
- 让 reconciliation 成为每轮 dispatch 前的强制步骤，并安全处理孤儿 claim、孤儿进程和过期 run。
- 提供可配置的退避、stall 检测、传输恢复、终态清理和审计/指标证据。
- 通过向后兼容的迁移和集成测试覆盖实际故障路径。

**Non-Goals:**

- 不在本变更中实现 `WORKFLOW.md`、外部 TaskTracker、DAG、per-task workspace 或 continuation turns。
- 不建设 Agent 能力注册中心、复杂负载均衡、Redis/Kubernetes 集群或供应商模型路由。
- 不让浏览器直接启动 Agent，也不在 Handler 请求生命周期内运行调度循环。

## Decisions

### 1. 控制循环与边界

Orchestrator 继续作为后端 worker 运行，使用受控的生命周期循环：`reconcile → claim/dispatch → reap/metrics`。它只调用 Service/Repository 和 Harness Supervisor 接口，不直接执行 SQL 或启动 app-server。每轮和每次修正都使用 Scheduler/System Actor。

备选方案：把调度逻辑放进 Handler 或让浏览器轮询并启动 run。拒绝原因：会导致请求时限、权限上下文、并发控制和重启恢复分散，无法保证始终在线。

### 2. Claim、attempt 与唯一性

在现有 claim 租约基础上增加明确的 attempt 语义和数据库约束。业务幂等键固定为 `task_id + attempt`；随机 claim ID 只标识租约实例。领取事务同时检查任务状态、活动 run 和租约有效性，并在同一事务内创建或复用 run。

备选方案：仅依赖内存锁或随机 claim UUID。拒绝原因：多副本和进程重启后无法恢复，也不能阻止重复 run。

### 3. Reconciliation 状态机

对账读取可恢复的活动 run、claim 和 Supervisor 运行快照，按以下顺序处理：

1. 释放过期 claim，拒绝旧 worker 的后续写入。
2. 标记不存在进程的 run，读取退出摘要并进入恢复/失败决策。
3. 中断已取消或不再满足派发条件的待启动 run。
4. 回收终态 run 的 workspace/子进程并追加幂等通知。
5. 更新队列、延迟、stall 和修正指标。

状态修正采用条件更新或版本校验，避免较晚的旧事件覆盖较新的终态。

### 4. 失败分类与退避

将错误分为可重试传输/基础设施错误、stall、用户取消、审批拒绝、质量门禁失败和不可重试配置/权限错误。退避由基础延迟、指数因子、随机抖动、最大延迟和最大 attempt 共同决定；这些值来自配置并在 run 快照中固化。传输恢复在同一 attempt 内完成，超过恢复上限才进入新的 attempt 或失败。

备选方案：所有错误统一立即重试或无限重试。拒绝原因：会放大故障、产生重复执行并掩盖不可修复错误。

### 5. 数据库迁移与兼容

优先使用添加字段、索引和约束的向后兼容迁移；已有 run 的 attempt 以 1 作为回填默认值，历史状态不重写。新字段完成回填和验证后才由 worker 强制使用。回滚只撤销未被旧版本读取的新增列/索引，禁止删除已有审计和事件。

### 6. 测试与观测

使用真实 PostgreSQL 集成测试验证多 worker 竞争、租约过期、重启恢复、断流、stall、取消和重复终态事件；Supervisor 使用可控测试替身，不连接生产数据库。指标采用低基数标签，日志和审计复用统一脱敏器。交付前通过 `cargo flow verify --all`，并附运行验收记录。

## Risks / Trade-offs

- [时钟漂移导致租约误过期] → 使用数据库时间或统一时钟来源，并在续租和对账测试中覆盖边界。
- [reconciliation 误判健康进程] → 以 Supervisor 快照和最后心跳双重确认，修正前记录诊断上下文。
- [重试造成外部副作用重复] → 仅对幂等操作自动重试；高风险工具仍由审批策略控制。
- [新增索引影响现有查询] → 先在测试数据量上验证执行计划，再分阶段部署并监控锁等待。
- [长时间 backlog] → 暴露队列深度和派发延迟，容量不足时保持排队而不是丢弃任务。

## Migration Plan

1. 添加 attempt、actor、租约续期、心跳、重试元数据和必要唯一索引的迁移，并为历史记录回填安全默认值。
2. 部署只读兼容版本，验证新旧 worker 均能读取迁移后的数据。
3. 启用 reconciliation 和稳定幂等逻辑，观察队列、重复 claim、stall 和恢复指标。
4. 启用退避与终态清理策略，完成并发、重启和传输断流运行验收。
5. 若需回滚，先停止新 worker，再恢复旧逻辑；保留新增审计、事件和失败记录，不重写历史。

## 实现字段映射与评审记录

| 既有事实 | 本变更字段/约束 | 语义边界 |
| --- | --- | --- |
| `devrail_tasks.scheduler_claim_token/claimed_at` | 保留并增加可配置 lease 校验 | UUID 只标识租约实例，不作为业务幂等键 |
| run 的随机/用户幂等键 | `devrail_runs.attempt`、`uq_devrail_run_task_attempt` | 自动调度键固定为 `scheduler:{task_id}:{attempt}` |
| `recovery_attempts` | `scheduler_retry_count`、`scheduler_retry_at`、`scheduler_last_error` | 前者是同 attempt 传输恢复次数；后者是跨 attempt 调度重试，二者不重复 |
| `started_at/completed_at` | `last_heartbeat_at`、`last_event_at` | 生命周期时间不替代活性时间；stall 取最新活性事实 |
| 普通用户 actor | `actor_type` 与 System Actor 审计 | System 审计的 `actor_user_id` 为 NULL，组织/部门/所有者仍显式保留 |
| thread/turn 恢复字段 | `parent_run_id`、`parent_turn_id` | thread/turn 是 Harness 恢复位置；parent 字段是 run 谱系，不代替 continuation 模型 |
| 进程退出状态 | `retry_reason`、`recovery_suggestion`、`cleanup_status` | 原因、用户恢复建议和资源清理结果分别持久化 |

评审结论：上述字段没有与既有事件、审批或通知事实重复。历史 run 按创建顺序回填 attempt；旧版不显式传 attempt 时由 sentinel trigger 分配下一编号，新版显式 attempt 仍由唯一约束提供确定性冲突。`cargo flow scope` 已将本 change 正确识别为 backend、frontend、workflow 三组件；原任务文字中的“backend/workflow”已按实际 OpenAPI/Angular 影响修正。

## 固化策略与配置

启动时校验 poll、claim lease、retry base/max/jitter、stall 和 priority aging。claim lease/stall 必须大于 poll，retry max 不小于 base。调度排序先计算 priority aging 后依次使用截止时间、创建时间和 ID；等待每满一个 aging 周期提升一级有效优先级。

run policy 快照保存最大 attempts、退避、抖动、stall 和 priority aging。运行中环境变量或策略变化不修改现有快照；任务取消和启动阶段环境失效由下一轮 reconciliation 中断，后续 dependency 传播由 SY-DAG change 实现。

传输 EOF 只在数据库已持久化 thread 时恢复同一 run/attempt。恢复前先退出旧控制循环、删除旧控制通道并释放并发槽位，再发送 `thread/resume`；没有 thread 时失败并按策略进入下一 attempt，避免创建无法证明幂等的重复 Agent。
