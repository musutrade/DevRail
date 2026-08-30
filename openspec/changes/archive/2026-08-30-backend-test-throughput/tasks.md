## 1. Baseline and test inventory

- [x] 1.1 记录 `cargo flow verify --components backend`、裸 `cargo test` 和 API 流程分段耗时，输出包含测试二进制、数据库初始化、Argon2 与测试用例耗时的基线报告
- [x] 1.2 清点所有 PostgreSQL、环境变量、固定路径、端口和全局锁依赖，标注可并发、必须串行和需要重构的测试组；通过静态扫描和随机顺序运行验证清单完整

## 2. Reusable database fixtures

- [x] 2.1 实现进程内共享的迁移初始化与测试连接配置：迁移仅执行一次，未隔离测试继续独立创建并关闭 `PgPool`，避免无效 migration 检查；通过初始化计数和现有后端测试验证
- [x] 2.2 实现测试级 schema 隔离，覆盖 search path、迁移、清理和连接池归还；通过两个并发 fixture 互不读取/修改数据的集成测试验证
- [x] 2.3 将 `DATABASE_TEST_LOCK` 限制到确需全局数据库事实的测试（工作流 UUID fixture 的 4 线程试验发现仍受公共状态影响，已恢复锁），并为其余测试提供明确的并发夹具入口；通过 `cargo test -- --test-threads=4` 和重复运行验证无竞态

## 3. Split slow integration flow

- [x] 3.1 将 `backend/tests/api_flow.rs` 拆分为认证/MFA、权限与组织、DevRail 资源/审计等独立测试边界；保持原断言和中文错误契约，验证每个测试可单独运行
- [x] 3.2 消除拆分测试之间的隐式用户、角色、MFA secret 和数据库状态依赖，所有 fixture 使用唯一标识并支持并发清理；通过随机测试顺序和至少两次重复运行验证
- [x] 3.3 保留迁移、全局审计触发器和跨测试数据库约束的专门串行覆盖，并在测试报告中标注串行原因；验证这些场景仍被默认 backend 门禁执行

## 4. Controlled parallel execution

- [x] 4.1 为后端测试增加可配置并发上限，默认值与 CI CPU/数据库连接池匹配，并在日志和报告中记录实际线程数；通过 1、2、4 线程基准比较稳定性和耗时
- [x] 4.2 评估并接入 `cargo nextest` 或等价测试执行器，仅在跨进程隔离已证明安全后启用；通过失败重试、JUnit/现有 parser 兼容和完整结果计数验证（评估结论：公共 schema 测试尚未具备跨进程隔离，保持 Rust harness；现有 parser 与 156 个结果计数不变）
- [x] 4.3 为 `cargo flow` 增加后端测试超时、并发和隔离模式的配置校验，确保共享报告文件、测试服务和数据库不会被无序并发；通过 arc-flow 单元测试和故障注入验证

## 5. CI build and workflow caching

- [x] 5.1 为 GitHub Actions Rust toolchain 配置基于 lockfile/toolchain 的依赖和目标缓存，并验证缓存命中与失效均执行完整质量门（PR #88 首轮 miss/save、rerun full hit；lockfile/toolchain hash 作为失效边界，完整质量门两轮均成功）
- [x] 5.2 为后端和前端 Docker 构建配置 BuildKit/GHA layer cache，保留 Trivy、SBOM 和依赖策略检查；通过 security workflow 成功运行和缓存键变更验证（PR #88 两轮 security workflow 成功；两套镜像均完成 Trivy/SBOM，Dockerfile/build context 内容键与独立 scope 作为失效边界）
- [x] 5.3 评估 `backend.compile` 与 Clippy/test 的重复编译成本，在不削弱 required step 覆盖的前提下合并或复用构建产物；通过 `cargo flow verify --all` 和报告步骤计数验证

## 6. Verification and rollout

- [x] 6.1 运行后端完整测试、随机顺序、线程数 1/2/4 对比和至少一次重复压力套件，记录失败率、数据库锁等待、CPU/内存和总耗时（6 个独立、正确环境的 1/2/4 轮次均通过；另有 1 次批处理测量因共享报告覆盖而无法复现，锁等待时长未由 harness 暴露，均已记录）
- [x] 6.2 运行 `TMPDIR=/tmp cargo flow verify --all`、`openspec validate --all --strict`、secret scan、架构审计和前端回归，确认输出仍包含 `TEST_SUMMARY: PASS`
- [x] 6.3 更新测试运行手册和实现状态，记录基线、目标、并发默认值、隔离回滚开关和已知限制；仅在总耗时和稳定性达到验收阈值后勾选完成（2026-08-30 修复 migration advisory lock 竞争后，1/2/4 线程各一轮均以 157 个结果通过；全量 backend/frontend/workflow 门禁与 OpenSpec 严格校验通过，详见 `docs/verification/backend-test-throughput-2026-08-29.md` 和 ADR-0007）
