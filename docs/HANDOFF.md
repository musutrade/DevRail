# DevRail 项目状态与交接

更新日期：2026-08-23。长期约定以根目录 `AGENTS.md`、[开发指南](development.md)、[架构说明](architecture.md)、[UI 与 CSS 规范](ui-design-system.md) 和生成的 [OpenAPI 契约](openapi.json) 为准。

DevRail 的产品范围见 [需求文档](requirements.md)，当前实现口径见 [DevRail 实现状态](devrail-implementation-status.md)，业务边界见 [项目公约](devrail-governance.md)，门禁和审计证据见 [审计与门禁](devrail-audit-and-gates.md)。项目通过 `scripts/init-project.sh` 以 `devrail` slug、数据库名和权限前缀完成初始化，并以 `.arc-project.json` 记录 arc-admin 框架版本。

## 重要状态结论

当前完成的是 arc-admin 基线、`arc-flow` 审计工具生产化、治理文档和 CI 配套；Codex Harness 产品 MVP 尚未完成。不得把 `cargo flow verify --all` 或审计工具的通过结果解释为 DevRail 业务需求已验收。完整覆盖矩阵见 [DevRail 实现状态](devrail-implementation-status.md)。

## 当前基线

| 领域     | 当前状态                                                                                               |
| -------- | ------------------------------------------------------------------------------------------------------ |
| 版本     | 框架 `v2.3.0`；Node.js `24.18.0`；Rust `1.97.1`；Angular/Material `22.1`；PostgreSQL `16`              |
| 工作流   | `cargo flow` 统一范围检测、Secret Scan、架构审计、格式、lint、编译、测试、构建和报告                   |
| 后端分层 | Handler → Service → Repository → PostgreSQL；SQL 写入位置和反向依赖由 auditor 阻断                     |
| 认证     | HttpOnly Cookie 服务端会话、CSRF、三维登录限流、会话上限、即时撤销和安全审计                           |
| MFA      | `super_admin` 强制 TOTP；支持通行密钥、恢复码、敏感操作 step-up 和会话撤销                             |
| RBAC     | 组织、层级部门、用户、角色、权限目录、角色授权，以及 organization/department/self 数据范围             |
| 契约     | Rust DTO/utoipa 生成 OpenAPI 3.1，再生成 Angular DTO 与 API Client；漂移由门禁阻断                     |
| 前端     | Angular standalone、signals、zoneless、OnPush；真实 API、权限守卫、亮暗主题和运行时产品配置            |
| UI       | 用户、权限、部门、角色、权限分配、审计、安全和错误页使用统一页面、筛选、表格、状态、卡片与反馈规范     |
| 响应式   | Desktop Chrome 与 Pixel 7 双端覆盖；审计日志在手机切换卡片，权限分配保留可操作的固定末列               |
| 无障碍   | 跳转链接、唯一 main landmark、表格 caption、焦点可见、菜单焦点返回、状态播报和减少动效                 |
| 测试     | ESLint、Prettier、84 项 Angular 单测、Playwright 桌面/移动端 E2E、Rust/PostgreSQL 集成测试和全栈 smoke |
| 运维     | 生产 Compose、独立 migration、JSON 日志、Prometheus/Loki/Grafana、Blackbox、可选 Tempo、备份与审计归档 |
| 供应链   | Dependabot、RustSec、`cargo deny`、CodeQL、Trivy 镜像扫描和 SPDX SBOM                                  |
| DevRail 产品 MVP | 仍未完成；Phase 0 CRUD 与仓库/环境创建入口、仓库远端 HEAD 同步检查、环境健康检查，以及 Phase 1 Harness Supervisor、审批、撤回、过期 worker、活动 run 数据库重启恢复、策略版本校验、受限命令质量门禁执行、质量门禁失败联动、终态重试、审批 UI、运行详情、changeset/质量门禁查询已加入；Phase 2 已加入站内通知、transactional outbox、run 终态通知、审批状态通知、用户通知偏好和通知中心/设置页面，完整资源同步、完整质量门禁定义与日志引用、Web Push、设备注册、投递重试和审计仍待开发 |
| 审计工具配套 | `arc-flow` 生产化、跨平台 CI、性能基准、SBOM 和操作文档已完成 |

## 常用命令

```bash
cargo flow doctor
cargo flow scope
cargo flow verify
cargo flow verify --all
```

前端视觉复核：

```bash
cd frontend
VISUAL_REVIEW=1 npm run e2e -- --project=chromium --project=mobile-chromium
```

后端集成测试默认启动并销毁一次性 `postgres:16-alpine` 容器；也可显式导出指向隔离测试库的 `TEST_DATABASE_URL`。测试库不得与开发库或生产库相同。

## 生产环境仍需完成

这些事项依赖具体域名、基础设施、密钥和责任人，不能由仓库默认值代替：

1. 配置真实 TLS、`DATABASE_URL`、`MFA_ENCRYPTION_KEY`、WebAuthn RP 与 CORS origin，并完成 MFA 全流程演练；
2. 配置 Grafana 联系点和通知策略，验证 `Firing` 与 `Resolved` 均能送达；
3. 在应用故障域之外配置 HTTPS 心跳，覆盖主机、网络、证书、代理和应用整体故障；
4. 保护 `main`，将 Quality gate、Backend verification、Frontend verification 设为 required checks，并启用 secret scanning 与 push protection；
5. 将审计归档、SBOM、备份和发布证据写入权限独立的不可变存储，并执行恢复演练；
6. 根据业务 RPO/RTO、数据分类、司法辖区与合同要求补齐高合规控制；
7. 按 [DevRail 实现状态](devrail-implementation-status.md) 和 [需求文档](requirements.md) 继续完成本地工作树状态、完整资源同步、更丰富的质量门禁日志后端及通知推送链路；当前已落地仓库远端 HEAD/默认分支/分支数量检查、环境健康检查、受限命令质量门禁执行、结构化门禁元数据、站内通知、transactional outbox、run/审批状态通知和通知中心，Web Push 设备/偏好、dispatcher、投递重试/审计仍未完成。

## 已知边界

- 漏洞库和容器镜像结果会随时间变化，真实结论以 GitHub Actions 的 PR 和定时安全任务为准；
- 同一 Docker 环境中的 Blackbox 和 Tempo 无法覆盖整机或区域故障，也不是不可变证据库；
- 数据库触发器、宿主机卷和本机日志都受其管理员控制，高合规证据必须外置；
- 前端权限只改善体验，所有新增 API 与行级数据范围仍必须在后端强制执行；
- `arc-flow` auditor 是确定性补充门禁，不能替代语言解析器、Clippy、ESLint、CodeQL 或人工安全评审。
