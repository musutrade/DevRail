## Why

当前任务运行只绑定到受控根目录下的已有路径，多个任务无法获得确定性的独立工作区，也缺少统一的生命周期钩子和清理结果。实现任务级 workspace/worktree 后，Agent 执行、变更集、重试和审查才能在隔离且可复现的目录中运行，并让调度器能够可靠处理清理失败。

## What Changes

- 为每个任务和执行尝试创建确定性、组织范围隔离的 workspace 元数据和受控路径。
- 支持基于 Git worktree 或等价目录的任务级隔离，校验仓库、基础提交、分支和环境策略。
- 增加 `before_run`、`after_run`、`on_failure`、`cleanup` 生命周期 hooks，并复用现有命令、网络、审批、超时和脱敏边界。
- 将 workspace 创建、绑定、使用、清理和失败状态纳入 run reconciliation、审计、事件、指标和 API 查询。
- 终态运行必须幂等清理；清理失败保留诊断信息并支持后续重试，不删除变更集和审计证据。
- 保存 workflow 版本、基础提交、环境版本和工具版本，使 workspace 可以按同一快照重建。

## Capabilities

### New Capabilities

- `task-workspace-manager`: 任务级隔离 workspace/worktree 的创建、生命周期 hooks、可复现元数据和清理恢复。

### Modified Capabilities

- `symphony-orchestrator`: 调度和 reconciliation 使用任务级 workspace，并在 run 启动前完成绑定、终态后完成清理。

## Impact

- 后端新增 workspace 数据模型、迁移、Repository、Service、Harness Supervisor 集成、reconciliation 和权限审计。
- 任务/run API、OpenAPI schema、Angular 任务详情和运行详情需要展示 workspace 状态、路径摘要、基础提交和清理错误，但不暴露受控目录外路径或凭据。
- `WORKFLOW.md` hooks 配置需要严格解析并与平台安全策略求交集。
- 增加 PostgreSQL、受控文件系统、Git worktree、故障清理和并发占用测试，并同步运行手册、需求矩阵和实现状态。
