# Symphony Orchestrator 运行手册

更新日期：2026-08-26

本文覆盖 OpenSpec change `symphony-orchestrator-reconciliation`、`tasktracker-workflow-foundation` 与 `task-dependency-dag-and-followups` 已实现的 TaskTracker、调度控制循环、任务依赖传播、受控 follow-up、仓库 workflow 和 Harness 恢复能力。外部 tracker、独立 workspace manager 和 continuation turn 不在本手册范围内。

## 启动配置

所有配置在后端启动时校验。非法值会阻止服务启动，不会静默回退。

| 变量 | 默认值 | 有效范围/关系 | 作用 |
| --- | ---: | --- | --- |
| `DEVRAIL_HARNESS_MAX_CONCURRENCY` | 2 | 正整数 | 单副本 Harness 并发槽位 |
| `DEVRAIL_RUN_MAX_DURATION_SECS` | 3600 | 正整数 | 单次 run 总时限 |
| `DEVRAIL_RUN_GRACEFUL_INTERRUPT_SECS` | 10 | 正整数 | 中断后强制杀进程前的等待时间 |
| `DEVRAIL_SCHEDULER_POLL_SECS` | 10 | 1–3600 | 控制循环间隔 |
| `DEVRAIL_SCHEDULER_CLAIM_LEASE_SECS` | 60 | 2–86400，且大于轮询间隔 | 待启动任务的 claim 租约 |
| `DEVRAIL_SCHEDULER_RETRY_BASE_SECS` | 1 | 1–86400 | 指数退避基础延迟 |
| `DEVRAIL_SCHEDULER_RETRY_MAX_SECS` | 300 | 1–604800，且不小于基础延迟 | 自动重试最大延迟 |
| `DEVRAIL_SCHEDULER_RETRY_JITTER_PERCENT` | 20 | 0–100 | OS 随机源生成的正向抖动比例 |
| `DEVRAIL_SCHEDULER_STALL_SECS` | 120 | 2–86400，且大于轮询间隔 | 无 stdout/stderr 有效活动的 stall 阈值 |
| `DEVRAIL_SCHEDULER_PRIORITY_AGING_SECS` | 3600 | 60–604800 | queued 任务每等待一个周期提升一级有效优先级 |
| `DEVRAIL_WORKFLOW_RELOAD_SECS` | 15 | 1–3600 | workflow 对账轮询间隔 |
| `DEVRAIL_WORKFLOW_RELOAD_JITTER_PERCENT` | 20 | 0–100 | workflow 对账正向抖动比例 |

claim 租约只保护“已领取、尚未启动”的窗口。run 创建后并发额度由 Supervisor reservation 持有，数据库事实由 run 状态、心跳和事件游标维护。实际调度/重试策略会写入 run policy 快照，运行中修改环境变量不会改变现有 run。

## WORKFLOW.md 运行语义

仓库作者按 [仓库工作流契约](workflow-contract.md) 维护根目录 `WORKFLOW.md`。文件缺失时使用版本化安全默认值；路径越界、未知字段/模板能力或安全权限扩大都会拒绝候选版本。

workflow 在任务进入 `queued` 时锁定。文件后续变化不会改变已排队任务和活动 run；合法 reload 只影响之后入队的任务。任务详情和 run 详情应显示相同的来源、版本与摘要。不得通过数据库手工替换快照。

## 指标与建议告警

| 指标 | 含义 | 建议告警 |
| --- | --- | --- |
| `devrail_scheduler_queue_depth` | 当前 queued 任务数 | 15 分钟持续增长且无 completed run |
| `devrail_scheduler_dispatch_total{outcome}` | started、capacity、stale_claim、failed 等固定低基数结果 | `failed`/`permanent_failure` 5 分钟突增 |
| `devrail_scheduler_dispatch_latency_seconds` | 从任务创建到实际派发的等待时间 | P95 超过业务 SLO |
| `devrail_scheduler_claim_conflict_total` | 旧租约/旧 worker 被拒绝次数 | 10 分钟持续增长，检查时钟与副本抖动 |
| `devrail_scheduler_retry_total` | 自动 retry 次数 | 与供应商或数据库错误同时突增 |
| `devrail_scheduler_stall_total` | stall 和进程缺失修正数 | 任意持续增长均应调查 Harness 日志 |
| `devrail_run_active` | starting/active/awaiting approval 数量 | 长期等于并发上限且队列增长 |
| `devrail_run_reconciliation_total{outcome}` | claim、stale、取消、环境失效和重启修正 | `stale_run`、`retry_exhausted` 出现即告警 |
| `devrail_task_dependency_propagation_total{outcome}` | 依赖终态传播结果（固定标签） | `applied` 持续增长且下游失败率异常时调查依赖配置 |
| `devrail_task_dependency_conflict_total{outcome}` | 环、版本或幂等冲突次数 | `cycle`/`revision` 突增时检查并发编辑 |
| `devrail_task_dependency_query_duration_seconds` | 任务关系查询耗时 | P95 超过 API SLO 时检查图规模和索引 |
| `devrail_agent_followup_total{outcome}` | 受控 follow-up 接受、重放和拒绝结果 | `rejected_policy` 或 `unavailable` 突增时检查 Agent 工具调用 |
| `devrail_workflow_reload_total{outcome}` | accepted、unchanged、rejected、fallback 等固定结果 | `rejected`/`fallback` 持续增长 |
| `devrail_workflow_reload_duration_seconds` | 单轮 workflow 对账耗时 | P95 接近轮询间隔 |
| `devrail_workflow_reload_healthy` | 最近一轮对账是否成功 | 连续为 0 |

指标标签由代码白名单归一化，未知值统一记为 `other`，不得把 task、run、组织或错误文本写入标签。

## 故障处理

### 队列持续堆积

1. 查看 queue depth、dispatch latency 和 `dispatch_total{outcome="capacity"}`。
2. 确认 Harness 并发上限、宿主机资源和运行平均时长。
3. 检查任务详情的下一次重试时间、最后错误和环境 enabled 状态。
4. 不直接修改数据库 claim；修复容量或环境后，任务会在下一轮自动参与调度。

### claim 冲突或旧 worker 写入

1. 检查多个后端副本的系统时钟与数据库连接延迟。
2. 确认 claim lease 大于 poll interval，避免配置导致正常 tick 被误判过期。
3. 旧 token 的续租/状态写入会返回 false；不要人工复用 token。

### Harness 无事件或断流

- 无事件达到 stall 阈值：Supervisor 杀进程、记录 `stall`、清理 run，并在 attempt 未耗尽时带退避重新排队。
- stdout/传输 EOF：最多在同一 run/attempt 恢复两次；只有已持久化 thread 时才执行 `thread/resume`。没有 thread 时进入下一 attempt，避免创建不可追踪的重复 Agent。
- 浏览器 SSE 断开：前端按 cursor/Last-Event-ID 补拉，不会触发 Harness 恢复或改变 task/run 事实。

### 后端进程重启

1. 启动阶段扫描 starting/active run；有 thread 的 run 使用原 thread/turn 恢复。
2. 没有 thread 的 run 原子标记 `supervisor_restart` 失败，并创建一次站内通知/outbox 和 System Actor 审计。
3. 周期 reconciliation 会处理数据库 active 但 Supervisor 无进程的 stale run，避免无限期 active。
4. 通过 run 详情核对 `exitReason`、`traceId`、`recoverySuggestion` 和 `cleanupStatus`。

### WORKFLOW.md 非法或删除

1. 查看 `devrail_workflow_reload_total`、System Actor 的 `devrail.workflow.reject` 审计和环境对应的最近摘要；诊断不会包含正文、完整路径或 secret。
2. 相同坏候选只创建一条失败证据并累加次数，last-known-good 继续服务新入队任务；不要手工清空版本表。
3. 修复文件后等待下一轮对账。删除文件会发布安全默认 workflow，不会修改既有 task/run。
4. 在任务与 run 详情核对 workflow 来源、版本和摘要；三元身份不一致时 run 创建会 fail closed。

### 任务依赖与受控 follow-up

- 依赖关系在同组织范围内由 PostgreSQL 保存；每条边的失败、取消和超时动作固定为 `wait`、`skip` 或 `fail`，默认 `wait`。
- 调度器每轮 dispatch 前先执行依赖 reconciliation。全部前置任务成功后下游恢复为可派发；终态动作按 `fail > skip > wait` 处理，跳过任务显示 `skipped`。
- 依赖修改只允许草稿/排队任务，并要求 revision 与幂等键；环冲突或范围外节点不会写入部分结果。
- Agent 不能调用浏览器 HTTP 接口创建后续任务。只有受控 app-server JSONL 事件 `devrail/followup.create` 会由 Supervisor 绑定当前 run/task、组织、部门、所有者、仓库、环境和权限后调用内部 Service。
- 单个 run 最多 8 个 follow-up，单任务深度最多 8 层；断流、EOF 或进程重启重放同一幂等键只返回原任务，不重复占额、事件或通知。

验证证据：`dependency_claim_and_terminal_propagation_are_deterministic`、
`dependency_replace_rejects_cycles_atomically`、
`dependency_relation_queries_hide_out_of_scope_prerequisites` 和
`followup_replay_is_idempotent_and_does_not_consume_quota` 使用隔离 PostgreSQL
数据库执行；`cargo flow verify --components backend` 与
`cargo flow verify --components frontend` 是发布前必须通过的范围门禁。

发布/回滚：先应用 `20260905100000_add_task_dependency_dag_and_followups.sql`，确认历史任务的 `legacy` 来源和空依赖可查询，再滚动部署后端与前端。回滚应用版本时关闭依赖写入、传播和 follow-up 工具入口，保留新增表、审计和事件；不得直接删除 `skipped` 历史或幂等记录。

## 发布与回滚

迁移 `20260826030000_add_symphony_scheduler_reliability.sql` 与 `20260904100000_add_tasktracker_workflow_foundation.sql` 均为添加式迁移。后者为历史 queued/active task 和 run 回填明确的 `legacy/default` 身份，并保留旧 INSERT 默认值；回滚时保留新增快照、版本、失败证据和状态历史表。

发布顺序：

1. 备份 PostgreSQL 并执行 migration job。
2. 滚动部署新后端，确认启动配置校验通过。
3. 观察 queue、claim conflict、stall 和 reconciliation 指标。
4. 再部署前端生成契约。

应用回滚时先停止新 worker，再回滚后端镜像；保留新增列、索引、触发器、审计和通知，不执行 DROP。旧版本可依赖兼容触发器继续创建 run。数据库结构回退属于破坏性操作，必须另立迁移并经过数据备份、停机和专项评审；常规故障优先向前修复。

## 验收命令

```bash
export PATH="/home/gem/.npm-global/bin:$PATH"
openspec validate symphony-orchestrator-reconciliation --strict
openspec validate tasktracker-workflow-foundation --strict
cargo flow scope
cargo flow verify --all
```

真实 PostgreSQL 故障测试使用隔离的 `TEST_DATABASE_URL`，不得连接生产数据库。完整证据矩阵见 [Symphony 专项需求](symphony-devrail-requirements.md#124-p0-调度可靠性证据矩阵2026-08-26)。
