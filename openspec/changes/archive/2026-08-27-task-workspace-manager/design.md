## Context

当前 `devrail_environments.workspace_root` 只提供受控根目录或共享执行目录，run 记录虽然有 `cwd` 和 cleanup 状态，但没有任务级 workspace 的持久化占用、来源提交和生命周期状态。Harness Supervisor 已具备工作目录边界、命令策略、网络关闭、审批和脱敏能力；本变更在这些边界之上增加 workspace 管理，不改变现有 app-server 所有权。

## Goals / Non-Goals

**Goals:**

- 以任务和 attempt 为粒度管理确定性、互斥且可重建的 workspace。
- 在 Agent 启动前完成目录和基础提交校验，在终态后可靠执行 hooks 与 cleanup。
- 将 workspace 状态、元数据和失败恢复接入现有 run、reconciliation、审计、事件、指标和权限模型。
- 让 Git worktree 与等价受控目录都遵守相同的安全策略和清理语义。

**Non-Goals:**

- 不在本变更中实现 continuation turn、质量门禁自动修复或外部 issue tracker。
- 不引入独立 workspace 微服务、容器编排或生产数据库连接。
- 不允许仓库 workflow 放宽平台权限、网络、命令白名单、审批、脱敏和受控根目录。

## Decisions

### 1. 使用持久化 workspace 记录作为占用事实

新增 workspace 表以任务、run、attempt、组织范围、受控路径摘要、基础提交、分支、生命周期状态和 cleanup 信息为核心字段，并用数据库唯一约束阻止活动路径重复占用。Repository 在 SQL 中完成组织/部门/所有者过滤和原子 claim/绑定。

相比只依赖文件系统锁，数据库事实可被多 worker reconciliation 观察、审计和恢复；文件系统锁仍可作为创建 worktree 时的短生命周期实现细节。

### 2. 稳定路径由 task/attempt 派生并做 canonical 校验

workspace 目录名由不可变 task ID、attempt 和短哈希组成，不接受用户直接提供的绝对路径。服务端先 canonicalize 受控根目录，再校验候选路径未越界、不是符号链接逃逸且未被其他活动记录占用。

相比完全随机目录，稳定命名便于重启恢复、清理重试和审计关联；随机部分只作为碰撞防护，不作为业务幂等键。

### 3. hooks 复用现有受限命令执行器

`WORKFLOW.md` 只声明受控 hook 名称和允许命令摘要。Workspace Manager 将其转换为现有质量门禁/命令白名单执行路径，继承网络、超时、资源、审批、工作目录和脱敏设置；不在 Handler 中执行 shell，也不把原始输出写入事件。

这样可避免出现第二套命令安全模型。若 hook 失败，服务根据 hook 类型决定阻止启动、保留终态或进入 cleanup 重试，并把结构化错误写入 run/workspace 诊断字段。

### 4. 清理采用幂等状态机而非立即删除记录

workspace 生命周期至少包含 `preparing`、`ready`、`running`、`cleanup_pending`、`cleanup_failed`、`cleaned` 和 `orphaned`。终态 run 保留 workspace 记录及元数据，文件清理成功后才释放路径；失败时由 reconciliation/worker 按退避重试，禁止复用失败路径。

保留记录可支持审计和失败诊断，也避免重复终态事件触发危险的重复删除。达到保留期的元数据归档/删除属于后续产物保留策略，不在本变更中静默处理。

### 5. API 只返回脱敏 workspace 投影

任务详情和运行详情增加 workspace 状态、生命周期时间、基础提交、版本摘要、清理状态和错误引用；完整绝对路径只在后端内部使用，前端显示受控根相对标识或脱敏摘要。所有查询继续使用现有权限和数据范围 actor。

## Risks / Trade-offs

- [文件系统或 Git worktree 清理失败] → 保留 `cleanup_failed` 记录、告警和下一次重试时间，禁止路径复用，并在运维手册提供人工恢复步骤。
- [并发 worker 在数据库绑定后同时创建目录] → 使用事务唯一约束、确定性锁顺序和创建后状态校验；失败时回滚绑定并清理未完成目录。
- [hook 运行时间增加调度延迟] → 每个 hook 使用独立超时、低基数耗时指标和可配置上限，失败原因可查询。
- [workspace 元数据包含仓库或路径信息] → 仅保存受控路径摘要和脱敏版本字段；日志、事件、推送不得包含完整路径、凭据或命令输出。
- [旧 run 没有 workspace 记录] → 迁移提供兼容的 `legacy`/受控 cwd 投影，reconciliation 不为历史终态 run 强行创建目录；新 run 必须走 workspace 管理器。

## Migration Plan

1. 增加 workspace 表、索引、状态约束和 run/workflow 关联字段，旧 run 保持可读。
2. 发布后端 Repository、Service 和 Supervisor 绑定逻辑，先以 feature flag 控制新任务使用独立 workspace。
3. 开启 workspace 创建、hooks 和 cleanup reconciliation，观察失败率、占用和清理耗时指标。
4. 验证 PostgreSQL 并发、进程重启、路径逃逸、hook 失败和清理失败演练后，再将新 run 默认切换为强制 workspace。
5. 回滚时关闭 feature flag，保留已创建 workspace 的 cleanup worker 和审计记录；不得删除迁移字段或强制清理仍被活动 run 使用的目录。

## Open Questions

暂无。Git worktree 与等价目录的具体选择可根据仓库 provider 能力在实现阶段通过现有环境策略确定，不改变本变更的外部契约。
