## ADDED Requirements

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
