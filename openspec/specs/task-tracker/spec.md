# Task Tracker Specification

## Purpose

为 Symphony 调度器提供与具体任务存储解耦、保持多租户数据范围且可审计的任务访问契约，并在派发前固化可复现的任务输入。

## Requirements

### Requirement: Storage-independent task contract

Orchestrator MUST 只通过 TaskTracker 契约读取可调度任务、获取单任务状态、追加状态历史和更新调度元数据，不得要求调用方了解具体数据库表或 SQL。

#### Scenario: Scheduler reads dispatch candidates

- **WHEN** Orchestrator 请求当前可派发候选任务
- **THEN** TaskTracker 返回稳定排序的领域任务及调度元数据，调用方不需要访问具体存储实现

#### Scenario: Tracker operation fails

- **WHEN** tracker 因连接、并发冲突或数据校验失败而不能完成操作
- **THEN** 它返回可分类且不含敏感信息的错误，Orchestrator 可据此重试、跳过或终止，而不伪造成功

### Requirement: Scoped PostgreSQL tracker

DevRail PostgreSQL tracker MUST 在 SQL 查询和写入条件中强制组织、部门和所有者数据范围，并保持现有 claim、租约、attempt 和 System Actor 审计语义。外部 tracker 不能绕过 DevRail Task 或权限边界。

#### Scenario: Cross-organization task lookup

- **WHEN** 调用方在某组织范围内请求另一个组织的任务
- **THEN** PostgreSQL tracker 不返回也不修改该任务，并产生与未找到等价的安全结果

#### Scenario: Scheduler updates metadata

- **WHEN** System Actor 更新符合范围的 claim、重试时间或调度原因
- **THEN** 更新与状态历史在后端事务中完成，并记录操作者、原因、策略版本、trace 和时间

### Requirement: Immutable queued task snapshot

任务进入 `queued` 时 MUST 固化标题、目标、仓库、环境、验收标准、任务版本和已验证 workflow 的来源、声明版本、摘要及规范化内容。已排队任务的这些派发输入 MUST 不可原地修改；变更输入必须产生新任务版本，或显式取消后重建。

#### Scenario: Task enters queue

- **WHEN** 一个有效任务从可编辑状态进入 `queued` 或以 `queued` 状态创建
- **THEN** 系统在同一事务中持久化完整派发快照，后续 run 可仅凭该快照复现输入

#### Scenario: Queued task input is edited

- **WHEN** 调用方尝试原地修改已排队任务的目标、仓库、环境、验收标准或 workflow 身份
- **THEN** TaskTracker 拒绝修改并返回明确的版本冲突，且原快照保持不变

### Requirement: Valid and auditable lifecycle transitions

TaskTracker MUST 只允许任务状态机定义的转换，并在同一后端事务中追加不可变状态历史。转换结果 MUST 可查询，包含前后状态、操作者、原因和时间。

#### Scenario: Allowed transition

- **WHEN** 授权操作者以满足前置条件的任务执行合法状态转换
- **THEN** 当前状态与状态历史原子更新，读取方能查询完整转换依据

#### Scenario: Illegal transition

- **WHEN** 调用方请求状态机不允许的转换或使用过期任务版本
- **THEN** TaskTracker 不改变任务，返回包含当前状态与允许动作的安全冲突错误

### Requirement: Continuation lifecycle projection

TaskTracker MUST 将有效 continuation 请求投影为可查询的 `continuation_pending` 任务状态，并在同一后端事务中追加不可变状态历史。只有来源 run 已成功或失败终态、任务未取消、没有活动 run 且请求满足固化策略时，任务才能从来源终态进入 `continuation_pending`；成功派发后进入 `running`，child run 终态后投影其最终结果。每次转换 MUST 记录 continuation 请求、来源 run、child run（如已创建）、操作者、触发类型、原因、策略版本和时间。

#### Scenario: Valid continuation becomes pending

- **WHEN** 有效 continuation 请求绑定到没有活动 run 的成功或失败终态任务
- **THEN** TaskTracker 原子写入 `continuation_pending` 状态与历史，并使普通 queued claim 不再领取该任务

#### Scenario: Continuation is dispatched

- **WHEN** Orchestrator 为待处理 continuation 成功创建并绑定唯一 child run
- **THEN** TaskTracker 将任务从 `continuation_pending` 原子转换为 `running`，且状态历史可追溯到请求与来源 run

#### Scenario: Pending continuation is cancelled or rejected

- **WHEN** continuation 在 child run 启动前被取消或被确定性拒绝
- **THEN** TaskTracker 恢复请求前保存的成功或失败任务投影，追加取消或拒绝历史，且不修改来源 run 终态

#### Scenario: Continuation child run reaches terminal state

- **WHEN** continuation child run 进入成功、失败、取消或中断终态
- **THEN** TaskTracker 幂等投影该 child run 的任务终态并追加历史，重复终态事件不产生第二次转换

#### Scenario: Invalid continuation transition is requested

- **WHEN** 任务已取消、已有活动 run、当前状态或版本不匹配，或请求不属于该任务和来源 run
- **THEN** TaskTracker 不改变任务或历史，并返回包含当前状态与允许动作的安全冲突结果

### Requirement: Dispatch eligibility is evaluated consistently

TaskTracker MUST 只把状态为 `queued`、全部前置任务均为 `succeeded`、环境启用且健康、没有活动 run、未处于退避并符合截止时间策略的任务列为可派发；候选查询与原子 claim MUST 使用一致的依赖资格条件。不满足条件的任务 MUST 保持可查询且记录低基数等待原因；依赖终态要求 `skip` 或 `fail` 时，TaskTracker MUST 在派发前按固化策略完成幂等状态传播。

#### Scenario: Task is eligible

- **WHEN** 排队任务的全部前置任务已成功，并满足环境、活动 run、退避和截止时间条件
- **THEN** TaskTracker 可在原子 claim 流程中返回该任务，并保留既有优先级和饥饿防护排序

#### Scenario: Task is temporarily ineligible

- **WHEN** 依赖尚未全部成功、环境不健康、已有活动 run 或重试时间尚未到达
- **THEN** TaskTracker 不派发该任务、不将其标为成功或永久丢弃，并提供低基数等待原因

#### Scenario: Dependency changes between selection and claim

- **WHEN** 任务在候选扫描后、claim 提交前因依赖变化而不再满足派发条件
- **THEN** 原子 claim 再次校验依赖事实并跳过该任务，不创建 run，也不覆盖依赖传播产生的终态

#### Scenario: Terminal dependency policy prevents dispatch

- **WHEN** 依赖终态对应的固化动作要求将下游任务标记为 `skipped` 或 `failed`
- **THEN** TaskTracker 幂等保存终态与历史，任务不进入候选或 claim 结果
