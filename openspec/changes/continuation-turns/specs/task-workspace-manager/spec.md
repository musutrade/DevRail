## ADDED Requirements

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
