## MODIFIED Requirements

### Requirement: Repair runs are policy-bound and distinct from retry and continuation

系统 SHALL 将 repair run 表示为来源失败之后具有独立 run、attempt、修复序号和稳定幂等身份的受控执行。普通 retry、传输恢复、continuation、follow-up task 与 repair run MUST 使用可区分的运行种类、谱系和限额。修复策略 MUST 默认关闭，并在来源任务的不可变快照中固化允许触发、最大修复次数、诊断大小、审批要求和人工交接阈值；来源 run 的状态、终态时间、changeset 与审计事实 MUST NOT 被改写。

Repair 门禁重跑 MUST 使用可续租、可过期且与唯一 owner/token 绑定的执行权。执行期间租约失效时，原执行者 MUST 停止提交结果并终止正在运行的门禁；接管者 MUST 只能领取已失效的执行权。任一 repair 门禁在同一时刻 MUST 至多有一个有效执行者。

#### Scenario: Eligible failure creates one repair request

- **WHEN** 可信失败证据、来源任务快照、数据范围、策略限额和活动 run 条件均满足
- **THEN** 系统创建或返回同一稳定幂等身份的 repair 请求，并在后续派发时最多创建一个关联 repair run

#### Scenario: Repair policy or Hook circuit blocks automation

- **WHEN** 策略关闭、修复次数达到上限、已有活动 run、来源或候选 run 处于 `hook_failure_circuit_open`，或预算/容量不允许自动执行
- **THEN** 系统不启动 Agent，保留来源失败结论并写入可查询的人工交接原因

#### Scenario: Duplicate failure is replayed

- **WHEN** 同一门禁、CI 或审查失败事件被重复投递或 worker 在处理期间重启
- **THEN** 系统返回原诊断、repair 请求和既有 run 结果，不增加修复次数、不创建第二个 run，也不重复通知

#### Scenario: Repair gate execution renews ownership

- **WHEN** repair 门禁仍在执行且当前 owner 在租约期限内持续续租
- **THEN** 系统保持该 owner 的唯一执行权，不允许过期回收器或其他 worker 接管同一门禁

#### Scenario: Repair gate loses ownership

- **WHEN** repair 门禁续租失败、租约过期或 owner 进程失去执行权
- **THEN** 原 owner 不得写入门禁结果或任务终态，正在执行的门禁被终止，已失效执行权可由一个新 owner 安全接管

#### Scenario: Stale repair gate completion is replayed

- **WHEN** 旧 owner 在失去租约后提交完成、失败或取消结果
- **THEN** 系统拒绝该结果且不覆盖新 owner 的状态、结果、通知、审计或任务投影
