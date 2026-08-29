# controlled-repair-runs Specification

## Purpose

为质量门禁、CI 与审查失败提供受策略约束、可审计且不会扩张 Agent 权限的修复 run 契约，使失败证据能够安全地转化为有限次重新验证或明确的人工交接。

## Requirements

### Requirement: Repair diagnosis is scoped, redacted, and immutable

系统 MUST 在创建 repair run 前生成并持久化来源失败的诊断快照。快照 MUST 仅包含受影响门禁或可信事件身份、结构化低敏感度错误、受限日志引用、changeset 摘要、任务/workflow/environment 快照和来源 run 谱系；MUST NOT 包含凭据、Cookie、token、私钥、数据库连接串、完整请求头、完整命令输出或受控绝对路径。诊断快照 MUST 在组织、部门、所有者、项目和任务范围内查询，并在后续 repair run 中保持不可变。

#### Scenario: Trusted failure produces a repair diagnosis

- **WHEN** 可信质量门禁、已验证 CI 回调或已归一化的审查要求修改关联到范围内终态 run
- **THEN** 系统保存脱敏诊断快照和稳定证据身份，且不会向浏览器、事件、日志或通知暴露原始敏感内容

#### Scenario: Failure evidence is missing or stale

- **WHEN** 来源 run、门禁结果、CI 事件或审查证据无法验证、已过期或与当前 changeset 不匹配
- **THEN** 系统不创建 repair run，保存低敏感度拒绝原因并将处理交给人工，而不猜测或补全缺失输入

### Requirement: Repair runs are policy-bound and distinct from retry and continuation

系统 SHALL 将 repair run 表示为来源失败之后具有独立 run、attempt、修复序号和稳定幂等身份的受控执行。普通 retry、传输恢复、continuation、follow-up task 与 repair run MUST 使用可区分的运行种类、谱系和限额。修复策略 MUST 默认关闭，并在来源任务的不可变快照中固化允许触发、最大修复次数、诊断大小、审批要求和人工交接阈值；来源 run 的状态、终态时间、changeset 与审计事实 MUST NOT 被改写。

#### Scenario: Eligible failure creates one repair request

- **WHEN** 可信失败证据、来源任务快照、数据范围、策略限额和活动 run 条件均满足
- **THEN** 系统创建或返回同一稳定幂等身份的 repair 请求，并在后续派发时最多创建一个关联 repair run

#### Scenario: Repair policy or Hook circuit blocks automation

- **WHEN** 策略关闭、修复次数达到上限、已有活动 run、来源或候选 run 处于 `hook_failure_circuit_open`，或预算/容量不允许自动执行
- **THEN** 系统不启动 Agent，保留来源失败结论并写入可查询的人工交接原因

#### Scenario: Duplicate failure is replayed

- **WHEN** 同一门禁、CI 或审查失败事件被重复投递或 worker 在处理期间重启
- **THEN** 系统返回原诊断、repair 请求和既有 run 结果，不增加修复次数、不创建第二个 run，也不重复通知

### Requirement: Repair actions preserve approval and safety boundaries

repair run MUST 继承来源任务的数据范围、受控 workspace、网络、命令、资源和凭据策略。系统 MAY 为低风险格式化或未使用导入问题生成建议；逻辑修改、依赖升级、远端写入、权限/安全策略变更和任何未被策略明确允许的操作 MUST 在执行前要求对应审批，且不得自动应用。所有修复输入、审批、拒绝、取消和终态 MUST 写入幂等审计、领域事件和 transactional outbox。

#### Scenario: Low-risk repair is suggested or executed by policy

- **WHEN** 诊断匹配策略明确允许的低风险修复类别，且任务快照允许自动处理
- **THEN** 系统仅在固化权限与策略范围内创建 repair run 或可审查建议，并记录采用的策略与证据摘要

#### Scenario: High-risk repair needs approval

- **WHEN** repair 可能修改逻辑、依赖、远端状态或安全策略，或不属于已允许的低风险类别
- **THEN** 系统在 Agent 执行前创建可撤回、可过期且不可伪造的审批事实；未批准、被拒绝或已过期时不启动 Agent

### Requirement: Repair outcomes are revalidated and handed off to humans

每个 repair run MUST 重新执行受影响门禁，并将原始失败、诊断、修复 changeset、门禁结果和最终状态双向关联。成功修复不得删除或覆盖原始失败证据；失败、取消、策略拒绝、容量耗尽或达到修复上限时，系统 MUST 停止自动创建新 repair run，产生受权限保护的人工处理项、脱敏通知和恢复建议。

#### Scenario: Repair passes affected gates

- **WHEN** repair run 完成且所有受影响门禁均以当前修复 changeset 成功结束
- **THEN** 系统记录 repair 成功和完整谱系，保留来源失败事实，并使授权用户可从任务与 run 详情追溯两者

#### Scenario: Repair cannot proceed automatically

- **WHEN** repair run 失败、取消、门禁再次失败或达到策略次数/成本上限
- **THEN** 系统不创建新的自动 repair run，保存最终原因、下一步人工建议和脱敏通知，并保持所有来源与修复审计不可变

#### Scenario: Unauthorized user views repair history

- **WHEN** 调用方查询范围外 repair 请求、诊断、审批或 run 谱系
- **THEN** 系统返回与资源不存在等价的安全结果，不泄露失败类别、修复内容、证据、路径或存在性
