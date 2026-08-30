# ADR-0007：测试数据库隔离与并发门禁

- 状态：Accepted
- 日期：2026-08-30
- 决策人：DevRail 项目维护者
- 关联变更：[backend-test-throughput OpenSpec change](../../openspec/changes/backend-test-throughput/proposal.md)
- 关联运行手册：[后端测试吞吐基线](../verification/backend-test-throughput-2026-08-29.md)

## 背景

后端测试需要在单个进程内受控并发，但 PostgreSQL 夹具、Rust 测试 harness 和 CI 编排的失败边界必须清晰。数据库连接或迁移失败不能被误报为测试通过；共享 schema、报告文件和 GitHub Actions 缓存权限也不能因并发配置而扩大风险。

## 决策

1. `TEST_DATABASE_URL` 未配置时，数据库测试可以按既有约定跳过；一旦配置，连接、迁移、schema 创建或测试连接初始化失败必须让测试进程失败，不得缓存失败状态或静默返回。
2. 测试迁移初始化只缓存成功结果。失败的初始化允许后续调用重试，并保留原始错误上下文。
3. schema fixture 同时提供显式 `cleanup()` 和丢弃时的异步兜底清理。显式清理返回数据库错误；兜底清理失败必须写入可诊断的测试日志。
4. `test_threads` 只能用于带 Cargo 测试参数分隔符 `--` 的步骤，并插入到该分隔符之后。`test_isolation = "shared"` 必须显式声明 `test_threads = 1`。
5. 测试报告优先记录 Rust harness 命令行中的实际线程参数，其次才使用编排器注入的环境变量或 CPU 默认值。
6. BuildKit GitHub Actions 缓存只在镜像 job 获得 `actions: write`；其他 job 使用最小权限。

## 取舍与后果

- CI 配置了测试数据库但服务不可用时会明确失败，避免产生假绿；未配置数据库的本地非数据库测试仍保持可运行。
- schema fixture 的兜底清理依赖 Tokio runtime；正常路径仍应显式调用 `cleanup()`，以便把清理错误返回给测试。
- 严格的 Cargo 参数和 shared 模式校验会拒绝部分旧的自定义工作流配置，但错误会在加载阶段暴露，而不是在测试任务中间失败。
- 最小权限会要求需要写入 GHA cache 的 job 单独声明权限，降低 workflow 其他步骤的令牌暴露面。

## 回滚

回滚并发 rollout 时将后端测试的 `test_threads` 设为 `1`，保留 schema 隔离、失败可见性和最小权限约束；不执行破坏性数据库回滚，也不恢复共享全局连接池。

## 关联实施

本 ADR 与 `backend-test-throughput` OpenSpec change 同步验收。实现证据包括 `backend/src/db.rs` 的测试数据库夹具、`codex-audit-pipeline/tools/arc-flow` 的配置与线程编排测试，以及 Security workflow 权限检查。只有 `cargo flow verify --all`、OpenSpec 严格校验和对应测试全部通过后，才视为本 ADR 完成落地。
