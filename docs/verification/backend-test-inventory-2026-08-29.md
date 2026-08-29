# 后端测试依赖清单

日期：2026-08-29。以下静态扫描范围为 `backend/src` 与 `backend/tests`，不包含 `target`。

## 统计

| 依赖 | 数量 | 当前结论 |
| --- | ---: | --- |
| `#[tokio::test]` | 52 | 纯内存测试可由 Rust 默认调度并发执行。 |
| `DATABASE_TEST_LOCK.lock()` | 13 个锁点，约 24 个测试 | 仍访问公共 PostgreSQL schema、迁移种子、全局审计/权限事实或跨测试恢复状态的测试；其余已迁移到 schema fixture。 |
| `TEST_DATABASE_URL` | 5 | 4 个生产测试入口及 1 个共享迁移夹具入口。 |
| `std::env::set_var` / `remove_var` | 1 | repair callback 测试；必须使用独立环境变量锁。 |
| 临时目录或固定 `/tmp` 路径 | 33 | 必须使用 UUID 或测试专属子目录，完成后清理。 |
| 固定端口/回环地址引用 | 7 | 主要是配置、懒连接和 HTTP origin fixture；没有测试绑定固定监听端口。 |

## 分组

| 分组 | 范围 | 并发状态 | 理由 |
| --- | --- | --- | --- |
| 纯单元测试 | access、config、models、workflow parser、telemetry 等 | 可并发 | 无 PostgreSQL、无进程级可变环境或固定工作区。 |
| 共享库数据库测试 | 尚未迁移的 `repositories/devrail*`、`services/devrail_*`、`workers/*` 锁定测试 | 暂时串行 | 读取迁移种子、重放 seed、验证全局 trigger，或显式清理其他测试遗留状态；锁的语义是公共 schema 全局事实保护。 |
| schema 隔离数据库测试 | `db::test_schema_pool()`、API 流、工作流/运行/工作区/Harness 场景及 `backend/tests/db_schema.rs` | 可并发 | 每个 fixture 使用唯一 schema 和连接池，连接建立时设置 `search_path`，结束时关闭连接并删除 schema。 |
| API 集成流 | `backend/tests/api_flow.rs` | 可并发 | 已拆为认证/MFA、组织权限、DevRail/审计三个测试；每个测试自建唯一 schema、Router、管理员和会话。 |
| 全局事实测试 | 迁移、`pg_trgm`、审计 append-only trigger、权限 seed | 必须保留串行覆盖 | PostgreSQL 扩展和全局 migration/trigger 事实不能依靠搜索路径隔离。 |
| 文件/环境测试 | workspace、Harness、repair callback | 受控后可并发 | 需要 UUID 临时目录；repair callback 另有环境变量锁。 |

## 随机顺序验证

Rust stable 1.97.1 不接受 `--shuffle`；随机顺序仅作为本地 nightly 诊断，不进入正式门禁。当前共享数据库仍由少量全局锁保护；工作流 UUID fixture 的 occurrence 断言竞态已通过迁移到 schema fixture 消除。随机顺序只用于发现未受保护的全局状态，不能作为提升线程数的依据。schema 隔离夹具完成后，应在一次性测试数据库上执行：

```bash
TMPDIR=/tmp TEST_DATABASE_URL=postgres://user:password@127.0.0.1:5432/arc_admin_test \
  cargo +nightly test --manifest-path backend/Cargo.toml -- \
  -Z unstable-options --shuffle --shuffle-seed 20260829 --test-threads=1
```

2026-08-29 已在新的 PostgreSQL 容器上以 nightly seed `20260829`、`20260830`、3 线程完成拆分 API 流随机顺序运行：两次均为 3/3 通过，耗时 131.24 秒和 89.04 秒。每个测试均自建唯一 schema，MFA secret 随会话保存，结束时关闭连接池并删除 schema；未发现跨测试用户、角色或 MFA 状态污染。每次运行必须使用新的临时测试数据库，且确认没有读取 `DATABASE_URL`。在 1/2/4 线程隔离验证均稳定前，禁止接入跨进程执行器或放开默认数据库测试并发。
