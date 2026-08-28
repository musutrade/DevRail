# 架构说明

## 系统边界

- `frontend/`：Angular 22 standalone 应用，负责界面、路由和前端交互状态。
- `backend/`：Axum API，负责认证、授权、业务规则和持久化。
- `backend/src/openapi.rs` 与后端 DTO：前后端 HTTP 契约的可编辑来源；`docs/openapi.json` 和 Angular Client 均为生成产物。
- `codex-audit-pipeline/`：`arc-flow` Rust CLI，统一负责范围路由、静态审计、安全扫描、测试编排、报告和 Git hook profile。

## 后端分层

请求按 `Router -> Handler -> Service -> Repository -> PostgreSQL` 流动：

- Handler 解析 HTTP 输入并映射状态码，不直接写 SQL。
- Service 编排业务规则和事务边界，不直接写 SQL。
- Repository 是运行时 SQL 读写的唯一入口。
- Model 定义 API 和数据库数据结构，不包含业务流程。
- migrations、tests 和 seed 可执行数据库写操作，但不得连接生产库。

依赖只能向下流动。Handler 禁止直接依赖 Repository，Repository 禁止依赖 Handler 或
Service；这些规则与 SQL 写入位置一起由 auditor 强制检查。新增业务继续在单体内按同名
模块横向落到 `handlers/`、`services/`、`repositories/`，不要把整个业务塞进某一层，
也不要在没有独立部署需求时提前拆微服务。

`backend/src/lib.rs` 暴露可复用的 `build_router`，生产进程和集成测试使用同一套路由。`backend/src/main.rs` 仅负责读取配置、连接数据库并启动监听。开发环境默认自动迁移；生产环境默认关闭，由独立 `migrate` 任务在应用副本启动前完成迁移。

## Symphony 任务与工作流边界

- `backend/src/orchestration/task_tracker.rs` 是调度器访问任务存储的领域端口；PostgreSQL adapter 复用 Repository，worker 不持有 SQLX 查询或表结构知识。
- `backend/src/orchestration/workflow.rs` 只从受控仓库根目录读取 `WORKFLOW.md`，执行严格 schema、封闭模板、安全策略交集和规范化摘要计算。使用方式见 [仓库工作流契约](workflow-contract.md)。
- `draft → queued` 是派发输入的原子快照边界。任务修订号、任务输入和 workflow 来源/版本/摘要/规范化内容在同一事务固化；排队后禁止原地修改。
- run 创建在 Repository SQL 中再次比对 task 修订号及 workflow 三元身份，并复制完全相同的 workflow 快照。Harness 只接收持久化快照渲染出的输入，不重新读取磁盘文件。
- workflow reloader 以有界抖动轮询校验候选版本，合法版本只供之后入队的任务使用；非法版本保留 PostgreSQL 中的 last-known-good，并产生去重审计和低基数指标。

- 任务依赖 DAG、受控 follow-up 和 per-task workspace/worktree 已作为独立能力落地；workspace 的路径、hooks、快照、清理和诊断边界见 [Task Workspace Manager 规范](../openspec/specs/task-workspace-manager/spec.md)。
- continuation turns 已作为独立能力落地：同一 Codex thread 使用新 turn 和新 child run，来源终态保持不可变，handoff 证据先于 cleanup；实现决策见 [ADR-0005](adr/ADR-0005-continuation-turn-lifecycle.md)。外部 tracker adapter 仍是后续独立能力，不得用故障恢复或 follow-up 语义替代 continuation。

用户-角色和角色-权限写入由 Service 开启事务并把同一个连接传给 Repository，主记录与关联表要么同时成功，要么同时回滚。内置 `super_admin` 不能被停用或清空权限，最后一个有效超级管理员不能被停用、删除或移除角色。

## 组织与数据范围

权限码与角色定义是平台级全局目录；用户必须归属一个 `organization`，可以归属一个层级 `department`。组织是模板中的租户边界，不允许使用部门代替租户。`super_admin` 可以跨组织管理，其余角色的数据范围为 `organization`、`department_and_children`、`department` 或 `self`。

认证成功后，Repository 从会话、用户和角色实时生成 `ActorContext`，其中包含用户、会话、组织、部门、有效数据范围和权限码。权限码回答“能否执行动作”，数据范围回答“可以操作哪些行”，两项必须同时满足。内置用户目录、仪表盘和审计日志已经按该上下文过滤，越界资源统一表现为不存在。

所有新增业务表必须包含 `organization_id`、可空 `department_id` 和 `owner_user_id`，外键需保证部门属于同一组织。Handler 把 `RequirePermission` 中的 Actor 传给 Service，Service 原样传给 Repository；Repository 在 SQL 中完成组织、部门树和所有者过滤。禁止先全量读取后在 Service 或前端过滤。标准实现见 `rust_handler.rs.tmpl`、`rust_service.rs.tmpl` 和 `rust_repository.rs.tmpl`。

登录成功后，浏览器只持有 256-bit 随机会话标识；标识通过 HttpOnly、`SameSite=Strict` Cookie 发送，数据库仅保存其 SHA-256 哈希。每个受保护请求都会从数据库重新读取会话、账号有效状态和权限码，因此退出、密码变更、停用账号或修改角色权限会立即撤销会话或按新权限执行。所有受保护写请求还必须通过与会话绑定的 CSRF Cookie 和 `X-CSRF-Token` 双提交校验。Handler 使用类型化权限提取器声明所需权限，前端隐藏按钮仅用于交互体验，不作为安全边界。

登录失败按“账号”“来源 IP”“账号+来源 IP”三个哈希桶在 PostgreSQL 中计数，多实例共享同一限制。账号和组合桶默认 15 分钟内失败 5 次后锁定 15 分钟，来源 IP 桶默认 50 次，以降低公司出口 NAT 被少量误输整体锁定的风险。登录成功只清理账号与组合桶，不清理来源 IP 桶，避免攻击者用已知账号重置 IP 限制。登录成功、失败和退出都会写入安全审计；退出、密码变更、管理员重置、账号停用/删除及会话上限淘汰还会统一写入 `auth.session.revoked`，并记录撤销原因和数量。会话表及审计日志不保存原始 Cookie、密码、CSRF Token、IP 或完整 User-Agent。每个用户默认最多保留 10 个有效会话，超出后撤销最早会话。

来源 IP 默认取 TCP 直连地址。只有直连地址命中 `TRUSTED_PROXY_CIDRS` 时才解析 `X-Forwarded-For`，并从右向左跳过受信代理得到最近的非受信地址；未受信客户端提供的转发头和格式错误的转发链一律不采用。部署 Nginx、Ingress 或云负载均衡时必须只配置实际代理网段，并由最外层代理覆盖而不是追加客户端传入的 `X-Forwarded-For`。

认证与业务权限标记分离：`backend/src/auth.rs` 只实现认证和通用 `RequirePermission` 提取器，`backend/src/permissions.rs` 保存平台权限，新增业务权限按模块放入 `backend/src/permissions/`。数据库、Rust 与 Angular 的权限码从[业务权限模板](business-permissions.md)同步创建。

## 前端业务边界

- `core/` 保存认证、运行时配置、拦截器及 RBAC 平台资源客户端，不依赖业务代码。
- `features/<domain>/` 保存新增业务域自己的页面、数据访问、模型和测试。
- `app.routes.ts` 与 `app.navigation.ts` 是应用组合根，负责把业务入口接入壳层。
- 路由守卫和导航项引用同一个 `ROUTE_ACCESS` 权限要求；未声明权限的受保护路由默认拒绝。
- `_tokens.scss` 保存设计 token，`styles.scss` 保存跨页面 UI 模式，页面 SCSS 只维护页面独有布局。

依赖方向固定为 `app 组合根 -> features -> core`。业务域之间不直接引用内部页面或服务；
跨域流程留在组合层，稳定且无业务归属的能力才允许提取到 `core`。模板自带的 RBAC 页面
暂留在 `pages/` 作为平台功能，新业务不得继续加入该目录。

具体接入流程与目录示例见 [业务模块扩展指南](business-extension.md)，页面结构、响应式和无障碍要求见 [UI 与 CSS 规范](ui-design-system.md)。

## 配置与安全

| 变量                                   | 说明                                               |
| -------------------------------------- | -------------------------------------------------- |
| `DATABASE_URL`                         | 必填 PostgreSQL 连接串                             |
| `DB_MAX_CONNECTIONS`                   | API 单副本最大连接数，默认 10                      |
| `DB_MIN_CONNECTIONS`                   | API 单副本预热连接数，默认 1                       |
| `DB_ACQUIRE_TIMEOUT_SECS`              | 从连接池取连接的超时，默认 5 秒                    |
| `DB_CONNECT_TIMEOUT_SECS`              | 应用启动建立连接池的超时，默认 10 秒               |
| `DB_IDLE_TIMEOUT_SECS`                 | 空闲连接回收时间，默认 600 秒                      |
| `DB_MAX_LIFETIME_SECS`                 | 单连接最长生命周期，默认 1800 秒                   |
| `DB_STATEMENT_TIMEOUT_MS`              | PostgreSQL 单语句超时，默认 30000 毫秒             |
| `AUTO_MIGRATE`                         | 是否随 API 启动迁移；开发默认 true，生产默认 false |
| `PORT`                                 | 监听端口，默认 8080，范围 1-65535                  |
| `APP_ENV`                              | `development`、`test` 或 `production`              |
| `SESSION_TTL_SECS`                     | 普通会话绝对有效期，默认 8 小时                    |
| `SESSION_IDLE_TIMEOUT_SECS`            | 普通会话空闲有效期，默认 30 分钟                   |
| `PERSISTENT_SESSION_TTL_SECS`          | “记住我”会话绝对有效期，默认 30 天                 |
| `PERSISTENT_SESSION_IDLE_TIMEOUT_SECS` | “记住我”会话空闲有效期，默认 7 天                  |
| `MAX_SESSIONS_PER_USER`                | 每个用户最多有效会话数，默认 10                    |
| `LOGIN_MAX_FAILURES`                   | 单账号登录失败锁定阈值，默认 5                     |
| `LOGIN_IP_MAX_FAILURES`                | 单来源 IP 登录失败锁定阈值，默认 50                |
| `LOGIN_ACCOUNT_IP_MAX_FAILURES`        | 账号与来源 IP 组合失败锁定阈值，默认 5             |
| `LOGIN_FAILURE_WINDOW_SECS`            | 登录失败统计窗口，默认 900 秒                      |
| `LOGIN_LOCKOUT_SECS`                   | 达到阈值后的锁定时长，默认 900 秒                  |
| `TRUSTED_PROXY_CIDRS`                  | 逗号分隔的受信反向代理网段，默认空                 |
| `CORS_ALLOWED_ORIGINS`                 | 逗号分隔 origin；生产环境必填且禁止 `*`            |
| `MFA_ENCRYPTION_KEY`                   | 生产必填；Base64 编码的 32 字节 TOTP 加密密钥      |
| `WEBAUTHN_RP_ID`                       | Passkey 绑定的 relying party 域名                  |
| `WEBAUTHN_RP_ORIGIN`                   | Passkey 页面实际使用的 HTTPS origin                |
| `WEBAUTHN_RP_NAME`                     | 身份验证器显示的站点名称                           |
| `LOG_FORMAT`                           | `pretty` 或 `json`；生产环境默认 `json`            |
| `RUST_LOG`                             | Rust 日志过滤规则                                  |
| `SERVICE_NAME`                         | 日志中的服务标识，默认 `arc-admin-backend`         |

管理员账号不再由固定密码迁移创建。`bootstrap_admin` 使用进程环境传入的至少 16 字符密码，并在事务中创建或激活账号及绑定 `super_admin`。

凭据只存放在进程环境或已忽略的 `backend/.env`，不得进入 Git remote、日志、报告或文档。

## 契约与测试

- `API_ROUTE_CONTRACT` 和 `backend/tests/openapi_contract.rs` 校验生成路径与实际路由一致，`workflow.api-generation` 校验 OpenAPI 与 Angular Client 没有漂移。
- `backend/tests/api_flow.rs` 使用真实迁移和 PostgreSQL，覆盖安全初始化、默认密码失效、Cookie/CSRF、登录限流、会话撤销、权限拒绝、事务回滚和超级管理员保护。
- `/healthz` 仅表示进程存活；`/readyz` 检查 PostgreSQL，可用于负载均衡就绪探针。服务收到 Ctrl+C 或 SIGTERM 后执行优雅停机。
- Angular 单测使用 Vitest/jsdom；Playwright 使用拦截 API 的确定性数据，在 Desktop Chrome 和 Pixel 7 视口覆盖登录、权限导航、用户操作、部门、角色授权、审计、安全设置、错误页及未认证重定向；真实全栈 smoke 连接 Angular、Axum 与隔离 PostgreSQL。
- arc-flow 对 SQL 写入位置、分层依赖和旧模板模式做确定性扫描，并管理外部命令超时与一次性 PostgreSQL 生命周期。

所有请求、响应、枚举和参数由 `utoipa` 从 Rust 类型生成 OpenAPI 3.1，再由 `ng-openapi-gen` 生成 Angular DTO 和调用函数。禁止手改 `docs/openapi.json` 或 `frontend/src/app/generated/api/`。

`super_admin` 在密码通过后只得到五分钟的一次性 MFA 挑战，完成 TOTP、通行密钥或恢复码验证后才创建服务端会话。挑战和 WebAuthn ceremony 状态只保存在 PostgreSQL；TOTP 密钥使用用户 ID 作为附加认证数据进行 AES-256-GCM 加密，恢复码只保存 Argon2 哈希。角色被提升为 `super_admin` 后，未完成 TOTP 注册的既有会话会在下一次请求时失效。生产配置和恢复要求见 [多因素认证运维](mfa-operations.md)。
