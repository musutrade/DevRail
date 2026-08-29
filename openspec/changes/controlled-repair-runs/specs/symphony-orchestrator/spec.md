## ADDED Requirements

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

repair run 终态 MUST 驱动受影响门禁的重新执行、repair 请求结果、任务投影、审计、指标和 outbox 的幂等处理。来源 run 的终态与失败证据 MUST 保持不可变；终态重放不得重复执行门禁、通知、清理、递增修复次数或创建后续 repair run。

#### Scenario: Repair terminal event is replayed

- **WHEN** Orchestrator 多次收到同一 repair run 的退出、超时或门禁终态事件
- **THEN** 系统保留唯一 repair 结果、唯一门禁重跑记录和唯一通知/审计副作用，并记录重放事实

#### Scenario: Repair gate fails again

- **WHEN** repair run 完成后受影响门禁仍失败
- **THEN** Orchestrator 仅在策略允许且存在新的可信失败证据时处理下一次 repair；否则停止自动化并交接人工
