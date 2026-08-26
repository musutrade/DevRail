## MODIFIED Requirements

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
