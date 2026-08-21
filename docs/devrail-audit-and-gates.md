# DevRail 审计与质量门禁

## 1. 门禁目标

DevRail 的门禁分为确定性检查、编译/测试检查和人工审查三层。`arc-flow` 负责统一编排；先执行 Secret Scan 和架构审计，二者通过后才运行语言工具和测试。任何 blocker、secret、编译错误或关键测试失败都必须阻止合并。

## 2. 检查分层

| 层级 | 工具/配置 | 关注点 | 失败处理 |
| --- | --- | --- | --- |
| L0 | `git diff --check`、hook syntax | 空白、脚本语法、暂存快照 | 立即修复 |
| L1 | `cargo flow secrets` | token、私钥、连接串、环境秘密、Webhook | 阻止提交和 PR |
| L1 | `cargo flow audit` | SQL 写入位置、分层依赖、敏感日志、前端 core 边界 | blocker 必须修复；误报须补规则/注释说明 |
| L2 | Rust fmt/Clippy/check/test | 后端质量、状态机、权限和 PostgreSQL 集成 | 阻止合并 |
| L2 | ESLint/Prettier/Vitest/Playwright/build | Angular 22、响应式 UI、无障碍和 API 交互 | 阻止合并 |
| L2 | OpenAPI/template/config scripts | 契约、模板、生产配置、审计保留、供应链 | 阻止合并 |
| L3 | CodeQL、RustSec、cargo-deny、Trivy、SBOM | 供应链和静态安全 | 高危/严重问题阻止发布 |
| L4 | PR 人工审查 | 业务安全、数据范围、用户体验、回滚方案 | 未完成不得合并 |

## 3. 必须执行的命令

从仓库根目录执行：

```bash
# 配置和环境
cargo flow config check
cargo flow doctor --strict

# 变更范围和快速门禁
cargo flow scope
cargo flow verify

# PR/发布前完整门禁
cargo flow verify --profile full --all
```

前端依赖准备：

```bash
cd frontend
npm ci
npm run lint
npm run format:check
npm run test:ci
npm run build
```

后端集成测试必须使用一次性 PostgreSQL 或名称以 `_test`/`-test` 结尾的隔离库；禁止使用开发或生产 `DATABASE_URL`。CI 使用 arc-flow 声明的 `postgres:16-alpine` service。

## 4. 审计规则

### 4.1 数据库和分层

- `INSERT`、`UPDATE`、`DELETE`、DDL、`.execute()` 和 `.exec()` 只能在 Repository、migration、测试或 seed。
- Handler 不得依赖 Repository；Service 不得包含 SQL；Repository 不得依赖 Handler/Service。
- 所有 DevRail 查询在 SQL 中按 organization、department 和 owner 过滤。
- 状态转换必须由 Service 定义并记录历史；客户端提交状态只能作为意图，不能直接覆盖服务端状态。
- migration 只追加；修改已应用 migration 是 blocker。

### 4.2 Agent 和执行安全

- 只有 Harness Supervisor 可以启动或终止 Codex 进程。
- `cwd`、环境变量、命令、网络和资源限制必须由后端策略解析，并记录生效快照。
- 生产、受保护分支、远端推送、删除和 Secret 读取默认拒绝或需要审批。
- app-server JSONL 解析失败、事件乱序、超时和异常退出必须产生明确失败状态，不能静默继续。
- 事件和日志必须脱敏；不得将完整命令、环境变量、模型隐藏推理或凭据写入推送。

### 4.3 推送可靠性和隐私

- 业务事件、站内通知和 outbox 必须同事务提交。
- delivery 以 `event_id + recipient_id + channel` 唯一；worker 必须可重入并支持租约到期恢复。
- 供应商 5xx/超时进入退避重试，404/410 等永久错误使设备失效。
- Push payload 只能有通知 ID、事件类型、脱敏摘要和深链接；打开深链接必须重新授权。
- 推送权限、设备、偏好、投递状态和错误需要审计；推送失败不能影响站内通知。

### 4.4 前端和契约

- Core 不得依赖 feature；组件不得直接注入 `HttpClient`，业务 API 经 Service。
- 每个异步页面必须有加载、成功、空数据和失败状态；移动端不得产生页面级横向溢出。
- OpenAPI 生成文件只能由 `npm run generate:api:all` 更新；契约漂移阻止合并。
- 路由、导航、按钮共享权限常量；后端必须重复校验权限。

## 5. CI 必检作业

`.github/workflows/ci.yml` 必须保留以下 required checks：

| Job | 检查 |
| --- | --- |
| `Quality gate` | workflow 配置、secret scan、审计和 arc-flow 工作流组件 |
| `Backend verification` | Rust fmt、Clippy、编译、隔离 PostgreSQL 测试 |
| `Frontend verification` | Doctor、生产依赖审计、ESLint、Prettier、Vitest、Playwright、全栈 smoke、Angular build |
| `Dependency review` | 公共仓库 PR 的依赖风险，moderate 及以上阻断 |
| `CodeQL` | JavaScript/TypeScript 和 Rust 静态分析 |
| `Security` | RustSec、cargo-deny、Trivy HIGH/CRITICAL、SPDX SBOM |

所有 job 使用固定版本 action、最小 `contents: read` 权限、超时和 artifact 保留期。CI 中不得使用真实数据库、生产密钥、真实推送私钥或外部仓库 token。

## 6. PR 审核清单

- [ ] 需求、架构、数据模型和权限影响已写入文档。
- [ ] 变更路径与 `cargo flow scope` 组件一致。
- [ ] 新表具备组织/部门/所有者边界；查询没有先全量读取再过滤。
- [ ] Agent 工具、命令、工作区、网络和资源策略经过审查。
- [ ] 审批不可伪造、不可重复处理、过期后不能继续执行。
- [ ] 推送 payload 无敏感数据；outbox、重试、幂等和设备撤销有测试。
- [ ] API 权限测试覆盖允许和 403；移动端页面覆盖关键操作。
- [ ] migration 可前滚、可回滚策略明确，旧数据保留策略明确。
- [ ] 本地 `cargo flow verify --all` 和 CI 全部成功。

## 7. 例外和升级

临时例外必须在 PR 中记录规则、原因、影响范围、到期日期、替代控制和责任人；不能通过删除 audit rule、降低 CI 阈值或忽略报告来“解决”失败。任何安全 blocker 只能由项目维护者在审计记录中明确批准，并创建后续修复任务。

框架升级使用 `scripts/upgrade-framework.sh --check` 和 `scripts/upgrade-framework.sh`，升级后重新运行全量门禁。DevRail 业务代码不得直接修改 arc-admin 框架源仓库。

## 8. 报告和证据

`codex-audit-pipeline/.codex/reports/` 只保存本地/CI 生成产物，不提交 Git。PR 描述应引用关键 job、commit SHA、迁移名称和测试命令；生产发布还需保存 SBOM、镜像摘要、审计归档校验和恢复演练记录。
