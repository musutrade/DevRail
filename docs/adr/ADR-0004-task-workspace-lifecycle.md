# ADR-0004：任务执行工作区生命周期

## 状态

已接受

## 决策

每个任务执行尝试使用由 `task_id` 和 `attempt` 派生的确定性工作区标识。工作区记录保存在 `devrail_task_workspaces`，作为跨进程对账的占用事实；路径只允许位于 `DEVRAIL_RUN_WORKSPACE_ROOT` 受控根目录，API 仅返回相对标识和脱敏诊断。

工作区状态按 `preparing → ready → running → cleanup_pending → cleaned` 流转，清理失败进入 `cleanup_failed` 并由调度器按退避重试。终态记录和 changeset/audit 证据保留，重复终态和清理操作必须幂等。

历史 run 不回填工作区。迁移采用可空 `run_id` 和新增表，不改变既有 run 查询；回滚时关闭新工作区创建并继续运行清理对账，禁止删除仍被活动 run 使用的目录。

## 安全边界

- 所有 Repository 查询包含组织、部门和所有者数据范围。
- 工作区候选路径先 canonicalize 受控根，拒绝 `..`、绝对路径和符号链接越界。
- 凭据只允许运行时注入，不写入 workspace 元数据、事件、日志或通知。
