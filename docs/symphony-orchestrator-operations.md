# Symphony Orchestrator 运行手册

更新日期：2026-08-26

本文覆盖 OpenSpec change `symphony-orchestrator-reconciliation` 已实现的 DevRail DB tracker、调度控制循环和 Harness 恢复能力。外部 tracker、`WORKFLOW.md`、DAG 和独立 workspace manager 不在本手册范围内。

## 启动配置

所有配置在后端启动时校验。非法值会阻止服务启动，不会静默回退。

| 变量 | 默认值 | 有效范围/关系 | 作用 |
| --- | ---: | --- | --- |
| `DEVRAIL_HARNESS_MAX_CONCURRENCY` | 2 | 正整数 | 单副本 Harness 并发槽位 |
| `DEVRAIL_RUN_MAX_DURATION_SECS` | 3600 | 正整数 | 单次 run 总时限 |
| `DEVRAIL_RUN_GRACEFUL_INTERRUPT_SECS` | 10 | 正整数 | 中断后强制杀进程前的等待时间 |
| `DEVRAIL_SCHEDULER_POLL_SECS` | 10 | 1–3600 | 控制循环间隔 |
| `DEVRAIL_SCHEDULER_CLAIM_LEASE_SECS` | 60 | 2–86400，且大于轮询间隔 | 待启动任务的 claim 租约 |
| `DEVRAIL_SCHEDULER_RETRY_BASE_SECS` | 1 | 1–86400 | 指数退避基础延迟 |
| `DEVRAIL_SCHEDULER_RETRY_MAX_SECS` | 300 | 1–604800，且不小于基础延迟 | 自动重试最大延迟 |
| `DEVRAIL_SCHEDULER_RETRY_JITTER_PERCENT` | 20 | 0–100 | OS 随机源生成的正向抖动比例 |
| `DEVRAIL_SCHEDULER_STALL_SECS` | 120 | 2–86400，且大于轮询间隔 | 无 stdout/stderr 有效活动的 stall 阈值 |
| `DEVRAIL_SCHEDULER_PRIORITY_AGING_SECS` | 3600 | 60–604800 | queued 任务每等待一个周期提升一级有效优先级 |

claim 租约只保护“已领取、尚未启动”的窗口。run 创建后并发额度由 Supervisor reservation 持有，数据库事实由 run 状态、心跳和事件游标维护。实际调度/重试策略会写入 run policy 快照，运行中修改环境变量不会改变现有 run。

## 指标与建议告警

| 指标 | 含义 | 建议告警 |
| --- | --- | --- |
| `devrail_scheduler_queue_depth` | 当前 queued 任务数 | 15 分钟持续增长且无 completed run |
| `devrail_scheduler_dispatch_total{outcome}` | started、capacity、stale_claim、failed 等固定低基数结果 | `failed`/`permanent_failure` 5 分钟突增 |
| `devrail_scheduler_dispatch_latency_seconds` | 从任务创建到实际派发的等待时间 | P95 超过业务 SLO |
| `devrail_scheduler_claim_conflict_total` | 旧租约/旧 worker 被拒绝次数 | 10 分钟持续增长，检查时钟与副本抖动 |
| `devrail_scheduler_retry_total` | 自动 retry 次数 | 与供应商或数据库错误同时突增 |
| `devrail_scheduler_stall_total` | stall 和进程缺失修正数 | 任意持续增长均应调查 Harness 日志 |
| `devrail_run_active` | starting/active/awaiting approval 数量 | 长期等于并发上限且队列增长 |
| `devrail_run_reconciliation_total{outcome}` | claim、stale、取消、环境失效和重启修正 | `stale_run`、`retry_exhausted` 出现即告警 |

指标标签由代码白名单归一化，未知值统一记为 `other`，不得把 task、run、组织或错误文本写入标签。

## 故障处理

### 队列持续堆积

1. 查看 queue depth、dispatch latency 和 `dispatch_total{outcome="capacity"}`。
2. 确认 Harness 并发上限、宿主机资源和运行平均时长。
3. 检查任务详情的下一次重试时间、最后错误和环境 enabled 状态。
4. 不直接修改数据库 claim；修复容量或环境后，任务会在下一轮自动参与调度。

### claim 冲突或旧 worker 写入

1. 检查多个后端副本的系统时钟与数据库连接延迟。
2. 确认 claim lease 大于 poll interval，避免配置导致正常 tick 被误判过期。
3. 旧 token 的续租/状态写入会返回 false；不要人工复用 token。

### Harness 无事件或断流

- 无事件达到 stall 阈值：Supervisor 杀进程、记录 `stall`、清理 run，并在 attempt 未耗尽时带退避重新排队。
- stdout/传输 EOF：最多在同一 run/attempt 恢复两次；只有已持久化 thread 时才执行 `thread/resume`。没有 thread 时进入下一 attempt，避免创建不可追踪的重复 Agent。
- 浏览器 SSE 断开：前端按 cursor/Last-Event-ID 补拉，不会触发 Harness 恢复或改变 task/run 事实。

### 后端进程重启

1. 启动阶段扫描 starting/active run；有 thread 的 run 使用原 thread/turn 恢复。
2. 没有 thread 的 run 原子标记 `supervisor_restart` 失败，并创建一次站内通知/outbox 和 System Actor 审计。
3. 周期 reconciliation 会处理数据库 active 但 Supervisor 无进程的 stale run，避免无限期 active。
4. 通过 run 详情核对 `exitReason`、`traceId`、`recoverySuggestion` 和 `cleanupStatus`。

## 发布与回滚

迁移 `20260826030000_add_symphony_scheduler_reliability.sql` 是添加式迁移：历史 run 按创建顺序回填 attempt，审计/事件不重写。旧版本不传 attempt 时，兼容触发器会在插入前分配下一 attempt；当前版本显式传入正 attempt，不经过兼容分配。

发布顺序：

1. 备份 PostgreSQL 并执行 migration job。
2. 滚动部署新后端，确认启动配置校验通过。
3. 观察 queue、claim conflict、stall 和 reconciliation 指标。
4. 再部署前端生成契约。

应用回滚时先停止新 worker，再回滚后端镜像；保留新增列、索引、触发器、审计和通知，不执行 DROP。旧版本可依赖兼容触发器继续创建 run。数据库结构回退属于破坏性操作，必须另立迁移并经过数据备份、停机和专项评审；常规故障优先向前修复。

## 验收命令

```bash
export PATH="/home/gem/.npm-global/bin:$PATH"
openspec validate symphony-orchestrator-reconciliation --strict
cargo flow scope
cargo flow verify --all
```

真实 PostgreSQL 故障测试使用隔离的 `TEST_DATABASE_URL`，不得连接生产数据库。完整证据矩阵见 [Symphony 专项需求](symphony-devrail-requirements.md#124-p0-调度可靠性证据矩阵2026-08-26)。
