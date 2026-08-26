## Purpose

为 DevRail 提供可靠、可恢复且可审计的 Symphony 调度控制循环，使任务在多 worker、进程重启和传输异常下仍能保持单次执行语义与最终一致状态。

## ADDED Requirements

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

每一轮调度 MUST 先对账任务状态、run 状态、claim、Agent 进程和 workspace，再进行新的任务派发。发现不一致时 MUST 按确定性规则恢复、重新排队或标记失败，并写入原因和审计事件。

#### Scenario: Database says active but process is gone

- **WHEN** run 在数据库中仍为活动状态，但对应 Agent 进程已退出
- **THEN** 系统读取退出摘要，按重试策略恢复或标记明确失败，不留下无限期活动 run

#### Scenario: Task is cancelled while dispatch is pending

- **WHEN** 任务在 claim 后、Agent 启动前被取消
- **THEN** 调度器释放 claim，不启动 Agent，并将任务和 run 更新为可查询的取消状态

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

### Requirement: System actor and auditable terminal handling

自动调度和 reconciliation MUST 使用独立的 Scheduler/System Actor，并在状态历史中记录触发来源、策略版本和 trace。终态处理 MUST 幂等，重复退出、超时或 webhook 事件不得重复清理、通知或创建后续动作。

#### Scenario: Scheduler updates a task

- **WHEN** worker 自动领取、重试或修正任务状态
- **THEN** 审计记录显示 Scheduler/System Actor、触发原因和关联 trace，而不是伪造普通用户会话

#### Scenario: Duplicate terminal event

- **WHEN** 同一 run 的终态事件被接收两次或更多次
- **THEN** 系统只保留一个终态结果、一组幂等通知和一次清理结果，后续事件记录为重复事件

### Requirement: Scheduler observability

系统 MUST 提供可聚合的队列深度、派发延迟、claim 冲突、重试、stall、活动 run、reconciliation 修正和丢弃事件指标，并可通过 run 详情查看当前 attempt、下一次重试时间和失败原因。

#### Scenario: Queue backlog is visible

- **WHEN** 任务因容量、环境或退避时间无法立即派发
- **THEN** 指标和任务详情均显示队列原因、等待时长和下一次调度时间
