## ADDED Requirements

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
