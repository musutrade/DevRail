## Purpose

为 DevRail 提供按任务隔离、可复现且可审计的 Agent 工作区生命周期，使每次运行都能在受控路径中安全创建、使用、清理和重新建立等价执行环境。

## ADDED Requirements

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
