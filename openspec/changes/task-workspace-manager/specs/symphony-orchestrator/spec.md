## MODIFIED Requirements

### Requirement: Reconciliation before dispatch

每一轮调度 MUST 先对账任务状态、依赖状态、run 状态、claim、workspace 和 Agent 进程，再进行新的任务派发。发现不一致时 MUST 按确定性规则恢复、重新排队、保持依赖等待，或按固化依赖策略标记 `skipped`/`failed`，并写入原因和审计事件。依赖传播、workspace 占用和 claim 资格检查 MUST 以同一数据库事实为准；Agent 只能在 workspace 成功绑定且必要的 `before_run` hook 完成后启动。

#### Scenario: Database says active but process is gone

- **WHEN** run 在数据库中仍为活动状态，但对应 Agent 进程已退出
- **THEN** 系统读取退出摘要，按重试策略恢复或标记明确失败，不留下无限期活动 run，并对账关联 workspace 的占用和清理状态

#### Scenario: Task is cancelled while dispatch is pending

- **WHEN** 任务在 claim 后、workspace 绑定或 Agent 启动前被取消
- **THEN** 调度器释放 claim，不启动 Agent，执行幂等 workspace 清理，并将任务和 run 更新为可查询的取消状态

#### Scenario: Dependency outcome changes before dispatch

- **WHEN** reconciliation 发现排队任务的前置任务刚进入成功、失败、取消或超时终态
- **THEN** 系统在新派发前清除等待状态或按固化策略幂等传播 `wait`、`skip`、`fail` 结果，并使 claim 查询观察到相同结论；不为未满足依赖的任务创建 workspace

#### Scenario: Workspace creation or hook fails before dispatch

- **WHEN** workspace 创建、基础提交校验或 `before_run` hook 失败
- **THEN** 系统不启动 Agent，保存脱敏原因和 workspace 清理结果，按策略重新排队或明确失败，并记录 Scheduler/System Actor 审计

#### Scenario: Terminal run requires workspace cleanup

- **WHEN** run 进入成功、失败、取消或中断终态
- **THEN** reconciliation 按稳定 run/workspace 键只执行一次终态 hook 和 cleanup；清理失败不会覆盖原 run 结论，且 workspace 保持不可复用直到清理成功
