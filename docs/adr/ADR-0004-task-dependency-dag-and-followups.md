# ADR-0004：PostgreSQL 任务依赖 DAG 与受控后续任务

- 状态：Accepted
- 日期：2026-08-26
- 决策人：DevRail 项目维护者
- 关联需求：[SY-DAG-001 至 SY-DAG-004](../symphony-devrail-requirements.md#55-任务依赖与后续任务p1)、[SY-TRACK-005](../symphony-devrail-requirements.md#51-tasktracker-与任务生命周期p1)、[SY-ORCH-007](../symphony-devrail-requirements.md#53-orchestrator-调度循环p0)、[SY-RECON-004](../symphony-devrail-requirements.md#58-reconciliation-与终态处理p0)

## 背景

DevRail 已具备 PostgreSQL TaskTracker、原子 claim、稳定 attempt、workflow 快照和 Harness Supervisor，但任务只能独立排队。缺少依赖事实会让下游任务过早派发；允许 Agent 直接调用普通任务创建或写数据库，又会绕过范围、权限、配额和断流重放所需的幂等边界。

## 决策

1. 使用 PostgreSQL 邻接表保存同组织任务依赖，不引入图数据库、Redis 或第二套调度事实来源。
2. 创建和修改依赖时，在单个 Repository 事务内按确定性顺序锁定任务，使用递归 CTE 完成同组织范围校验、重复校验和环检测。
3. 每条边固化失败、取消和超时动作，动作限定为 `wait`、`skip` 或 `fail`，安全缺省为 `wait`；运行时 reload 不改变既有依赖策略。
4. 依赖等待使用 `queued` 状态加可重建的阻塞原因；策略跳过使用新增的 `skipped` 终态，避免与用户取消或执行失败混淆。
5. TaskTracker 候选 SQL、原子 claim 和 reconciliation 使用同一依赖资格事实；终态传播使用稳定幂等键，并由 Scheduler/System Actor 写历史、审计、事件和 outbox。
6. Agent 只能通过绑定来源 task/run 的受控 follow-up API 创建后续任务。接口采用封闭 schema，从服务端派生组织与权限，限制数量和深度，并以来源 run 加幂等键去重。
7. 任务详情、OpenAPI 和 Angular 展示当前用户可见的上下游关系、阻塞原因、传播策略和创建来源；日志、事件、指标和推送继续执行脱敏与低基数约束。

## 取舍与后果

### 正面影响

- 依赖、任务状态、claim、审计和幂等位于同一事务边界，多 worker 与重启后仍可确定性恢复。
- 无依赖的历史任务保持原有派发语义，DAG 能力可以分阶段启用。
- 后续任务具有明确来源、配额和重放语义，Agent 无法借此扩大权限。
- 不新增运行基础设施，继续复用现有 PostgreSQL 运维和备份能力。

### 代价与风险

- 递归 CTE 和并发加边需要真实 PostgreSQL 压力及死锁测试，并限制边数和遍历深度。
- 新增 `skipped` 终态需要同步状态机、API 契约、前端枚举、历史触发器和旧客户端兼容行为。
- 终态传播与用户取消、重试可能竞争，必须使用行锁、revision 和允许转换规则，不得覆盖已完成终态。
- follow-up 配额需要初始保守值和指标观察，不能用无限自动拆分掩盖任务设计问题。

## 拒绝的方案

- 不在 Service 内预读整张图后写边；该方式无法防止并发写偏差。
- 不把依赖状态仅保存在调度器内存或前端；PostgreSQL 是唯一事实来源。
- 不用 `cancelled` 表示策略跳过，也不把依赖等待新增为需要双写的持久化 `blocked` 状态。
- 不让 Agent 复用拥有全部字段的普通任务创建接口，也不接受 Agent 提交的组织、所有者或权限字段。
- 不在 Handler 中执行 SQL、传播任务状态或调用外部推送供应商。

## 验收条件

- 同组织依赖、跨组织拒绝、自依赖、重复边、串行与并发成环均有 PostgreSQL 测试。
- 依赖等待不进入候选或 claim；成功解除阻塞，失败/取消/超时按 `wait | skip | fail` 确定性传播。
- 重复终态事件和传输断流后的 follow-up 重放不会重复创建任务、依赖、通知或审计事实。
- API/OpenAPI/Angular 可查看上下游、阻塞原因和创建来源，且通过范围与敏感字段测试。
- `cargo flow verify --all`、arc-flow 审计、供应链检查和 CodeQL 通过，需求证据矩阵与运维文档已更新。

## 关联实施

本 ADR 由 `task-dependency-dag-and-followups` OpenSpec change 跟踪。实现证据包括：

- 迁移：`20260905100000_add_task_dependency_dag_and_followups.sql`；
- 核心模块：`repositories/devrail.rs`、`services/devrail.rs`、
  `workers/task_scheduler.rs` 和 `workers/harness_supervisor.rs`；
- PostgreSQL 测试：`dependency_claim_and_terminal_propagation_are_deterministic`、
  `dependency_replace_rejects_cycles_atomically`、
  `dependency_relation_queries_hide_out_of_scope_prerequisites`、
  `followup_replay_is_idempotent_and_does_not_consume_quota`；
- 前端验证：任务详情 Vitest、桌面/移动 Playwright、真实全栈 smoke 和生产构建；
- 最终门禁：2026-08-26 `cargo flow verify --all` 全部通过，OpenSpec 严格校验通过。

- 实现提交：`dbc2dcc8d1321b9d4235b3be3ce529b44d767283`；
- 交付 PR：[musutrade/DevRail#75](https://github.com/musutrade/DevRail/pull/75)；
- 远端验证：CI、Supply chain security 和 arc-flow platform 均成功；CodeQL
  按工作流路径条件正常跳过。
