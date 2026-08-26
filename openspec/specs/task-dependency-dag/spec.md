# task-dependency-dag Specification

## Purpose

为 DevRail 提供组织范围内可验证、可追溯且可幂等处理的任务依赖图与 Agent 后续任务入口，确保调度顺序、终态传播和自动拆分不会绕过现有权限与安全边界。

## Requirements

### Requirement: Scoped acyclic task dependencies

系统 MUST 允许任务声明一个或多个前置任务，并 MUST 保证依赖两端属于同一组织。创建、替换或删除依赖 MUST 在单个数据库事务中完成范围校验、重复校验和环检测；同一依赖的幂等重放 MUST 返回原结果，而任何跨组织、自依赖或成环变更 MUST 不写入部分结果。

#### Scenario: A valid dependency is created

- **WHEN** 授权调用方为同一组织内的下游任务添加一个不会形成环的前置任务
- **THEN** 系统原子保存依赖及创建者、策略和来源，并可从上下游两个方向查询该关系

#### Scenario: A dependency would create a cycle

- **WHEN** 新增或替换依赖会使目标任务可沿现有依赖路径回到自身
- **THEN** 系统拒绝整次变更、返回可区分的依赖冲突，并保持原依赖图不变

#### Scenario: A dependency crosses organization scope

- **WHEN** 调用方尝试关联不同组织的任务，或任一任务不在其数据范围内
- **THEN** 系统不泄露范围外任务是否存在，不创建依赖，并返回符合权限边界的未找到或无权限结果

#### Scenario: The same dependency request is replayed

- **WHEN** 同一调用方使用相同幂等键和相同规范化请求重复创建依赖
- **THEN** 系统返回第一次成功结果且不新增第二条边；相同幂等键对应不同请求时返回冲突

### Requirement: Deterministic dependency blocking and propagation

每条依赖 MUST 固化前置任务失败、取消和超时后的动作，动作只能为 `wait`、`skip` 或 `fail`，缺省动作 MUST 为 `wait`。下游任务在全部前置任务成功前 MUST 不可派发；前置任务进入终态时，系统 MUST 按已固化策略幂等地保持等待、将下游任务标记为 `skipped` 或标记为 `failed`，并保存低基数阻塞原因和可读摘要。

#### Scenario: All prerequisites succeed

- **WHEN** 下游任务的全部前置任务均进入 `succeeded`，且其他派发条件也满足
- **THEN** 系统清除依赖阻塞原因，使该任务可以在后续原子 claim 中参与派发

#### Scenario: A prerequisite is still active

- **WHEN** 任一前置任务尚未成功或仍可恢复，且没有终态传播动作需要执行
- **THEN** 下游任务保持可查询的排队等待状态、显示阻塞前置任务，不会创建 run 或占用调度容量

#### Scenario: A terminal dependency policy skips the task

- **WHEN** 前置任务失败、取消或超时，且对应固化动作是 `skip`
- **THEN** 下游任务只进入一次 `skipped` 终态，记录前置任务和策略依据，且不再参与派发

#### Scenario: A terminal dependency event is replayed

- **WHEN** 同一前置任务终态被 reconciliation、进程退出或 webhook 重复观察
- **THEN** 系统不会重复传播状态、重复写通知或重复产生后续动作，重复处理具有可审计结果

### Requirement: Controlled Agent follow-up proposals

Agent MUST 只能通过与当前 run、task 和组织绑定的受控 API 提议后续任务。系统 MUST 在创建任务前校验封闭 schema、任务与 run 状态、调用方权限、组织/部门/所有者范围、仓库与环境范围、单次及累计配额和稳定幂等键；Agent MUST NOT 直接写数据库、指定更高权限主体或扩大网络、工具和审批能力。

#### Scenario: A valid follow-up proposal is accepted

- **WHEN** 活动或刚完成的受控 run 提交合法后续任务提议，且范围、权限、配额和幂等校验均通过
- **THEN** 系统创建带来源任务、来源 run、创建方式和请求摘要的新任务，并按请求建立合法依赖关系

#### Scenario: A follow-up proposal is replayed

- **WHEN** 同一来源 run 使用相同幂等键和相同规范化 payload 重试请求
- **THEN** 系统返回同一个已创建任务，不重复消耗配额、不创建重复依赖或重复事件

#### Scenario: A follow-up proposal exceeds authority or quota

- **WHEN** Agent 请求范围外资源、更高权限、未知字段、超限任务数量或与既有幂等键不一致的 payload
- **THEN** 系统拒绝请求、不创建任何任务或依赖，并记录不包含敏感 payload 的安全审计结果

### Requirement: Dependency traceability and user visibility

任务详情 API MUST 返回当前调用方可见的前置任务、下游任务、各依赖状态、固化传播策略、阻塞原因和任务创建来源。依赖变更、终态传播和后续任务创建 MUST 产生脱敏事件、Scheduler/System Actor 审计、SSE 更新和低基数指标；任何响应、日志或推送 MUST NOT 包含密钥、完整请求头或未经脱敏的 Agent payload。

#### Scenario: User inspects a blocked task

- **WHEN** 有权用户打开因依赖未满足而等待的任务详情
- **THEN** API 和界面显示可见前置任务、当前状态、阻塞原因及策略，不暴露范围外任务或敏感执行内容

#### Scenario: Dependency state changes

- **WHEN** 前置任务成功或终态传播改变下游任务的可派发状态
- **THEN** 系统写入一次可关联 trace 的事件和审计记录，并通过 SSE 使任务详情更新为一致状态
