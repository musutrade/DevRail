# symphony-orchestrator Specification

## Purpose

为 DevRail 提供可靠、可恢复且可审计的 Symphony 调度控制循环，使任务在多 worker、进程重启和传输异常下仍能保持单次执行语义与最终一致状态。

## Requirements

### Requirement: Deterministic task claiming

调度器 MUST 只领取满足排队条件的任务，并使用任务 ID 与执行尝试编号构成稳定的业务幂等键。并发 worker 领取同一任务时，系统 MUST 只允许一个 worker 获得有效执行权。

#### Scenario: Concurrent workers claim one task

- **WHEN** 两个或更多 worker 同时领取同一个可调度任务
- **THEN** 只有一个 worker 获得该任务当前 attempt 的执行权，其余 worker 得到可重试的冲突结果，且不会创建第二个活动 run

#### Scenario: Repeated dispatch for the same attempt

- **WHEN** 同一任务和 attempt 因循环重入、网络重试或重复消息被再次派发
- **THEN** 系统返回已有 run 或幂等成功结果，不创建重复 run、不重复发送终态通知

### Requirement: Lease expiry and bounded concurrency

调度器 MUST 为执行权设置可续租、可过期的 claim。系统 MUST 遵守配置的活动 run 上限；容量不足时任务保持排队并显示可诊断原因。

#### Scenario: Claim expires after worker loss

- **WHEN** 持有 claim 的 worker 停止续租并超过租约期限
- **THEN** 其他 worker 可以安全重新领取该任务，旧 worker 不能继续推进该 attempt 的状态

#### Scenario: Capacity is exhausted

- **WHEN** 活动 run 数量达到并发上限
- **THEN** 新任务保持排队，记录跳过原因并在后续容量释放后重新参与调度

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

### Requirement: Retry backoff and stall recovery

对于标记为可重试的失败，系统 MUST 使用带抖动的指数退避、最大 attempt 和最大延迟。系统 MUST 检测无心跳、无事件、进程退出和传输断流等 stall，并在恢复、重新排队或失败之间作出可审计的确定性选择。

#### Scenario: Transient transport interruption

- **WHEN** Agent 与 app-server 之间发生可恢复的传输断流且尚未达到恢复上限
- **THEN** 系统在同一 task/attempt 上执行幂等恢复，保留已持久化事件，不创建重复 Agent 执行

#### Scenario: Retry limit is reached

- **WHEN** 同一任务达到最大 attempt 或错误被判定为不可重试
- **THEN** 任务进入明确失败状态，保存脱敏根因、trace/log 引用和恢复建议，并停止自动重试

#### Scenario: Stall is detected

- **WHEN** run 在配置的 stall 阈值内没有心跳或有效事件
- **THEN** 系统按策略中断并清理子进程，随后恢复、重新排队或标记失败，且清理结果可查询

### Requirement: Hook failure circuit breaker

系统 MUST 为每个任务持久化最近一次 Hook 错误的脱敏 fingerprint 和连续失败次数。相同 fingerprint 连续失败达到 5 次时，系统 MUST 将当前 run 标记为 `hook_failure_circuit_open`、保持任务失败并停止自动启动 Agent；第 1 至第 4 次仅允许自动调度场景按策略重试，手工触发不得自动重试。fingerprint 变化或 Hook 成功完成时 MUST 重置连续计数，重复终态事件不得重复累计失败次数或产生副作用。

#### Scenario: Repeated Hook failure opens the circuit

- **WHEN** 同一任务的同一 Hook fingerprint 连续失败达到第五次
- **THEN** 当前 run 以 `hook_failure_circuit_open` 失败，任务保持失败，后续自动调度停止并生成要求人工介入的脱敏原因

#### Scenario: Hook failure remains retryable below the threshold

- **WHEN** 同一 fingerprint 连续失败次数为 1 至 4 次且 run 来自自动调度
- **THEN** 系统按 Hook 重试策略重新排队，不因普通 scheduler attempt 上限提前终止该 Hook 重试窗口

#### Scenario: Hook success or fingerprint change resets the counter

- **WHEN** Hook 成功完成或下一次失败产生不同 fingerprint
- **THEN** 系统将连续失败计数重置为零或一，并允许后续行为按当前策略重新评估

#### Scenario: Duplicate terminal event does not increment Hook failures

- **WHEN** 同一 run 的 Hook 失败终态事件被重复接收
- **THEN** 系统只保留一次计数、终态、通知和清理结果，后续事件记录为幂等重放

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

### Requirement: Scheduler observability

系统 MUST 提供可聚合的队列深度、派发延迟、claim 冲突、重试、stall、活动 run、reconciliation 修正和丢弃事件指标，并可通过 run 详情查看当前 attempt、下一次重试时间和失败原因。

#### Scenario: Queue backlog is visible

- **WHEN** 任务因容量、环境或退避时间无法立即派发
- **THEN** 指标和任务详情均显示队列原因、等待时长和下一次调度时间

### Requirement: Repair dispatch is bounded and restart-safe

Orchestrator MUST 在普通 queued task 派发前后，以 repair 请求的稳定幂等身份处理符合资格的 repair run，并在派发前重新验证来源失败、固化策略、审批、Hook 熔断、活动 run、容量与 workspace 条件。并发 worker、重复事件、claim 过期和重启 MUST 最多创建一个 repair run；临时错误只能释放 claim 并按固化退避重试，确定性拒绝或人工交接不得启动 Agent。

#### Scenario: Eligible repair is dispatched

- **WHEN** repair 请求具备可信诊断、已满足审批、未超过策略限额、未触发 Hook 熔断且 workspace 已安全准备
- **THEN** Orchestrator 原子绑定唯一 repair run、workspace 和任务投影后才启动 Agent，并记录稳定 start key

#### Scenario: Repair dispatch becomes ineligible

- **WHEN** claim 后发现来源证据漂移、审批撤回/过期、任务取消、已有活动 run、策略限额耗尽或 Hook 熔断打开
- **THEN** Orchestrator 不启动 Agent，将请求明确拒绝或交接人工，并恢复或保持符合来源结论的任务投影

#### Scenario: Worker restarts during repair dispatch

- **WHEN** worker 在 repair run 创建、workspace 绑定、请求派发标记或 Agent 启动之间重启
- **THEN** reconciliation 复用已存在的 repair run 与稳定 start key，修正可恢复绑定或安全终结请求，不产生重复进程

### Requirement: Repair terminal handling revalidates without rewriting source history

repair run 终态 MUST 驱动受影响门禁的重新执行、repair 请求结果、任务投影、审计、指标和 outbox 的幂等处理。来源 run 的终态与失败证据 MUST 保持不可变；终态重放不得重复执行门��、通知、清理、递增修复次数或创建后续 repair run。

#### Scenario: Repair terminal event is replayed

- **WHEN** Orchestrator 多次收到同一 repair run 的退出、超时或门禁终态事件
- **THEN** 系统保留唯一 repair 结果、唯一门禁重跑记录和唯一通知/审计副作用，并记录重放事实

#### Scenario: Repair gate fails again

- **WHEN** repair run 完成后受影响门禁仍失败
- **THEN** Orchestrator 仅在策略允许且存在新的可信失败证据时处理下一次 repair；否则停止自动化并交接人工
