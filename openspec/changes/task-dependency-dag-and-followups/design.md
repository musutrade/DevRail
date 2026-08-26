## Context

见 [proposal.md](proposal.md) 的 Why。当前 `devrail_tasks`、TaskTracker 原子 claim、稳定 `task_id + attempt`、Scheduler/System Actor、workflow 快照和 reconciliation 已经存在；本设计需要在不改变 Harness Supervisor 唯一启动边界、不引入新基础设施的前提下，使依赖资格成为数据库事实，并让 Agent 只能通过受控运行时接口创建后续任务。

现有任务状态没有 `skipped`，候选任务以 `queued` 为入口。新增终态和依赖等待原因必须同步状态机、历史、API、OpenAPI、Angular 与旧数据兼容；所有关联表和查询继续在 SQL 中强制组织、部门和所有者范围。

## Goals / Non-Goals

**Goals:**

- 使用 PostgreSQL 邻接表表达同组织 DAG，并在写事务内完成范围、幂等和环检测。
- 让候选扫描、claim、reconciliation 和终态传播观察同一依赖事实。
- 固化每条边的失败、取消、超时传播策略，避免配置 reload 改变已排队任务。
- 为 Agent 后续任务提供可配额、可去重、可审计的窄 API。
- 通过任务详情、SSE、事件、审计和低基数指标提供可诊断关系。

**Non-Goals:**

- 不实现 per-task workspace/worktree、continuation turns、外部 tracker adapter 或质量门禁自动修复 run。
- 不引入 Redis、图数据库或独立 Symphony 服务。
- 不允许 Agent 自行选择组织、提升权限或直接执行 SQL。
- 不在本批实现通用工作流图编辑器；前端只提供任务级依赖查看与受权编辑入口。

## Decisions

### 1. PostgreSQL 邻接表是依赖事实来源

新增 `devrail_task_dependencies`，保存 `organization_id`、`department_id`、`owner_user_id`、下游任务、前置任务、三个终态动作、创建主体、来源、幂等键、创建/更新时间。使用 `(task_id, organization_id)` 复合外键约束两端组织一致，使用唯一键禁止重复边，使用检查约束禁止自依赖。Repository 的列表、变更和候选查询同时带组织、部门、所有者范围。

选择邻接表是因为当前图规模由单项目任务数量约束，递归 CTE 足以支持环检测和上下游查询，并能与任务状态、claim 和审计放在同一事务中。物化路径会显著增加边变更维护成本；图数据库或 Redis 会引入双写和新的恢复事实来源，因此不采用。

### 2. 变更事务使用递归 CTE 环检测与确定性锁顺序

新增或整批替换依赖时，Repository 按任务 ID 排序锁定下游任务、所有前置任务及相关边，再执行递归 CTE，判断任一拟新增前置任务是否已经能到达下游任务。整批变更在一个事务中完成；任一范围、版本、幂等或环检测失败都会回滚。

并发事务通过确定性锁顺序和提交前的数据库环检测避免两个合法局部写入组合成环。测试必须覆盖相反方向并发加边、批量替换和重复请求。仅在 Service 内遍历预读图无法防止并发写偏差，因此不采用。

### 3. 传播策略按边固化，安全缺省为等待

每条边保存 `failure_action`、`cancelled_action`、`timeout_action`，枚举为 `wait | skip | fail`，缺省为 `wait`。策略在任务排队或依赖创建时经过 Service 校验并固化；后续 workflow reload 不会改变既有边。`success` 始终表示该边已满足。

当任一前置任务未成功时，下游任务保持 `queued`，并由可重建的依赖诊断查询返回 `dependency_pending` 等低基数原因。策略要求 `skip` 时新增 `skipped` 任务终态；要求 `fail` 时进入 `failed`。若多条终态边同时触发，确定性优先级为 `fail > skip > wait`，并保存所有可见阻塞边供诊断。

不新增持久化 `blocked` 状态，避免等待事实与边状态双写；`queued + dependency wait reason` 是可恢复事实。`skipped` 是明确终态，因为它与用户取消和执行失败语义不同。

### 4. 候选、claim 与 reconciliation 复用同一资格谓词

Repository 将“全部依赖成功且没有待传播终态”封装为可复用 SQL 资格谓词，候选列表和 `FOR UPDATE SKIP LOCKED` claim 均使用该谓词。claim 事务在创建 run 前再次检查，防止候选扫描后的依赖竞态。

每轮 reconciliation 先读取需要传播的前置终态，在事务中锁定下游任务和相关边，按稳定传播键写状态历史、审计、事件与 outbox，再进行 dispatch。传播幂等键由 `organization_id + dependency_id + prerequisite_terminal_version + action` 构成；重复进程退出、超时或 webhook 只能命中原结果。

将依赖资格只放在内存调度器会让多 worker 观察不同图状态，因此不采用。

### 5. 后续任务请求使用独立幂等记录和封闭输入

新增 `devrail_followup_requests`，保存组织/部门/所有者范围、来源 task/run、调用 actor、幂等键、规范化 payload 摘要、结果 task、状态和脱敏错误类别，并以 `(organization_id, source_run_id, idempotency_key)` 唯一。相同键和摘要返回原结果；相同键但摘要不同返回冲突。

Handler 只解析身份、run 绑定和封闭 DTO；Service 校验来源 run、schema、项目/仓库/环境范围、RBAC、单请求数量、每 run 累计数量、标题/正文大小和允许的依赖策略；Repository 在单事务内占用配额、创建任务、依赖、历史、审计、事件和幂等结果。Agent 不能提交组织、所有者、权限、网络或工具能力字段，这些值从来源 task/run 和平台策略派生。

直接复用普通任务创建接口会扩大 Agent 可控字段并使断流重放难以绑定来源 run，因此不采用。

### 6. API、事件和前端以可见范围投影关系

任务详情响应新增前置任务、下游任务、边状态、传播策略、阻塞原因、`creation_source`、来源 task/run 和 follow-up request 状态。Repository 先按当前数据范围裁剪关系；范围外节点既不返回标识，也不通过计数或错误泄露存在性。

变更事件只包含通知 ID、事件类型、脱敏摘要和深链接所需资源 ID；SSE 收到后由前端重新获取任务详情。指标标签限于动作、结果和原因代码，不使用 task ID、组织 ID、标题或幂等键等高基数字段。

Angular 任务详情使用现有 feature/service/store 模式展示上下游列表、阻塞摘要和创建来源；关系编辑使用明确权限控制、冲突反馈和加载状态，所有文案、Tooltip 与 ARIA 标签使用简体中文。

### 7. ADR 与证据链作为验收事实

ADR-0004 在规划阶段保持 `Proposed`。实现完成并通过 PostgreSQL 并发测试、双端测试和 `cargo flow verify --all` 后，将状态改为 `Accepted`，并补充迁移、核心模块、测试名和 PR 证据。需求 ID `SY-DAG-001` 至 `SY-DAG-004` 必须在需求文档证据矩阵中从未实现更新为已实现或明确的部分实现。

## Risks / Trade-offs

- [递归 CTE 在超大图上变慢] -> 限制单任务前置边数、后续任务配额和遍历深度，建立双向组合索引，并记录无 task ID 标签的查询耗时指标。
- [并发加边形成写偏差或死锁] -> 使用确定性任务锁顺序、事务内最终环检测、有限重试和真实 PostgreSQL 并发测试。
- [终态传播与用户状态变更竞争] -> 在同一事务锁定下游任务，使用 revision/允许转换校验；用户取消或已终态优先，传播记录冲突结果而不覆盖。
- [新增 `skipped` 破坏旧客户端枚举] -> 同步 Rust/OpenAPI/Angular 枚举和兼容测试；旧客户端按未知终态只读展示，部署时先迁移后滚动应用。
- [Agent 通过大量后续任务耗尽资源] -> 同时限制单请求、单 run、单 task 深度和组织级速率，所有拒绝写低基数审计，不信任 Agent 提交的范围字段。
- [事件/outbox 与任务状态重复写入] -> 事务内稳定 source key 和唯一约束，dispatcher 仍是唯一外部推送调用方。

## Migration Plan

1. 新增 `skipped` 状态约束、依赖表、后续请求表、必要任务来源字段、复合外键、唯一约束和索引；以空图作为旧数据的兼容状态。
2. 部署只读模型、Repository 查询和 API 字段，旧任务返回空依赖和 `manual`/`legacy` 创建来源。
3. 启用依赖写 API 与事务环检测，再启用 TaskTracker 资格谓词；此时无边旧任务行为不变。
4. 启用 reconciliation 传播和 Agent 后续任务入口，先使用保守配额及 `wait` 缺省策略，观察冲突、传播和延迟指标。
5. 更新 Angular、OpenAPI、运维文档和证据矩阵，完成 PostgreSQL 并发、恢复、双端及全量门禁后接受 ADR。

回滚时先关闭依赖写入、传播和后续任务 feature flag，使调度器忽略新入口但保留表中事实；回滚应用版本前将仍为 `skipped` 的任务按审计依据转换到兼容终态。数据库表和审计数据不在紧急回滚中删除，待确认无旧 worker 后再通过独立迁移清理。
