# 开发指南

## 环境准备

工具链版本由仓库固定：Node 读取 `.node-version`，Rust 读取 `rust-toolchain.toml`。还需要 Git 和 Docker；若显式提供隔离的 `TEST_DATABASE_URL`，后端测试可以不使用 Docker。

```bash
cd frontend && npm ci && cd ..
[[ -f backend/.env ]] || cp backend/.env.example backend/.env
docker pull postgres:16-alpine
git config core.hooksPath codex-audit-pipeline/hooks
cargo flow doctor
```

从模板创建的业务项目应先运行 `scripts/init-project.sh`，它会生成 `backend/.env`、`observability/.env` 和 `deployment/.env.production`，并同步项目标识。直接开发框架源仓库时，上述命令仅在文件不存在时复制开发环境示例配置。运行后端前确认 `DATABASE_URL` 指向本地开发库；生产部署文件仍须填写域名、数据库密码、完整连接串和 MFA 密钥。`APP_ENV=production` 时会强制认证 Cookie 使用 `Secure` 与 `__Host-` 前缀，并要求设置明确的 `CORS_ALLOWED_ORIGINS`。前后端应部署在同一站点下，跨 origin 部署还必须使用 HTTPS 并允许凭据请求。

数据库迁移不会留下可登录的默认账号。首次部署先显式初始化管理员，密码至少 16 字符：

```bash
BOOTSTRAP_ADMIN_PASSWORD='replace-with-a-strong-password' \
  cargo run --manifest-path backend/Cargo.toml --bin bootstrap_admin
```

命令默认使用用户名 `admin` 和显示名 `Administrator`；可通过
`BOOTSTRAP_ADMIN_USERNAME`、`BOOTSTRAP_ADMIN_DISPLAY_NAME`、`BOOTSTRAP_ADMIN_EMAIL`
覆盖。密码只通过当前进程环境传入，不要写进 `.env`、命令脚本或提交记录。

## 日常开发

```bash
# 前端
cd frontend && npm start

# 后端
cd backend && cargo run
```

前端默认地址是 `http://localhost:4200`，开发代理会把 `/api/v1` 转发到
`http://127.0.0.1:8080`。部署时可在静态资源 `config.js` 中设置产品名称、项目标识、
API 基址和主题存储键，无需重新构建前端。后端端口以 `backend/.env` 的 `PORT` 为准。

- `GET /api/v1/healthz` 是进程存活检查，不访问数据库；
- `GET /api/v1/readyz` 是就绪检查，数据库不可用时返回 HTTP 503。

## 验证流程

```bash
# 查看并按未提交变更自动选择检查范围
cargo flow scope
cargo flow verify

# 合并或交付前必须全量执行
cargo flow verify --all
```

执行顺序固定为 secret scan、架构审计、格式/lint、编译、单元/集成测试、Playwright
桌面与移动端 E2E，以及前端生产构建。Rust CLI 为每一步记录耗时和独立日志；报告位于
`codex-audit-pipeline/.codex/reports/`，同时提供 JSON 和末行带 `TEST_SUMMARY` 的 Markdown。

涉及页面、样式或交互的变更还必须按 [UI 与 CSS 规范](ui-design-system.md) 在 Desktop Chrome
和 Pixel 7 视口复核。需要保存本地审计截图时执行：

```bash
cd frontend
VISUAL_REVIEW=1 npm run e2e -- --project=chromium --project=mobile-chromium
```

后端集成测试仅允许使用临时容器或显式的隔离测试库：

```bash
TEST_DATABASE_URL=postgres://user:password@127.0.0.1:5432/arc_admin_test \
cargo flow verify --components backend
```

测试库名称必须以 `_test` 或 `-test` 结尾，且不得与 `DATABASE_URL` 指向同一数据库。
默认只接受本机回环地址；使用已确认隔离的远程测试库时，还需显式设置
`ARC_FLOW_ALLOW_REMOTE_TEST_DATABASE=1`。

后端库测试当前只共享迁移初始化，不共享数据库连接池；仍依赖公共 schema 全局事实的测试由 `DATABASE_TEST_LOCK` 串行执行。需要并发数据库访问的测试使用 `db::test_schema_pool()`，它为每个 fixture 创建唯一 schema、设置 `search_path`、执行迁移并在 `cleanup()` 中关闭连接和删除 schema。后端门禁默认使用 4 个 Rust 测试线程，可通过 `ARC_FLOW_BACKEND_TEST_THREADS=1|2|4` 覆盖；该配置只改变 Rust 测试调度，不会解除公共 schema 锁。不要同时启动多个指向同一 `TEST_DATABASE_URL` 的公共 schema 测试进程。基线、依赖分组和后续验证命令见[后端测试吞吐基线](verification/backend-test-throughput-2026-08-29.md)与[后端测试依赖清单](verification/backend-test-inventory-2026-08-29.md)。

后端测试会输出 `DEVRAIL_TEST_THREADS=<数量>`，表示本次测试进程使用的线程配置。通过 `cargo flow` 时该值与实际传给 Rust harness 的 `--test-threads` 一致；公共 schema 测试仍由 `DATABASE_TEST_LOCK` 保护。

`cargo flow` 以顺序步骤写入同一报告目录和管理测试服务生命周期，不在步骤间做并发调度。`test_isolation = "shared"` 与大于 1 的 `test_threads` 会在配置加载时被拒绝；`test_threads` 必须介于 1 和 64，并且必须显式声明隔离模式。`RUST_TEST_TIMEOUT` 仍可覆盖后端测试的 600 秒上限。

后端门禁保留 `cargo fmt`、`cargo clippy --all-targets --all-features` 和完整 `cargo test`。不再单独执行冗余的 `cargo check`：Clippy 已覆盖其类型检查范围，测试步骤仍会编译并执行测试目标。

前端依赖的 CI 门禁只阻断生产依赖的 high/critical 漏洞：

```bash
cd frontend
npm audit --omit=dev --audit-level=high
```

## Git 与 GitHub

- HTTPS remote 不能包含用户名、token 或密码；优先使用 `gh auth login` 与 `gh auth setup-git` 管理凭据。
- 自动化默认只允许本地提交；只有仓库维护者在当前任务中明确授权时，才能推送非受保护分支并创建 PR。禁止直接推送 `main`。
- 在 GitHub 仓库设置中保护 `main`，要求 `Quality gate`、`Backend verification`、`Frontend verification` 通过并至少一人审核。
- 启用 secret scanning、push protection 和 Dependabot alerts。公共仓库会额外运行 `Dependency review`；私有仓库是否可用取决于 GitHub 计划。
- 禁止 force push 和直接删除受保护分支。

CI 使用与本地相同的 `cargo flow` 命令。Frontend verification job 会在干净 checkout 中生成仅供 runner 使用的 `backend/.env`、配置 Git hooks，并使用隔离的 PostgreSQL service 执行 `cargo flow doctor --strict --json`；Doctor 结果随其他报告以 Actions artifact 保留 14 天。任何 warning 都会阻断该 job。

## 框架升级

派生项目使用 `.arc-project.json` 记录框架版本。升级必须从检出目标正式标签的模板仓库执行，先运行 `--check`，再执行升级；命令会三方合并框架文件，并在成功写入后运行 `cargo flow doctor` 和 `cargo flow verify --all`。

详细的发布、升级和冲突处理流程见[框架版本与派生项目升级](framework-upgrades.md)。
