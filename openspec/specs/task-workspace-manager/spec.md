# task-workspace-manager Specification

## Purpose

为 DevRail 提供按任务隔离、可复现且可审计的 Agent 工作区生命周期，使每次运行都能在受控路径中安全创建、使用、清理和重新建立等价执行环境。

## Requirements

### Requirement: Deterministic isolated workspace

系统 MUST 为每个任务执行尝试分配稳定的 workspace 标识，并将实际路径限制在配置的受控根目录内。不同任务的活动 workspace MUST 互不占用；默认实现 MUST 使用独立 Git worktree 或等价的隔离目录，不能把多个活动任务绑定到同一路径。

#### Scenario: Workspace is created for an attempt

- **WHEN** 调度器准备启动一个已 claim 的任务执行尝试
- **THEN** 系统在受控根目录下创建与任务和 attempt 关联的独立 workspace，记录路径摘要、状态和创建时间，并在 Agent 启动前完成绑定

#### Scenario: Workspace path escapes the controlled root

- **WHEN** 任务、仓库或环境配置解析出的 workspace 路径位于受控根目录之外，或经符号链接解析后越界
- **THEN** 系统拒绝创建或绑定 workspace，不启动 Agent，并返回不泄露实际文件系统结构的安全错误

#### Scenario: Two attempts contend for one workspace

- **WHEN** 并发 worker 为不同任务或同一任务的不同 attempt 请求相同 workspace
- **THEN** 只有一个请求获得绑定，其余请求得到可分类冲突，不能覆盖、复用或删除已绑定的活动 workspace

### Requirement: Reproducible workspace metadata

每个 workspace MUST 保存任务快照、workflow 版本和摘要、仓库身份、基础提交、目标分支、环境版本、工具版本和创建方式等可重建元数据。重新创建 workspace 时 MUST 使用同一不可变 run 输入，不能从运行中的可变文件推导权限或凭据。

#### Scenario: Workspace can be recreated from a run snapshot

- **WHEN** 一个可恢复的 run 需要重新建立 workspace
- **THEN** 系统根据持久化快照重建等价的仓库状态和环境元数据，并保留原 workspace 的历史与失败原因

#### Scenario: Credentials are used during setup

- **WHEN** 创建或更新 workspace 需要访问仓库或环境凭据
- **THEN** 凭据只在受控运行时注入，不写入 workspace 文件、数据库事件、日志、差异或通知载荷

### Requirement: Continuation workspace evidence handoff

来源 run 进入终态时，系统 MUST 在清理其 workspace 前持久化足以重建 continuation 输入的不可变证据，包括任务与 workflow 快照、仓库身份、基础提交、受控目标分支或 changeset 引用、变更摘要和工具环境版本。证据 MUST 绑定来源 run 和数据范围，不得包含凭据、完整命令输出或受控绝对路径；缺少必要证据时 MUST 明确阻止 continuation 派发。

#### Scenario: Terminal workspace is ready for cleanup

- **WHEN** 来源 run 已生成可能用于后续 continuation 的受控变更
- **THEN** 系统先持久化并校验不可变 handoff 证据，再允许终态 hook 和 cleanup 释放 workspace

#### Scenario: Required handoff evidence is missing

- **WHEN** continuation 准备阶段无法验证来源快照、基础提交、分支或 changeset 引用
- **THEN** 系统不创建或启动 child run，保留可查询的脱敏失败原因，且不从残留 workspace 猜测执行输入

### Requirement: Continuation uses a newly reconstructed workspace

每个 continuation child run MUST 从来源 run 的不可变快照和受控 handoff 证据建立新的隔离 workspace，并使用独立的 workspace 身份和生命周期。系统 MUST NOT 复用已清理路径、`cleanup_failed` 路径或其他活动 run 的 workspace；重建结果 MUST 校验仓库身份、基础提交、变更证据和受控根目录边界后才能绑定 Agent。

#### Scenario: Source workspace was cleaned successfully

- **WHEN** continuation 在来源 workspace 已完成清理后派发
- **THEN** 系统从持久化证据建立新路径和新 workspace 记录，恢复等价受控变更，而不依赖旧目录存在

#### Scenario: Source workspace cleanup failed

- **WHEN** 来源 workspace 状态为 `cleanup_failed` 或仍被占用
- **THEN** 系统不复用、覆盖或删除该路径，而是在独立路径重建 continuation workspace，并保留原清理诊断

#### Scenario: Reconstructed evidence does not match

- **WHEN** 检出的仓库、基础提交、分支或 changeset 摘要与持久化证据不一致
- **THEN** 系统拒绝绑定和启动 Agent，执行新 workspace 的幂等清理，并记录不含敏感内容的完整性错误

#### Scenario: Source and child cleanup overlap

- **WHEN** 来源 workspace 清理重试与 continuation child workspace 创建或清理并发发生
- **THEN** 两者按各自稳定 run/workspace 身份独立处理，任何一方都不能删除、占用或改变另一方的路径与状态

#### Scenario: Continuation is cancelled before Agent start

- **WHEN** continuation workspace 已创建但请求在 Agent 启动前成功取消
- **THEN** 系统不启动 Agent，按 child workspace 幂等键执行终态 hook 与清理，并保留来源 handoff 证据供审计

### Requirement: Policy-bound workspace lifecycle hooks

系统 MUST 支持 `before_run`、`after_run`、`on_failure` 和 `cleanup` hooks，并对每个 hook 应用与 Agent 相同的命令白名单、网络策略、工作区边界、超时、资源上限、脱敏和审批规则。未知 hook、未知命令或越权配置 MUST 在运行前拒绝。

#### Scenario: Before-run hook fails

- **WHEN** `before_run` hook 返回非零退出码、超时或违反策略
- **THEN** 系统不启动 Agent，保存脱敏失败原因和 hook 结果，并进入可查询的失败或等待处理状态

#### Scenario: Terminal hooks run in order

- **WHEN** run 进入成功、失败、取消或中断终态
- **THEN** 系统按策略执行对应的 `after_run` 或 `on_failure`，随后执行一次 `cleanup`，每一步都有幂等结果和可关联审计事件

### Requirement: Auditable cleanup and recovery

终态 workspace MUST 执行清理并将状态记录为 `pending`、`completed` 或 `failed`。清理失败 MUST 保留 workspace 元数据、变更集和审计证据，记录脱敏错误和下一次重试时间，并由 reconciliation 或专用 worker 重试；系统 MUST NOT 静默复用或删除清理失败的 workspace。

#### Scenario: Cleanup succeeds

- **WHEN** 终态 workspace 的文件、worktree 和临时资源均已安全移除
- **THEN** 系统将 cleanup 状态更新为 `completed`，记录清理时间，并释放 workspace 占用

#### Scenario: Cleanup fails

- **WHEN** workspace 被占用、文件系统暂时不可用或 cleanup hook 失败
- **THEN** 系统将 cleanup 状态更新为 `failed`，保留可诊断摘要和重试信息，触发低基数告警，并阻止该路径被新的活动任务占用

#### Scenario: Cleanup is replayed after restart

- **WHEN** worker 在 cleanup 完成前重启，或重复收到同一 run 的终态事件
- **THEN** 系统根据稳定 workspace/run 幂等键恢复或重试同一清理操作，不重复执行危险删除，也不产生重复通知或审计事实

### Requirement: Scoped workspace visibility

workspace 查询、诊断和下载接口 MUST 按组织、部门、所有者、项目和任务数据范围过滤，并只返回受控路径的脱敏摘要、状态、基础提交和错误引用。响应、日志、事件和推送 MUST NOT 暴露绝对路径、凭据、完整命令输出或其他组织的 workspace 是否存在。

#### Scenario: Authorized user views workspace status

- **WHEN** 有权用户查看其数据范围内任务的 workspace
- **THEN** API 返回状态、生命周期时间、基础提交、workflow/环境版本和可诊断错误引用，但不返回完整受控路径或敏感内容

#### Scenario: Unauthorized user requests workspace details

- **WHEN** 调用方请求范围外任务或 workspace 的 ID、详情或产物
- **THEN** 系统返回与资源不存在等价的安全结果，不泄露路径、状态、大小、错误或时间信息

### Requirement: Repair runs use isolated evidence-backed workspaces

每个 repair run MUST 从来源任务快照、来源失败诊断、受控 changeset 和可验证仓库/环境身份创建新的隔离 workspace。系统 MUST NOT 复用来源 run、continuation、retry 或其他活动 repair run 的路径；诊断和 workspace 元数据 MUST 仅保存脱敏摘要与受限引用，不得落盘凭据、完整命令输出、绝对路径或完整失败正文。

#### Scenario: Repair workspace is reconstructed

- **WHEN** 符合资格的 repair 请求准备派发
- **THEN** Workspace Manager 根据不可变快照和受控证据创建独立 workspace，校验根目录、仓库身份、基础提交与 changeset 后再允许 Agent 启动

#### Scenario: Repair workspace evidence is invalid

- **WHEN** 来源诊断、仓库、基础提交、changeset 或环境快照不可验证或相互不匹配
- **THEN** 系统不绑定或启动 repair Agent，清理新建的临时资源，并将请求交接人工且保留脱敏诊断

#### Scenario: Repair is cancelled or restarted before start

- **WHEN** repair 在 Agent 启动前被取消，或 worker 在 workspace 准备期间重启
- **THEN** 系统按 repair run/workspace 稳定身份执行幂等清理或恢复，且不影响来源或其他 run 的 workspace
