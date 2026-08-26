## 1. 范围与数据库契约

- [x] 1.1 执行 `cargo flow scope`，读取 `review_context.json` 与 Repository/Service/Handler/Angular 模板，记录本 change 的 backend、frontend、workflow 范围，并验证模板与范围产物可查询。
- [x] 1.2 新增数据库迁移，扩展任务状态为 `skipped`，创建带组织/部门/所有者边界的依赖表、follow-up 幂等表、任务创建来源字段、复合外键、唯一约束和双向索引，并通过一次性 PostgreSQL 正向迁移验证。
- [x] 1.3 为旧任务提供空依赖、兼容创建来源和安全缺省传播策略，编写迁移回滚/滚动部署说明，并通过历史 fixture 升级测试验证旧数据仍可查询与派发。
- [x] 1.4 增加依赖和 follow-up 所需的权限码与种子绑定，验证普通成员、项目管理员和 Scheduler/System Actor 的允许/拒绝矩阵。

## 2. 领域模型与 Repository

- [x] 2.1 增加 `skipped` 状态、依赖动作、依赖投影、阻塞原因、创建来源和 follow-up 请求/结果模型，并通过序列化、非法枚举和向后兼容单元测试。
- [x] 2.2 按 Repository 模板实现依赖查询、创建、整批替换和删除，所有 SQL 强制组织/部门/所有者范围，并通过同组织、跨组织、自依赖、重复边和不可见节点测试。
- [x] 2.3 在依赖写事务中实现确定性锁顺序、递归 CTE 环检测和幂等键摘要校验，并通过串行环、批量替换环、相反方向并发加边及死锁重试的真实 PostgreSQL 测试。
- [x] 2.4 实现 follow-up 请求占额、payload 摘要、结果任务/依赖原子创建和稳定结果重放，验证相同键同 payload 返回原结果、相同键不同 payload 冲突且失败事务不消耗配额。
- [x] 2.5 将依赖满足谓词加入 TaskTracker 候选查询和 `FOR UPDATE SKIP LOCKED` claim，复用一致 SQL 条件，并通过候选扫描后依赖变化、优先级 aging、多 worker 和无依赖旧任务回归测试。
- [x] 2.6 实现任务详情的上下游、策略、阻塞原因和创建来源范围投影，验证范围外节点不会通过 ID、计数、错误或时序信息泄露。

## 3. Service、Handler 与 API 契约

- [x] 3.1 按 Service 模板实现依赖变更用例，校验任务 revision、状态机、权限、范围、边数/深度上限和策略枚举，并通过合法变更、状态冲突、超限及幂等重放测试。
- [x] 3.2 实现绑定来源 run/task 的 Agent follow-up Service，采用封闭 DTO，从服务端派生组织、部门、所有者、仓库、环境和权限上限，并通过未知字段、越权资源、已失效 run、配额和正文大小测试。
- [x] 3.3 按 Handler 模板增加依赖管理、任务关系查询和 Agent follow-up 路由，统一映射未找到、无权限、状态冲突、依赖冲突、配额不足和暂时不可用错误，并通过 Handler/路由集成测试。
- [x] 3.4 扩展任务详情、状态历史和 follow-up 响应的 OpenAPI schema，重新生成/校验 Angular 契约，并通过 OpenAPI snapshot 与 Rust/TypeScript 字段一致性检查。
- [x] 3.5 为依赖变更、传播和 follow-up 创建写入脱敏事件、Scheduler/System Actor 审计、transactional outbox、SSE 更新和低基数指标，验证 Handler 不调用推送供应商且日志/payload 不含敏感字段。

## 4. Orchestrator 与 Agent 工具链

- [x] 4.1 在每轮 dispatch 前增加依赖 reconciliation，按 `fail > skip > wait` 和固化边策略原子传播状态，验证成功解除阻塞及失败、取消、超时三类终态结果。
- [x] 4.2 使用稳定传播键连接终态处理、状态历史、审计、事件和 outbox，验证重复退出、超时、webhook 与 worker 重启不会重复传播、通知或清理。
- [x] 4.3 在 Harness Supervisor/app-server 工具桥中接入受控 follow-up API，只向 Agent 暴露封闭参数和当前 run 身份，验证 Agent 无法提交组织、权限、网络、工具或审批升级字段。
- [x] 4.4 覆盖传输 EOF、恢复重放和进程重启后的 follow-up 幂等行为，验证同一来源 run 只创建一个任务、一组依赖和一组事件。
- [x] 4.5 增加队列依赖等待、传播结果、环冲突、follow-up 接受/拒绝和查询耗时指标，验证标签不包含 task ID、组织 ID、标题或幂等键等高基数数据。

## 5. Angular 任务体验

- [x] 5.1 按 Angular model/service 模板扩展任务、依赖、阻塞与创建来源契约，验证 API service 不绕过 core 认证/错误处理且类型检查通过。
- [x] 5.2 在现有任务详情 store/signal 中加载上下游、阻塞摘要和 follow-up 来源，处理 SSE 刷新、加载、空态、冲突和范围裁剪，并通过 store 单元测试。
- [x] 5.3 在任务详情页展示前置任务、下游任务、传播策略、`skipped` 终态和创建来源，所有文案、Tooltip 与 ARIA 标签使用简体中文，并通过桌面与移动视口组件测试。
- [x] 5.4 为有权限用户提供依赖添加、替换和删除交互，禁用非法自依赖选择并显示后端环冲突/版本冲突；验证无权限用户只有只读视图。
- [x] 5.5 增加 Vitest 与 Playwright 场景，覆盖依赖阻塞、成功解锁、策略跳过/失败、SSE 更新和 follow-up 来源深链接，并验证页面无重叠、文本溢出或未翻译英文。

## 6. 综合测试与安全回归

- [x] 6.1 执行真实 PostgreSQL 集成测试，覆盖环检测、并发写偏差、组织/部门/所有者范围、候选/claim 竞态、终态传播和 follow-up 配额幂等，并保存可追踪测试名称。
- [x] 6.2 执行 Harness 受控假 app-server 测试，覆盖 Agent 工具调用、重复终态、EOF 恢复、重启重放和敏感 payload 脱敏，验证不会重复执行 Agent 或创建后续任务。
- [x] 6.3 执行 backend 与 frontend 范围门禁，修复审计、Clippy、Rust 测试、Angular lint/typecheck/Vitest/build 和 Playwright 失败，并验证 `cargo flow verify --components backend` 与 `cargo flow verify --components frontend` 均通过。
- [x] 6.4 执行 `cargo flow verify --all`，确认 secret scan、架构审计、全量测试、构建和 reviewer 全部通过，并记录最终门禁结果。

## 7. 文档、ADR 与交付

- [x] 7.1 更新 Symphony 需求证据矩阵、任务状态机、数据模型、API 和未完成清单，将 `SY-DAG-001` 至 `SY-DAG-004` 关联到迁移、代码、测试和运行证据，并检查文档与实际实现一致。
- [x] 7.2 更新 Orchestrator 运维手册，说明依赖策略、限额、指标、告警、feature flag、滚动部署和回滚步骤，并通过文档链接检查。
- [x] 7.3 在全部验收通过后将 ADR-0004 从 `Proposed` 改为 `Accepted`，补充迁移、核心模块、测试名、提交和 PR 证据，并运行 OpenSpec 严格校验。
- [ ] 7.4 使用明确文件清单提交并推送；推送前检查当前分支 PR 是否已合并，已合并则创建新 PR，未合并则更新现有 PR，并记录提交 SHA、分支和 PR URL。
- [ ] 7.5 监控 PR 的 CI、arc-flow、供应链检查和 CodeQL；失败时读取日志、修复、重新执行相关测试和 `cargo flow verify --all`、提交并推送，循环直到所有必需检查成功。
