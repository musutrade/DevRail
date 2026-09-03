## Purpose

为审计、门禁和验收建立可提交、可复现、可追踪的证据契约，使独立审阅者能够从干净副本重建结论并区分历史记录、当前状态和未完成工作。

## ADDED Requirements

### Requirement: Security evidence is versioned and reproducible

每份安全审计、复核或验收证据 MUST 记录 UTC 执行时间、提交 SHA、变更范围、完整命令、结果摘要和失败详情。证据 MUST 位于版本控制目录或具有永久定位能力的 CI artifact；被 gitignore 排除的本机报告不得作为唯一依据。

#### Scenario: Auditor reviews a committed report

- **WHEN** 审阅者从干净副本打开审计或复核报告
- **THEN** 报告中的关键结论可通过同仓库文件、命令和稳定 artifact 定位，不依赖作者个人工作区

#### Scenario: Evidence artifact is unavailable

- **WHEN** 文档引用的证据文件不存在、被 gitignore 排除或无法从链接定位
- **THEN** 证据检查失败，相关“已通过”声明不得用于关闭审计条目

### Requirement: Remediation status is traceable to one change

每个审计条目 MUST 关联明确的 OpenSpec change、代码或配置范围、测试、门禁和关闭证据。原始审计报告 MUST 保持时间点记录，修复状态 MUST 在复核、ADR、OpenSpec 和 PR 中更新，不得覆盖原始结论。

#### Scenario: Remediation closes an audit item

- **WHEN** 一个实现变更完成并通过对应测试与 required checks
- **THEN** 追踪记录包含审计编号、变更、证据和关闭日期，且原始审计文本保持不变

#### Scenario: Documentation claims conflict

- **WHEN** 状态文档、HANDOFF、验收矩阵或审计复核对同一能力给出相互矛盾的“已通过”声明
- **THEN** 文档一致性门禁失败，不能将该能力标记为已验收

### Requirement: Planning artifacts do not claim implementation

OpenSpec proposal、design、spec 和 tasks MUST 区分目标契约与已完成实现。规划工件不得把本地演练、静态检查或单次 CI 通过写成产品验收完成。

#### Scenario: Proposal is reviewed before implementation

- **WHEN** 变更尚未实施时审阅规划工件
- **THEN** 工件只描述问题、决策、需求和验收条件，不声称业务代码已经完成

#### Scenario: Implementation evidence is incomplete

- **WHEN** 代码已合并但缺少要求的测试、artifact 或生产/浏览器证据
- **THEN** 追踪状态保持未验证，不能更新 ADR 为 Accepted 或把 MVP 验收项标为通过
