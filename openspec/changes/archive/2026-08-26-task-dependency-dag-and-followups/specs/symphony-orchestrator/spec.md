## MODIFIED Requirements

### Requirement: Reconciliation before dispatch

每一轮调度 MUST 先对账任务状态、依赖状态、run 状态、claim、Agent 进程和 workspace，再进行新的任务派发。发现不一致时 MUST 按确定性规则恢复、重新排队、保持依赖等待，或按固化依赖策略标记 `skipped`/`failed`，并写入原因和审计事件。依赖传播与 claim 资格检查 MUST 以同一数据库事实为准。

#### Scenario: Database says active but process is gone

- **WHEN** run 在数据库中仍为活动状态，但对应 Agent 进程已退出
- **THEN** 系统读取退出摘要，按重试策略恢复或标记明确失败，不留下无限期活动 run

#### Scenario: Task is cancelled while dispatch is pending

- **WHEN** 任务在 claim 后、Agent 启动前被取消
- **THEN** 调度器释放 claim，不启动 Agent，并将任务和 run 更新为可查询的取消状态

#### Scenario: Dependency outcome changes before dispatch

- **WHEN** reconciliation 发现排队任务的前置任务刚进入成功、失败、取消或超时终态
- **THEN** 系统在新派发前清除等待状态或按固化策略幂等传播 `wait`、`skip`、`fail` 结果，并使 claim 查询观察到相同结论

### Requirement: System actor and auditable terminal handling

自动调度、依赖传播和 reconciliation MUST 使用独立的 Scheduler/System Actor，并在状态历史中记录触发来源、策略版本和 trace。终态处理 MUST 幂等，重复退出、超时或 webhook 事件不得重复清理、通知、传播依赖状态或创建后续任务；Agent 后续任务请求 MUST 通过受控 API 并绑定来源 run 的稳定幂等记录。

#### Scenario: Scheduler updates a task

- **WHEN** worker 自动领取、重试、传播依赖结果或修正任务状态
- **THEN** 审计记录显示 Scheduler/System Actor、触发原因和关联 trace，而不是伪造普通用户会话

#### Scenario: Duplicate terminal event

- **WHEN** 同一 run 的终态事件被接收两次或更多次
- **THEN** 系统只保留一个终态结果、一组幂等通知、一次依赖传播和一次清理结果，后续事件记录为重复事件

#### Scenario: Duplicate follow-up request after recovery

- **WHEN** Agent 或恢复流程在传输断流后重放已成功的后续任务提议
- **THEN** 系统返回原任务和原依赖结果，不创建第二个任务，也不重复消耗配额或发送事件
