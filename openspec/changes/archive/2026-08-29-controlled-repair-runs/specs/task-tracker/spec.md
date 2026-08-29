## ADDED Requirements

### Requirement: Repair lifecycle projection and human handoff

TaskTracker MUST 将有效 repair 请求、派发、执行、门禁重跑、取消、拒绝和人工交接投影为可查询的任务状态与不可变历史。每次转换 MUST 关联来源 run、repair 请求和 repair run（如已创建）、诊断证据身份、操作者或 System Actor、策略版本、审批结果和时间。来源失败结论 MUST 保留；repair 成功或失败不得覆盖来源 run 的状态、终态时间或历史。

#### Scenario: Repair request becomes pending

- **WHEN** 终态失败任务收到满足策略且没有活动 run 的有效 repair 请求
- **THEN** TaskTracker 原子记录待修复投影和历史，使普通 queued claim 不会误领取该任务

#### Scenario: Repair requires human action

- **WHEN** repair 被取消、拒绝、审批未通过、Hook 熔断、预算/次数耗尽或门禁再次失败
- **THEN** TaskTracker 追加人工交接历史和安全原因，保持来源失败可追溯，且不把任务伪造为成功或重新排队

#### Scenario: Repair run completes

- **WHEN** 关联 repair run 与其受影响门禁进入最终结果
- **THEN** TaskTracker 幂等投影修复结果和下一步允许动作；重复终态不产生第二次状态转换或人工处理项
