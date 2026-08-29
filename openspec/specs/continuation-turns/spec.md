# continuation-turns Specification

## Purpose

为终态运行后的受控追加执行建立独立、可审计且可恢复的业务契约，使用户上下文、质量门禁和审查反馈能够在同一 Codex thread 上形成新 turn，同时不混淆传输恢复、重试或后续任务。

## Requirements

### Requirement: Continuation has distinct and immutable lineage

系统 MUST 将 continuation 表示为来源 run 之后、同一 Codex thread 上的新 turn 和新的 child run。来源 run 的终态 MUST 保持不可变；child run MUST 暴露来源 run、来源 turn、continuation 序号和运行种类，使 continuation、同 attempt 传输恢复、任务 retry 与 follow-up task 可明确区分。

#### Scenario: Continuation child run is created

- **WHEN** 一个有效 continuation 请求被成功派发
- **THEN** 系统创建同 thread 的新 turn 与新 child run，保存完整谱系，并保持来源 run 的状态和终态时间不变

#### Scenario: Transport recovery is requested

- **WHEN** 活动 run 发生可恢复的网关断流或 app-server 连接中断
- **THEN** 系统按同 run、同 attempt 的传输恢复处理，不创建 continuation 请求或 continuation child run

#### Scenario: Follow-up task is proposed

- **WHEN** Agent 通过受控工具提出具有独立目标的后续任务
- **THEN** 系统按 follow-up task 契约创建新任务及依赖关系，不把该请求投影为 continuation

### Requirement: Continuation triggers are bounded and evidence-backed

系统 SHALL 只接受授权用户追加上下文、质量门禁失败和审查要求修改三类 continuation 触发。每个请求 MUST 绑定来源 run 与可验证触发证据，并校验来源 run 终态、任务范围、活动 run、链深、累计次数和策略限额；不满足条件时 MUST 返回安全且可诊断的拒绝结果，不改变来源 run 或启动 Agent。

#### Scenario: Authorized user adds context

- **WHEN** 有权用户为数据范围内已成功或失败的终态 run 提交非空、符合长度和内容策略的追加上下文
- **THEN** 系统创建用户触发的 continuation 请求，保存脱敏摘要和审计身份，并进入待派发状态

#### Scenario: Quality gate requests a continuation

- **WHEN** 受信任的质量门禁报告可关联来源 run 的失败结果且策略允许继续修复
- **THEN** 系统以门禁标识、结果摘要和证据引用创建至多一个对应 continuation 请求

#### Scenario: Review requests changes

- **WHEN** 受信任的审查事件要求修改且能关联来源 run 和当前变更版本
- **THEN** 系统以审查线程或事件的稳定身份创建至多一个对应 continuation 请求

#### Scenario: Continuation policy rejects the request

- **WHEN** 来源 run 非法、已有活动 run、任务已取消、证据过期，或链深、次数、输入大小超过固化策略
- **THEN** 系统不创建 child run，返回低敏感度拒绝原因，并记录一次可审计的策略结论

### Requirement: Continuation request processing is idempotent and restart-safe

每个 continuation 请求 MUST 具有在组织、任务、来源 run、触发类型和触发证据范围内稳定的幂等身份，并公开可查询的待处理、已领取、已派发、已完成、已取消或已拒绝结果。并发 worker、重复事件、进程重启和超时重放 MUST 最多创建一个 child run，且所有重复请求 MUST 返回原请求及其既有结果。

#### Scenario: Duplicate trigger is replayed

- **WHEN** 相同幂等身份的用户请求、门禁结果或审查事件被重复提交
- **THEN** 系统返回原 continuation 请求和原 child run 结果，不重复消耗限额、不创建第二个 run，也不重复通知

#### Scenario: Worker restarts after claiming

- **WHEN** worker 在领取 continuation 后、派发结果持久化前重启
- **THEN** reconciliation 根据持久化请求和 claim 恢复处理，复用已存在的 child run 或创建唯一 child run，不把请求永久留在已领取状态

#### Scenario: Pending continuation is cancelled

- **WHEN** 授权操作者在 Agent 启动前取消待处理或已领取的 continuation
- **THEN** 系统幂等标记请求已取消、释放 claim、阻止 Agent 启动，并保留来源 run 终态

#### Scenario: Cancellation races with dispatch

- **WHEN** 取消与 child run 派发并发发生
- **THEN** 系统产生唯一确定结果：要么取消成功且不启动 Agent，要么返回已派发冲突并由既有 run 的取消契约处理，不出现未关联的活动进程

### Requirement: Continuation completion is projected without rewriting history

child run 的进度和终态 MUST 更新 continuation 请求结果与当前任务投影，但 MUST NOT 改写来源 run、来源 turn、来源 changeset 或既有审计事件。continuation 失败后的 retry MUST 归属于 child run 的 attempt；再次 continuation MUST 创建下一序号请求并重新执行链深和限额校验。

#### Scenario: Continuation child run succeeds

- **WHEN** continuation child run 进入成功终态
- **THEN** 请求记录关联成功结果，任务投影反映最新成功执行，并保留来源 run 与所有中间状态历史

#### Scenario: Continuation child run fails retryably

- **WHEN** child run 以可重试错误失败且尚未达到 retry 限额
- **THEN** 系统按该 child run 的 retry 策略处理，不创建新的 continuation 序号

#### Scenario: Another continuation is requested

- **WHEN** 最新 child run 已终态且新的有效触发到达
- **THEN** 系统在同一 thread 上创建下一 continuation 序号，并再次应用活动 run、链深、次数和证据策略

### Requirement: Continuation access and emitted data are scoped and redacted

创建、查询、取消和查看 continuation 谱系的 API MUST 在 SQL 数据访问中强制组织、部门、所有者、项目和任务范围，并要求相应权限。输入、事件、审计、日志、指标标签、SSE 和 transactional outbox payload MUST 脱敏，且推送载荷只包含通知 ID、事件类型、脱敏摘要和受控深链接。

#### Scenario: Authorized user views continuation lineage

- **WHEN** 有权用户查看其范围内任务或 run 的 continuation 信息
- **THEN** 系统返回请求状态、触发类型、脱敏摘要、序号、来源与 child run 引用，不返回秘密、完整命令输出或受控绝对路径

#### Scenario: Cross-organization continuation access

- **WHEN** 调用方创建、查询或取消另一个组织的 continuation
- **THEN** 系统返回与资源不存在等价的安全结果，不泄露请求、run、证据或任务是否存在

#### Scenario: Continuation state changes

- **WHEN** continuation 被创建、派发、取消、拒绝或进入终态
- **THEN** 系统写入一次幂等审计与领域事件，并通过 transactional outbox 产生符合脱敏限制的站内通知事实
