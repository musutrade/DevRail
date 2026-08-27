## 1. 数据模型与安全边界

- [x] 1.1 执行 `cargo flow scope`，读取 workspace、Repository、Service、Handler、Angular 模板和 `review_context.json`，记录本 change 的 backend、frontend、workflow 范围并验证范围产物可查询
- [x] 1.2 新增 workspace 数据库迁移，包含组织/部门/所有者、task/run/attempt、受控路径摘要、基础提交、分支、workflow/环境/工具版本、生命周期和 cleanup 字段、唯一约束与索引；使用一次性 PostgreSQL 正向迁移验证
- [x] 1.3 为历史 run 提供兼容投影和滚动部署/回滚说明，验证旧 run 可查询且不会被强制创建或删除 workspace
- [x] 1.4 增加 workspace 领域模型、状态枚举、请求/响应 DTO 和权限码/种子，验证序列化、非法状态、组织范围和权限矩阵

## 2. Workspace Manager 核心服务

- [x] 2.1 按模板实现 workspace Repository 的创建、绑定、状态更新、查询和 cleanup 重试，所有 SQL 强制组织/部门/所有者范围；通过同组织、跨组织、重复占用和并发绑定 PostgreSQL 测试
- [x] 2.2 实现确定性 task/attempt workspace 标识、canonical 路径校验、符号链接越界拒绝和受控根目录约束；通过路径逃逸、碰撞和目录权限测试
- [x] 2.3 实现 Git worktree 或等价隔离目录创建、基础提交/分支校验和凭据运行时注入；验证凭据不写入文件、事件、日志或通知
- [x] 2.4 实现 workspace 生命周期状态机和幂等重建，保存 workflow、环境、工具版本与快照摘要；通过重启恢复和同快照重建测试
- [x] 2.5 实现 `before_run`、`after_run`、`on_failure`、`cleanup` hooks，复用命令白名单、网络、审批、超时、资源和脱敏策略；通过未知 hook、越权命令、超时、非零退出和执行顺序测试

## 3. Supervisor 与调度集成

- [x] 3.1 在 Harness Supervisor 启动前完成 workspace 创建/绑定和 `before_run`，失败时不启动 Agent 并保存脱敏诊断；通过受控 app-server 测试
- [x] 3.2 在 run 成功、失败、取消、中断和进程退出路径接入终态 hooks 与 cleanup，保证 `run_id + workspace_id` 幂等；通过重复终态和取消竞态测试
- [x] 3.3 扩展 reconciliation 对账 workspace 占用、进程、run 和 cleanup 状态，支持 cleanup failed 退避重试和 orphaned 恢复；通过 worker 重启、文件占用和暂时不可用测试
- [x] 3.4 增加 workspace 创建、占用、hook、清理、重建和失败原因的低基数指标、事件、Scheduler/System Actor 审计和 outbox；验证 payload 不含绝对路径、凭据或完整命令输出

## 4. API、OpenAPI 与前端

- [x] 4.1 按 Handler/Service 模板增加任务和 run workspace 查询、重建、cleanup 状态与诊断接口，统一映射未找到、无权限、状态冲突、路径非法和暂时不可用错误；通过路由集成测试
- [x] 4.2 扩展 Rust DTO 与 `utoipa` schema，重新生成 `docs/openapi.json` 和 Angular client；通过 OpenAPI snapshot 与 Rust/TypeScript 字段一致性检查
- [x] 4.3 在任务详情和运行详情 store/signal 加载 workspace 状态、基础提交、版本摘要、cleanup 和诊断引用，处理 SSE 刷新、加载、空态、权限裁剪和冲突；通过 Vitest
- [x] 4.4 在 Angular 页面展示 workspace 生命周期、相对标识、重建/清理操作和中文错误提示，禁止显示完整绝对路径；通过桌面与移动视口组件测试和可访问性检查

## 5. 全面测试与验收

- [x] 5.1 执行真实 PostgreSQL 集成测试，覆盖范围隔离、并发占用、路径校验、worktree 创建、生命周期转换、清理重试和历史 run 兼容
- [x] 5.2 执行 Harness 受控假 app-server 测试，覆盖 before-run 阻断、终态 hooks、cleanup 幂等、EOF/重启恢复、敏感字段脱敏和 Agent 不重复启动
- [x] 5.3 执行 `cargo flow verify --components backend` 与 `cargo flow verify --components frontend`，修复审计、Clippy、Rust 测试、Angular lint/typecheck/Vitest/build 和 Playwright 失败
- [x] 5.4 执行 `cargo flow verify --all`、`openspec validate --all --strict` 和 workspace 运行演练，保存可追踪测试名称、日志引用与验收结果

## 6. 文档与交付

- [x] 6.1 更新 ADR、Symphony 需求证据矩阵、任务状态机、数据模型、API、配置和未完成清单，关联 workspace 需求到迁移、代码、测试和运行证据
- [x] 6.2 更新 Orchestrator 运维手册，说明受控根目录、worktree、hooks、清理失败告警、重试、滚动部署和回滚步骤，并通过文档链接检查
- [x] 6.3 使用明确文件清单提交并推送；推送前检查当前分支 PR 是否已合并，已合并则创建新 PR，未合并则更新现有 PR
- [x] 6.4 监控 PR 的 CI、arc-flow、供应链检查和 CodeQL；失败时读取日志、修复、重新测试、提交并推送，直到所有必需检查成功
