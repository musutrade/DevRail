## Purpose

为 Symphony 调度器提供与具体任务存储解耦、保持多租户数据范围且可审计的任务访问契约，并在派发前固化可复现的任务输入。

## ADDED Requirements

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

### Requirement: Dispatch eligibility is evaluated consistently

TaskTracker MUST 只把状态为 `queued`、环境启用且健康、没有活动 run、未处于退避并符合截止时间策略的任务列为可派发；不满足条件的任务 MUST 保持可查询且记录等待原因。本变更不增加 DAG 依赖模型。

#### Scenario: Task is eligible

- **WHEN** 排队任务满足环境、活动 run、退避和截止时间条件
- **THEN** TaskTracker 可在原子 claim 流程中返回该任务，并保留既有优先级和饥饿防护排序

#### Scenario: Task is temporarily ineligible

- **WHEN** 环境不健康、已有活动 run 或重试时间尚未到达
- **THEN** TaskTracker 不派发该任务、不将其标为成功或永久丢弃，并提供低基数等待原因
