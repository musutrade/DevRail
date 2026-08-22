# 变更日志

本项目遵循语义化版本。每个发布版本必须对应一个不可移动的同名 Git 标签，派生项目升级时使用标签内容作为三方合并基线。

## [未发布] - arc-flow 3.0.0

### 破坏性变更

- audit 配置升级到 schema v2，必须显式声明 `version = 2` 和 `[engine]`；缺失或未知版本不再使用隐式默认值；
- audit allowlist 不再接受含义不明确的字符串，必须逐项声明 `path-prefix` 或 `regex`；
- 每条 audit 规则使用的扩展名必须配置 `engine.comment_syntax`，否则门禁 fail closed。

### 工作流

- Secret Scan 配置升级为独立 schema v2，规则、占位符、本地测试数据库放行条件和报告策略均由受版本控制的配置声明；
- audit engine 的忽略文件名、报告文件名、Markdown 限制、注释和字符串定界符全部配置化；
- 新项目 audit preset 预置 Rust、TypeScript、JavaScript、SQL、TOML 和 YAML 的词法配置，并验证初始化后添加第一条规则的完整流程。

### 改进

- arc-flow auditor 增加 Rust 任意 raw-string、TypeScript/JavaScript 正则与嵌套模板、PostgreSQL dollar-quote 和转义字符串的词法边界，缓存文件内容/行索引/注释区间与 allowlist 正则，并增加随机稳健性测试；
- 增加 10,000 文件审计 benchmark（含单线程/并行一致性、峰值 RSS 与 CI JSON artifact）、临时仓库初始化/迁移/失败恢复演练、Linux/Windows workflow 矩阵、SPDX 2.3 SBOM 和 SHA-256 发布证据；
- 项目初始化同步开发、可观测性与生产部署的数据库、服务、镜像、监控网络和 WebAuthn 标识，并加强 API 基址与框架源仓库保护；
- 前端补齐键盘跳转、移动导航焦点、加载状态、表格与筛选控件的无障碍语义，统一焦点样式并优化移动端交互。
- 用户、权限、部门、角色、权限分配、审计、安全与错误页统一页面结构、筛选、表格、状态和统计卡样式；审计日志增加移动端卡片视图；
- README 改为 arc-admin 项目入口文档，新增 UI/CSS 规范，并同步认证、页面、测试、部署与安全状态说明。

### 升级说明

- 从当前 `empty.audit.toml` 复制 `[engine]` 和所需 `comment_syntax`，为旧配置补充 `version = 2`；
- 将旧字符串 allowlist 按真实意图转换为 `{ kind = "path-prefix", path = "..." }` 或 `{ kind = "regex", pattern = "..." }`；完整示例见 `codex-audit-pipeline/docs/configuration.md` 的 Audit v2 迁移章节。

## [v2.3.0] - 2026-08-08

### 新增

- 用户目录改为服务端搜索、角色筛选、排序和分页，避免前端并发拉取全部用户；
- 由 Rust DTO 生成 OpenAPI 3.1，再生成 Angular DTO/API Client，并增加契约漂移门禁；
- 增加连接真实 Angular、Axum 与一次性 PostgreSQL 的跨端冒烟测试；
- 增加 RustSec、`cargo deny`、CodeQL、Trivy 镜像扫描和 SPDX SBOM 工作流；
- 增加 Prometheus 应用与连接池指标、Blackbox 内部探测、外部心跳接入说明，以及可选的 OpenTelemetry/Tempo 链路追踪；
- 审计日志增加来源指纹、数据库级只追加保护、JSON Lines 归档、SHA-256 清单与保留任务。

### 升级说明

- 前端不再维护手写 API 字段定义；修改接口时先更新 Rust DTO 与 `backend/src/openapi.rs`，再运行 `npm run generate:api:all`；
- 生产环境应配置独立的审计归档目录与异地不可变存储，并按 `docs/audit-retention.md` 执行恢复演练；
- Prometheus、Blackbox 和 Tempo 通过 `arc-admin-monitoring` 容器网络访问应用；真正的外部可用性监测仍需部署在故障域之外；
- Tempo 与高合规参数默认不启用，只有明确的数据分类、保留策略和容量预算后才应使用。

## [v2.2.0] - 2026-08-08

### 新增

- 增加组织、层级部门、角色数据范围和统一 `ActorContext`，用户目录、仪表盘与审计日志按数据范围查询；
- CRUD 模板默认传递 Actor，并在 Repository SQL 中强制过滤组织、部门树和资源所有者；
- 增加前后端多阶段非 root 镜像、TLS 反向代理、资源限制、健康检查和最小生产 Compose；
- 增加独立 migration job、数据库连接池与查询超时配置，以及 PostgreSQL 备份、PITR 和恢复演练文档。

### 升级说明

- 迁移会创建默认组织和根部门，并把历史用户及审计记录归入该组织；现有业务数据表需按数据范围文档补齐归属列；
- 生产环境 `AUTO_MIGRATE` 默认关闭，部署流程必须先运行 `/app/migrate`；开发环境仍默认自动迁移；
- 角色新增 `dataScope` 字段。内置角色由迁移赋予默认范围，自定义角色默认仅本人；
- 使用生产 Compose 前必须配置真实 TLS 证书、数据库凭据和 `DATABASE_URL`，并把 WAL 归档复制到独立故障域。

## [v2.1.0] - 2026-08-08

### 安全

- 登录失败限制扩展为账号、来源 IP、账号与来源 IP 组合三个独立哈希桶；
- 增加受信代理网段配置，只有受信反向代理的转发链才参与来源 IP 判定；
- 所有实际会话撤销统一写入 `auth.session.revoked`，包含撤销原因和数量且不记录原始 IP；
- 将 `super_admin` 多因素认证列入安全待办，并定义无降级旁路的验收条件。

### 升级说明

- 反向代理部署应按 `backend/.env.example` 配置 `TRUSTED_PROXY_CIDRS`；直连部署保持为空；
- 可按企业 NAT 和攻击面调整 `LOGIN_IP_MAX_FAILURES` 与 `LOGIN_ACCOUNT_IP_MAX_FAILURES`，配置值必须为正整数；
- 本版本不新增数据库结构，既有 `auth_login_attempts` 表会按新的哈希键自然承载三个限流维度。

## [v2.0.0] - 2026-08-08

### 安全

- 使用数据库持久化的 256-bit 随机会话替代浏览器可读 JWT，数据库仅保存会话与 CSRF Token 的 SHA-256 哈希；
- 会话 Cookie 启用 HttpOnly、`SameSite=Strict`，生产环境额外启用 `Secure` 与 `__Host-` 前缀；
- 所有受保护写请求增加会话绑定的双提交 CSRF 校验；
- 增加多实例共享的登录失败窗口与账号锁定、单用户会话上限、空闲/绝对过期和过期数据清理；
- 登录成功、登录失败和退出登录写入结构化安全审计，密码变更与管理员重置会撤销已有会话；
- 用户密码策略统一为 12-128 个字符，引导超级管理员仍要求至少 16 个字符。

### 变更

- 登录响应删除 `accessToken`、`tokenType` 和 `expiresIn`，改为返回 `expiresAt` 与用户信息；
- Angular 删除 Web Storage 认证凭据，改用 Cookie 凭据与 CSRF 拦截器，并增加服务端退出接口；
- 删除 `JWT_SECRET`、`TOKEN_TTL_SECS`，增加会话寿命、会话上限和登录锁定配置。

### 升级说明

- 前后端必须同时升级，旧 Bearer 客户端与新 Cookie 会话接口不兼容；升级后需清理浏览器中的 `arc-auth` Local/Session Storage；
- 生产环境前后端必须处于同一站点并使用 HTTPS；跨 origin 部署需明确配置 `CORS_ALLOWED_ORIGINS`，不能使用 `*`；
- 部署前检查并按 `backend/.env.example` 补齐新的会话配置，旧 JWT 环境变量不再读取；
- 数据库启动迁移会新增 `auth_sessions` 与 `auth_login_attempts`，不会迁移或继续接受旧 JWT，所有用户需要重新登录。

## [v1.1.0] - 2026-08-08

### 新增

- 增加运行时产品配置和一次性项目初始化命令；
- 建立前后端业务扩展边界与业务权限模板；
- 增加模板质量门禁、结构化请求日志、Loki、Alloy 和 Grafana 集中日志；
- 增加基于版本标签、框架文件清单和三方合并的派生项目升级命令。

### 安全

- 禁用历史默认管理员，改为显式安全初始化；
- 加固超级管理员授权、会话撤销、事务一致性和 RBAC 审计；
- 增加生产 JWT、CORS、敏感日志和错误响应保护。

### 升级说明

- `v1.1.0` 是首个支持自动升级的基线版本；更早版本需要先人工合并到本版本。
- 从本版本开始，数据库迁移文件使用 UTC 时间戳版本，降低框架与业务迁移重名风险。

## [v1.0.0] - 2026-08-08

- 提供 Angular、Axum、PostgreSQL RBAC 管理基础功能。
