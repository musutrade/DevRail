## ADDED Requirements

### Requirement: Continuation reconciliation and dispatch

每一轮 reconciliation MUST 在普通 queued task 派发前处理符合资格的 continuation 请求，并以请求幂等身份领取执行权。Orchestrator MUST 在确认任务状态、来源 run 终态、同 thread 身份、活动 run、策略限额和 workspace 准备结果后，创建一个具有新 turn 序号、continuation 运行种类和完整父级谱系的 child run；Agent 只能在请求、任务、run 和 workspace 绑定原子可恢复后启动。

#### Scenario: Eligible continuation is dispatched
- **WHEN** 待处理 continuation 满足任务、thread、限额、容量和 workspace 前置条件
- **THEN** Orchestrator 创建并绑定唯一 child run，在同一 Codex thread 启动新 turn，并将任务投影为运行中

#### Scenario: Concurrent workers claim one continuation
- **WHEN** 两个或更多 worker 同时领取同一 continuation 请求
- **THEN** 只有一个 worker 获得有效执行权，其余 worker 返回原请求或可重试冲突，且不会创建第二个 child run

#### Scenario: Restart finds an existing child run
- **WHEN** reconciliation 在重启后发现请求未标记已派发但幂等身份已关联 child run
- **THEN** Orchestrator 复用并修正现有绑定与任务投影，不新建 run 或重复启动 Agent

#### Scenario: Dispatch prerequisites become invalid
- **WHEN** 请求领取后、Agent 启动前出现任务取消、活动 run、策略超限、thread 不匹配、workspace 失败或来源证据失效
- **THEN** Orchestrator 不启动 Agent，释放或终结请求，按错误分类恢复原任务投影、延后重试或明确拒绝，并记录脱敏原因

#### Scenario: Continuation child run terminates
- **WHEN** continuation child run 成功、失败、取消或中断
- **THEN** Orchestrator 幂等完成请求、任务状态、终态 hook、workspace 清理、审计、指标和 outbox 处理，重复终态事件不重复产生副作用

#### Scenario: Pending request loses its claim
- **WHEN** continuation claim 因 worker 丢失心跳而过期且尚无已启动 child run
- **THEN** 另一个 worker 可重新领取同一请求并继续确定性派发，不增加 continuation 序号或累计次数
