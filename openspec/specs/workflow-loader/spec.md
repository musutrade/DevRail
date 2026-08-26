# Workflow Loader Specification

## Purpose

为 DevRail 仓库定义安全、严格、可热加载且可复现的 WORKFLOW.md 契约，使每个 Agent run 使用经过验证并持久化的确定版本。

## Requirements

### Requirement: Repository workflow contract

系统 MUST 支持从受控仓库根目录加载 `WORKFLOW.md`。文件 MUST 由 YAML front matter 和 Markdown 提示正文组成，并支持声明版本、执行模式、工具策略、质量门禁、超时、重试、hooks 与通知策略；文件缺失时 MUST 使用带明确版本的安全默认 workflow。

#### Scenario: Valid repository workflow is loaded

- **WHEN** 受控仓库根目录存在格式和字段均合法的 `WORKFLOW.md`
- **THEN** loader 返回经过规范化的强类型配置、提示正文、来源和内容摘要

#### Scenario: Workflow file is absent

- **WHEN** 受控仓库根目录没有 `WORKFLOW.md`
- **THEN** loader 使用版本化的后端安全默认值，并将来源明确标记为默认配置

#### Scenario: Workflow path escapes repository

- **WHEN** `WORKFLOW.md` 是指向受控仓库之外的链接或解析路径越过仓库根目录
- **THEN** loader 拒绝加载并记录安全校验失败，不读取工作区外内容

### Requirement: Strict schema and template validation

loader MUST 拒绝未知字段、未知模板变量、未知过滤器、缺失必填字段、非法枚举、超出大小限制和不合法的 front matter。校验失败 MUST 返回带文件位置的安全诊断，不得将未知值静默替换为空，也不得部分采用无效版本。

#### Scenario: Unknown template input

- **WHEN** 提示正文引用未列入允许上下文的变量或未注册过滤器
- **THEN** 整份 workflow 校验失败，诊断标明安全的位置与类别，且不会创建使用该版本的任务快照

#### Scenario: Unknown configuration field

- **WHEN** YAML front matter 包含 schema 未定义的字段或非法枚举
- **THEN** 整份 workflow 校验失败，不忽略该字段也不回退为部分配置

#### Scenario: Secret-like diagnostic value

- **WHEN** 无效配置或模板上下文包含 token、连接串或其他敏感内容
- **THEN** 告警、审计和 API 诊断只保存脱敏摘要，不回显原始敏感值或完整正文

### Requirement: Platform safety bounds override repository policy

仓库 workflow MUST 只能在平台允许范围内选择或收紧能力。它 MUST NOT 绕过组织权限、审批、事件与命令脱敏、受控根目录、默认网络关闭、危险命令审批或平台资源上限。

#### Scenario: Workflow requests forbidden capability

- **WHEN** workflow 请求开放网络、工作区外路径、跳过审批或超过平台上限，且环境策略未明确授权
- **THEN** 该 workflow 被拒绝，或按字段契约收紧到平台上限并在快照中明确记录有效值；系统绝不扩大权限

#### Scenario: Workflow is stricter than platform policy

- **WHEN** workflow 禁用一个平台允许的工具或设置更短超时
- **THEN** 有效配置采用更严格的仓库值，并将规范化结果写入快照

### Requirement: Immutable workflow snapshots

任务进入 `queued` 时 MUST 持久化实际采用的 workflow 来源、声明版本、规范化内容、提示正文和内容摘要；由该任务创建的每个 run MUST 复制或稳定引用同一不可变快照。文件后续变化 MUST 不改变已排队任务或活动 run。

#### Scenario: Run starts from a queued task

- **WHEN** Orchestrator 为含 workflow 快照的排队任务创建 run
- **THEN** run 记录与任务快照一致的来源、版本和摘要，Harness 只使用该快照渲染执行输入

#### Scenario: File changes during an active run

- **WHEN** 活动 run 对应仓库的 `WORKFLOW.md` 被更新或删除
- **THEN** 当前 run 和已排队任务继续使用原快照，事件、重试与 changeset 仍关联原摘要

### Requirement: Safe dynamic reload

worker MUST 检测受控仓库 workflow 的内容变化并对候选版本执行完整校验。合法版本只供随后进入 `queued` 的任务建立快照；非法版本 MUST 保留上一次有效配置，并产生去重的告警、指标和 System Actor 审计事件。

#### Scenario: Valid workflow is reloaded

- **WHEN** 文件内容原子更新为一个不同摘要的合法版本
- **THEN** loader 原子发布该版本供新任务快照使用，不中断当前 run，也不改变既有任务快照

#### Scenario: Invalid workflow replaces valid version

- **WHEN** 文件变化后的候选版本无法通过严格校验
- **THEN** worker 继续提供上一次有效版本，记录一次按仓库和候选摘要去重的失败证据，并在后续合法变化时自动恢复

#### Scenario: Worker restarts

- **WHEN** worker 在 workflow 文件变化前后重启
- **THEN** 它重新校验磁盘候选版本和持久化身份，以确定性方式恢复当前有效版本，不依赖丢失的纯内存通知
