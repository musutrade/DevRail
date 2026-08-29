# 后端测试吞吐基线

日期：2026-08-29

## 采集条件

- 命令从仓库根目录执行，临时目录固定为 `TMPDIR=/tmp`。
- 集成测试只使用 `cargo flow` 创建的临时 PostgreSQL 容器，或名称以 `_test` / `-test` 结尾的显式测试库。
- 生产 `DATABASE_URL` 不参与任何测试；不能通过降低 Argon2 参数或跳过断言缩短测试时间。

## 变更前基线

| 命令或测试二进制 | 结果 | 耗时 | 观察 |
| --- | --- | ---: | --- |
| `cargo flow verify --components backend` | 通过 | 399.35 秒 | secret scan 20.6 秒、Clippy 29.2 秒、后端测试 329.26 秒。 |
| `cargo test --manifest-path backend/Cargo.toml -- --nocapture` | 通过 | 约 208 秒 | 库测试约 59.79 秒，`api_flow` 单测试约 148.37 秒，契约测试可忽略。 |
| 库测试二进制 | 通过 | 约 59.79 秒 | 143 个库测试；数据库测试被进程内全局锁串行化。 |
| `tests/api_flow.rs` | 通过 | 148.37 秒 | 单个 2292 行串联流程，覆盖认证、MFA、权限、组织、DevRail 和审计。 |

## 当前验证结果

| 命令或测试二进制 | 结果 | 耗时 | 观察 |
| --- | --- | ---: | --- |
| `TMPDIR=/tmp cargo flow step backend.tests` | 通过 | 373.73 秒 | 153 个测试结果；包含 secret scan 20.61 秒、架构审计 1.54 秒和测试冷编译 2 分钟。 |
| `TMPDIR=/tmp cargo flow step backend.tests` | 通过 | 406.66 秒 | 154 个测试结果；覆盖最近迁移的 continuation schema fixture。 |
| `TEST_DATABASE_URL=... RUST_TEST_THREADS=4 cargo test -- --test-threads=4` | 通过 | 约 220 秒 | 154 个结果；库测试 53.98 秒、API 流 147.49 秒、schema fixture 8.96 秒，公共 schema 锁保持稳定。 |
| 一次性 PostgreSQL：`cargo test -- --test-threads=1` | 通过 | 346.11 秒 | 144 个库测试 172.11 秒、3 个 API 流 162.88 秒、schema fixture 11.12 秒。 |
| 一次性 PostgreSQL：`cargo test -- --test-threads=2` | 通过 | 190.57 秒 | 144 个库测试 89.85 秒、3 个 API 流 91.06 秒、schema fixture 9.66 秒。 |
| 一次性 PostgreSQL：`cargo test -- --test-threads=4` | 通过 | 182.60 秒 | 144 个库测试 84.05 秒、3 个 API 流 88.75 秒、schema fixture 9.80 秒。 |
| 库测试二进制 | 通过 | 53.98 秒 | 144/144 通过，新增迁移初始化计数测试。 |
| `tests/api_flow.rs` 拆分后 | 通过 | 98.57 秒（二进制） | 3 个独立测试并发执行；认证/MFA、组织权限、DevRail/审计分别建立 schema fixture。 |
| `TMPDIR=/tmp cargo flow step backend.tests`（默认线程上限） | 通过 | 374.21 秒 | 报告显示 `backend tests (threads: 4)`、`DEVRAIL_TEST_THREADS=4`，156 个结果全部通过。 |
| `TMPDIR=/tmp ARC_FLOW_BACKEND_TEST_THREADS=1 cargo flow step backend.tests`（重复轮） | 通过 | 471.34 秒 | 156 个结果；受控 PostgreSQL 服务，`TEST_SUMMARY: PASS`。 |
| `TMPDIR=/tmp ARC_FLOW_BACKEND_TEST_THREADS=2 cargo flow step backend.tests`（重复轮） | 通过 | 186.47 秒 | 156 个结果；库/API/schema 分段均通过，`TEST_SUMMARY: PASS`。 |
| `TMPDIR=/tmp ARC_FLOW_BACKEND_TEST_THREADS=4 cargo flow step backend.tests`（重复轮） | 通过 | 175.86 秒 | 156 个结果；默认并发上限第二次通过，`TEST_SUMMARY: PASS`。 |

共享迁移初始化减少了重复迁移检查，但不会消除 API 集成流中的真实 Argon2/MFA 成本。冷编译、依赖缓存和 API 流拆分需要分别衡量，不能把它们混为同一项收益。

`api_flow` 中的多次密码登录、step-up、模块解锁、恢复码哈希会执行生产 Argon2；TOTP 重放保护在计数变化时最多按 100ms 重试 50 次。以上为真实测试成本，不能用更弱的生产安全参数替代。

## 数据库初始化测量

原始库测试夹具至少有 7 处各自读取 `TEST_DATABASE_URL`、创建连接池并调用迁移。一次 `cargo test` 内迁移结果对所有测试相同，重复调用只产生额外连接、迁移锁检查和 `_sqlx_migrations` 查询。

本变更引入 `db::test_pool()`：

- 首次调用创建短生命周期初始化连接，迁移只执行一次并立即归还连接；
- 后续调用仅创建独立测试池，保留原有测试的事务与连接生命周期；
- `TEST_MIGRATION_INITIALIZATIONS` 计数和测试保证同一测试进程不会重复初始化迁移；
- `db::test_schema_pool()` 已覆盖 schema 隔离；API 流和 16 个数据库场景已迁移到隔离 fixture。公共 schema 仅保留 13 个显式 `DATABASE_TEST_LOCK` 锁点（约 24 个测试），其余数据库测试从明确的 schema fixture 入口并发执行。

曾实验全套数据库测试共用单个 `PgPool`。由于现有测试含跨 `await` 的长事务及并发子任务，即使提高连接上限也会出现 `PoolTimedOut`。该方案已撤回，不作为性能结果；共享迁移初始化保留，连接池复用将在 schema 隔离完成后按隔离域重新评估。

## 验收目标

1. 先保证测试隔离与稳定性，再将默认后端测试线程限制为 4。
2. 将 API 流拆为认证/MFA、权限与组织、DevRail/审计三个可独立运行的测试边界。
3. 对 1、2、4 线程各运行两次，并记录成功率、总耗时、数据库锁等待、CPU 和峰值内存。
4. 默认门禁必须保持 `TEST_SUMMARY: PASS`；任何缓存命中仍执行完整 secret scan、审计、lint、编译和测试。

后端测试进程通过 `DEVRAIL_TEST_THREADS` 报告实际线程数。`cargo flow` 默认传递 `--test-threads=4`，可用 `ARC_FLOW_BACKEND_TEST_THREADS=1|2|4` 覆盖；该配置不会绕过公共 schema 测试锁。

## Schema fixture 验证

- `backend/tests/db_schema.rs` 通过两个并发 `db::test_schema_pool()` fixture 验证唯一 schema、连接级 `search_path`、各自迁移和同名表数据互不可见。
- fixture 清理先关闭业务连接池，再由管理连接池执行 `DROP SCHEMA ... CASCADE`；迁移或清理失败都会关闭已创建的连接并返回错误。
- 2026-08-29 `TMPDIR=/tmp cargo flow step backend.tests`：156 个结果通过，包含 schema fixture 测试；测试服务结束后未发现残留 `devrail_test_*` schema。

## 2.3 并发验证

- 静态扫描确认公共 schema 仅有 13 个 `DATABASE_TEST_LOCK.lock()` 锁点；它们覆盖迁移种子、全局审计触发器、共享权限事实和跨测试状态恢复，不能仅因唯一 UUID 而解除锁。
- 每个线程档位均使用新的 PostgreSQL 容器，避免固定幂等键或 migration/seed 状态跨基准污染；1、2、4 线程的纯测试二进制耗时分别为 346.11、190.57、182.60 秒，全部通过。
- 4 线程相对 1 线程缩短约 47%，相对 2 线程缩短约 4%。Argon2 与公共 schema 串行事实已接近瓶颈，因此默认上限保持 4，不继续提高。

## 4.1 受控并发

- `.arc-flow/flow.toml` 中 `backend.tests` 默认 `test_threads = 4`，可由 `ARC_FLOW_BACKEND_TEST_THREADS` 覆盖；arc-flow 在 `cargo test --` 后追加实际 `--test-threads` 参数。
- 报告步骤标签、`DEVRAIL_TEST_THREADS` 和 `test_result.md` 都记录实际并发上限。2026-08-29 默认门禁以 4 线程通过，156 个结果、`TEST_SUMMARY: PASS`。

## 4.2 跨进程执行器评估

- 本机已安装 `cargo-nextest 0.9.143`，但本变更不启用它。nextest 会让多个测试二进制跨进程并发，而 `DATABASE_TEST_LOCK` 仅能保护单一 Rust 测试进程。
- 仍有公共 schema 的迁移、seed、审计触发器和恢复状态测试；这些场景与另一个测试进程共享 `TEST_DATABASE_URL` 时没有跨进程锁或每 worker 独立数据库保证。启用 nextest 会绕开已验证的串行边界。
- 因此不执行 nextest 的失败重试、JUnit 或结果解析切换验证，现有 Rust parser、`TEST_SUMMARY` 和 156 个结果计数保持不变。待公共 schema 测试迁移至 schema 或每 worker 独立数据库，并证明跨进程清理安全后，再单独提出 change 评估 nextest。

## 4.3 编排配置门禁

- `test_threads` 仅接受 1-64，配置并发时必须声明 `test_isolation`；`shared` 隔离与大于 1 的线程数会被拒绝。arc-flow 单元测试覆盖超出上限、缺失隔离声明和共享状态并发三种故障注入。
- 测试步骤仍由 arc-flow 顺序执行，报告文件和测试服务不会被无序并发访问。并发仅由单个 `cargo test -- --test-threads=N` 进程内部进行；`RUST_TEST_TIMEOUT` 继续限制该步骤最长 600 秒。

## 5. CI 缓存与重复编译

- CI 的 Rust 工具链缓存以各自的 `Cargo.lock` 和 `rust-toolchain.toml` 生成键，分别覆盖 backend 与 arc-flow target 目录；缓存命中不改变 `cargo flow verify` 的 secret scan、审计、lint 或测试步骤。锁文件或 toolchain 变化会使对应缓存失效并重新构建。
- security workflow 使用 Buildx 的 `type=gha` 缓存，并按 backend/front-end 镜像拆分 scope。镜像仍以 `load: true` 导入本地 Docker，随后无条件执行 Trivy；构建成功后才生成 SBOM。首次远端 workflow 运行须确认 cache restore/save 日志，命中与 lockfile/Dockerfile 变更后的失效均须通过同一质量门。
- 后端质量门移除了独立 `backend.compile` 步骤。`backend.clippy --all-targets --all-features` 已提供更广的静态编译覆盖，`backend.tests` 仍会编译并执行全部测试目标；验证报告的后端步骤数因此减少一项，覆盖不减少。
- 2026-08-29 PR #88 首轮运行 `CI`、`Supply chain security` 和 `arc-flow platform` 均成功。Rust 缓存首轮明确报告 `No cache found` 后 `Saving cache`；同提交 rerun 中 backend 依赖策略 job 对 `v1-backend-<Cargo.lock/toolchain hash>` 报告 full cache hit 和 `Cache up-to-date`。CI 后端/前端验证、arc-flow Ubuntu/Windows/benchmark、Rust 依赖策略及两套镜像的 Trivy/SBOM 均在两轮成功。
- Rust 失效边界由 workflow 中的 `hashFiles('backend/Cargo.lock', 'rust-toolchain.toml')` 与对应 arc-flow lockfile 输入决定；BuildKit GHA cache 由 Dockerfile 和 build context 内容键决定，backend/front-end 使用独立 scope。首轮 miss/save 与 rerun hit 证明缓存不会替代质量门，依赖、toolchain 或镜像构建输入变更会创建新键并继续执行完整门禁。

## API 流拆分验证

- `authentication_and_mfa_flow` 单独运行通过，1/1，79.35 秒。
- `organization_and_permissions_flow` 单独运行通过，1/1，44.39 秒。
- `devrail_resources_and_audit_flow` 单独运行通过，1/1，91.24 秒；该场景显式创建密码变更和登出事件，避免依赖认证测试产生的审计记录。
- nightly 随机顺序 seed `20260829` 和 `20260830` 各运行一次，均为 3/3 通过；schema 清理由各测试独立完成。

## 6.1 重复压力与资源记录

- 已记录的 1/2/4 线程基线轮与各一轮受控重复均通过 156 个结果；线程 1 的重复轮为 471.34 秒，线程 2 为 186.47 秒，线程 4 为 175.86 秒。另有一次 `cargo flow` 的 2 线程批处理轮在 106.61 秒失败；其底层共享日志在紧接着启动 4 线程轮时被覆盖，无法恢复具体测试名。单独受控重跑 2 线程后通过，因此当前已观测的后端并发尝试为 6 通过、1 未复现失败，成功率 85.7%。
- 一次未通过 `cargo flow` 的手工 `cargo test` 重跑遗漏 `TMPDIR=/tmp`，导致 6 个临时工作区测试写入受限目录；该环境错误不计入并发稳定性统计，已停止并清理容器。
- 1/2/4 线程基线轮分别为 346.11、190.57、182.60 秒；合并重复轮后，4 线程仍是稳定默认值。测试日志只输出受保护的公共 schema 事实标记，未出现数据库锁等待或 `PoolTimedOut` 错误；当前 harness 未暴露 PostgreSQL 锁等待时长，不能伪造精确数值。
- `TMPDIR=/tmp /usr/bin/time -v cargo flow verify --all` 通过，wall time 306.50 秒，CPU 101%，峰值 RSS 1,095,084 KB（约 1.04 GiB），无 swap；该数据覆盖后端、前端和 workflow 全量门禁。

## 3.3 串行事实标记

- 公共 schema 测试通过 `DATABASE_TEST_LOCK` 保持串行，覆盖迁移/seed、全局审计 append-only trigger、共享权限事实和跨测试恢复状态。
- 默认 `cargo flow step backend.tests` 报告显示 `156 result(s); serial database facts: public migration, seed, audit trigger, and recovery-state tests use DATABASE_TEST_LOCK`，且 `TEST_SUMMARY: PASS`。
- schema fixture 测试不依赖该锁，仍可在同一测试进程中并发清理。
