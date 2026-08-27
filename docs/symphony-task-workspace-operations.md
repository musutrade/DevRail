# Symphony 任务工作区运维

## 检查

通过任务或运行详情 API 查看 `lifecycleStatus`、`cleanupStatus`、`relativeId`、版本摘要和 `diagnosticRef`。`relativeId` 不是绝对路径；完整路径只在受控 worker 日志中短暂使用并应脱敏。

## 清理失败

`cleanup_failed` 会记录错误摘要和下一次重试时间。调度器每轮最多处理 100 条到期记录，删除成功或目录已不存在均标记为 `cleaned`。目录被占用时不要复用路径，先停止占用进程，再通过 `POST /api/v1/workspaces/{id}/cleanup` 重试。

## 滚动部署与回滚

先应用 `20260906100000_add_task_workspaces.sql`，确认旧 run 仍可查询，再部署新 worker。回滚应用版本时保留迁移和 cleanup worker；关闭新 run 创建不会删除已存在的工作区记录或活动目录。

## 故障排查

1. 核对受控根目录存在且 worker 有读写权限。
2. 检查 workspace 的 `errorSummary` 和 `diagnosticRef`，不要复制原始命令输出。
3. 检查 `devrail_task_workspaces` 的 `next_cleanup_at` 和 `cleanup_attempts`，必要时人工释放文件占用后重试。
