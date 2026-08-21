<!-- ARC_PROJECT_HEADER_START -->
# DevRail

基于 arc-admin v2.3.0 权限框架构建。项目标识：`devrail`。
<!-- ARC_PROJECT_HEADER_END -->

当前框架版本为 `v2.3.0`。仓库内置身份认证、组织与部门、用户、角色、权限目录、权限分配、审计日志、`super_admin` MFA，以及开发、测试、部署和可观测性工具链。

## 核心能力

| 领域       | 已实现能力                                                                          |
| ---------- | ----------------------------------------------------------------------------------- |
| 身份认证   | HttpOnly Cookie 服务端会话、CSRF、防暴力破解、会话上限、密码修改与强制撤销          |
| 多因素认证 | `super_admin` 强制 TOTP，支持通行密钥、一次性恢复码和敏感操作二次认证               |
| 权限模型   | 用户、层级部门、角色、权限目录、角色授权、组织/部门/本人数据范围                    |
| 安全审计   | 登录与权限变更审计、追踪号检索、数据库只追加保护、归档与校验清单                    |
| 前端体验   | Angular 22 standalone、signals、zoneless、OnPush、亮/暗主题、响应式布局与键盘无障碍 |
| 工程质量   | OpenAPI 生成客户端、Vitest、Playwright 桌面/移动端 E2E、Rust 集成测试、统一质量门禁 |
| 生产运维   | 非 root 容器、TLS/Nginx、独立迁移任务、Prometheus/Loki/Grafana、备份与 PITR 指南    |

## 页面与访问控制

| 路由                   | 页面             | 访问要求                         |
| ---------------------- | ---------------- | -------------------------------- |
| `/login`               | 登录与 MFA 验证  | 公开                             |
| `/permissions`         | 权限目录         | `permission:directory:read`      |
| `/users`               | 用户管理         | `user:directory:read`            |
| `/departments`         | 部门管理         | `organization:department:read`   |
| `/roles`               | 角色管理         | `role:directory:read`            |
| `/role-permissions`    | 角色权限分配     | 角色、权限目录读取及角色授权权限 |
| `/audit-logs`          | 审计日志         | `audit:logs:read`                |
| `/security`            | 当前账号安全设置 | 已登录                           |
| `/403`、`/404`、`/500` | 统一错误页       | 公开                             |

前端守卫和按钮显隐只负责交互反馈；所有受保护 API 都由后端重新校验当前会话、账号状态、权限码和数据范围。

## 技术栈

- Angular `22.1`、Angular Material `22.1`、TypeScript、SCSS；
- Rust `1.97.1`、Axum、SQLX、utoipa；
- PostgreSQL `16`；
- Node.js `24.18.0`；
- Vitest、Playwright、Clippy、ESLint、Prettier；
- Docker Compose、Nginx、Prometheus、Loki、Alloy、Grafana，可选 OpenTelemetry/Tempo。

工具链版本由 `.node-version` 和 `rust-toolchain.toml` 固定。升级依赖前先阅读 [开发指南](docs/development.md) 和 [框架升级说明](docs/framework-upgrades.md)。

## 目录结构

```text
.
├── frontend/                  # Angular 22 管理端
│   └── src/app/{core,features,layout,pages}
├── backend/                   # Axum API、领域分层和 SQLX 迁移
│   ├── src/{handlers,services,repositories}
│   └── migrations/            # 只增不改的数据库迁移
├── docs/                      # 架构、UI、安全、部署与运维文档
├── deployment/                # 生产环境示例配置和 TLS 挂载目录
├── observability/             # Prometheus、Blackbox、Loki、Alloy、Grafana、Tempo
├── codex-audit-pipeline/      # arc-flow 工作流与架构审计工具
├── scripts/                   # 项目初始化、框架升级和质量检查脚本
├── compose.production.yaml    # 最小生产部署
├── FRAMEWORK_VERSION          # 当前框架版本
└── .github/workflows/         # CI、CodeQL 与供应链安全工作流
```

## 本地启动

需要 Git、Docker，以及仓库固定版本的 Node.js 和 Rust。

```bash
# 一次性准备
cd frontend && npm ci && cd ..
[[ -f backend/.env ]] || cp backend/.env.example backend/.env
docker pull postgres:16-alpine
git config core.hooksPath codex-audit-pipeline/hooks
cargo flow doctor

# 前后端一键启动；Ctrl+C 会同时停止两个服务
./start.sh
```

默认访问地址：

- 前端：`http://localhost:4200`；
- 后端存活检查：`http://localhost:8080/api/v1/healthz`；
- 后端就绪检查：`http://localhost:8080/api/v1/readyz`。

数据库迁移不会创建可登录的默认账号。首次运行时显式初始化管理员，密码至少 16 个字符：

```bash
BOOTSTRAP_ADMIN_PASSWORD='replace-with-a-strong-password' \
  cargo run --manifest-path backend/Cargo.toml --bin bootstrap_admin
```

也可以分别运行 `cd frontend && npm start` 和 `cd backend && cargo run`。`backend/.env` 中的 `DATABASE_URL` 必须指向本地开发数据库，不得复用生产数据库。

## 开发与验证

所有 `cargo flow` 命令从仓库根执行：

```bash
# 查看工作区变更影响范围
cargo flow scope

# 验证受影响组件
cargo flow verify

# 合并或创建 PR 前执行全部组件
cargo flow verify --all
```

前端定向检查：

```bash
cd frontend
npm run lint
npm run format:check
npm run test:ci
npm run e2e
npm run build
```

`cargo flow verify --all` 固定先执行凭据扫描和架构审计，再运行格式、lint、编译、单元/集成测试、桌面与移动端 E2E、真实全栈 smoke 和生产构建。后端测试默认使用一次性 PostgreSQL 容器；如提供 `TEST_DATABASE_URL`，它必须指向明确隔离且以 `_test` 或 `-test` 结尾的测试库。

## API 契约

[backend/src/openapi.rs](backend/src/openapi.rs) 与后端 DTO 是 HTTP 契约的可编辑来源。[docs/openapi.json](docs/openapi.json) 和 `frontend/src/app/generated/api/` 均为生成产物，禁止手工修改。

修改接口后执行：

```bash
cd frontend
npm run generate:api:all
```

开发服务器把 `/api/v1` 代理到 `http://127.0.0.1:8080`。前端使用 `HttpClient`、HttpOnly Cookie 会话与 CSRF interceptor 访问 API；运行时 API 根路径可通过公开配置覆盖。

## 运行时产品配置

前端启动时读取 `frontend/public/config.js`，修改后无需重新构建：

```javascript
window.__ARC_ADMIN_CONFIG__ = {
  appName: "DevRail",
  appShortName: "DevRail",
  appSlug: "devrail",
  apiBaseUrl: "/api/v1",
  themeStorageKey: "devrail-theme",
};
```

该文件会控制浏览器标题、登录页、导航品牌、API 地址和主题存储键。它会被浏览器直接读取，禁止写入 API 密钥、会话凭据、数据库连接串或其他秘密。

## 生产部署

最小生产拓扑由 `compose.production.yaml` 提供 PostgreSQL、一次性 migration、Axum API 和 Angular/Nginx。上线前必须完成真实 TLS、数据库与 MFA 密钥、CORS、备份、告警通知和外部心跳配置。

从 [最小生产部署](docs/production-deployment.md) 开始，并按 [多因素认证运维](docs/mfa-operations.md)、[日志与故障定位](docs/observability.md) 和 [PostgreSQL 备份与恢复](docs/postgresql-backup-recovery.md) 完成验收。示例配置不是生产密钥文件，也不构成合规认证。

## 文档导航

- [开发指南](docs/development.md)：环境、日常开发、质量门禁与 Git 约束；
- [架构说明](docs/architecture.md)：前后端分层、权限模型、数据范围和 API 契约；
- [UI 与 CSS 规范](docs/ui-design-system.md)：设计 token、页面结构、响应式、交互和无障碍检查清单；
- [前端说明](frontend/README.md)：Angular 目录、认证交互、页面与定向验证命令；
- [业务模块扩展指南](docs/business-extension.md)：新业务域的前后端接入边界；
- [业务权限模板](docs/business-permissions.md)：SQL、Rust、Angular 三端权限声明；
- [安全能力状态](docs/security-roadmap.md)：已实现的高权限认证控制和生产责任；
- [多因素认证运维](docs/mfa-operations.md)：MFA 密钥、WebAuthn、恢复与撤销演练；
- [审计日志保留](docs/audit-retention.md)：只追加保护、归档、校验与恢复；
- [日志与故障定位](docs/observability.md)：指标、日志、告警、心跳与链路追踪；
- [Grafana 告警通知](docs/grafana-alert-notifications.md)：联系点、通知策略和送达验收；
- [供应链安全](docs/supply-chain-security.md)：依赖审计、CodeQL、镜像扫描与 SBOM；
- [最小生产部署](docs/production-deployment.md)：Compose、TLS、发布和回滚；
- [PostgreSQL 备份与恢复](docs/postgresql-backup-recovery.md)：备份、PITR、RPO/RTO 与演练；
- [高合规部署基线](docs/high-compliance.md)：需要在项目层补齐的独立控制；
- [框架版本与派生项目升级](docs/framework-upgrades.md)：版本标签、三方合并和冲突处理；
- [当前项目状态](docs/HANDOFF.md)：最新能力、验证基线和剩余生产事项；
- [DevRail 需求文档](docs/requirements.md)：产品范围、Agent 运行、审批和手机推送需求；
- [DevRail 项目公约](docs/devrail-governance.md)：业务边界、Harness、数据和 PR 约定；
- [DevRail 审计与门禁](docs/devrail-audit-and-gates.md)：本地、CI 和发布检查清单；
- [arc-flow 操作手册](codex-audit-pipeline/README.md) 与 [schema v2 参考](codex-audit-pipeline/docs/configuration.md)。

## DevRail 项目约定

DevRail 不是直接修改 arc-admin 源仓库，而是以 `.arc-project.json` 登记的派生项目。框架管理文件由 arc-admin 升级器维护，DevRail 业务代码、需求和推送能力按 [项目公约](docs/devrail-governance.md) 扩展。

- 业务权限前缀固定为 `devrail`，新增权限必须同步数据库、Rust 和 Angular 三端声明；
- Agent 只能经后端 Harness Supervisor 执行，浏览器不得直连 `codex app-server`；
- 工具审批、工作区隔离、事件脱敏和手机推送 outbox 是强制安全边界；
- PR 合并前必须通过 [审计与门禁](docs/devrail-audit-and-gates.md) 规定的本地和 CI 检查。

## 升级派生项目

派生项目不会自动获得框架后续修复。框架发布新版本后，从检出目标正式标签的模板仓库先预检，再执行三方合并：

```bash
../arc-admin-framework/scripts/upgrade-framework.sh --check
../arc-admin-framework/scripts/upgrade-framework.sh
```

升级器只处理框架清单登记的文件，保留业务新增文件；冲突时不会写入。完整流程见 [框架版本与派生项目升级](docs/framework-upgrades.md)。
