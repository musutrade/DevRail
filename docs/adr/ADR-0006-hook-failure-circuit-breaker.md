# ADR-0006：重复 Hook 失败熔断与人工介入

- 状态：已接受
- 日期：2026-08-28
- 决策人：DevRail 项目维护者
- 关联架构：[调度器的稳定幂等与对账语义](ADR-0002-scheduler-idempotency-and-reconciliation.md)
- 关联规范：[Symphony Orchestrator](../../openspec/specs/symphony-orchestrator/spec.md)

## 背景

`before_run`、`after_run` 和 `on_failure` Hook 属于 Agent 启动及终态处理的强制门禁。Hook 本身持续失败时，普通 run retry 机制可能反复调度同一错误，既不能修复配置，也会持续消耗执行额度。重复终态事件还可能把一次失败重复计入熔断计数。

## 决策

1. 每个任务持久化最近一次 Hook 错误的脱敏 fingerprint 和连续失败次数。fingerprint 由 Hook 阶段与安全错误摘要计算，不保存完整命令、输出、凭据或请求头；错误 fingerprint 变化时计数重置为 1。
2. 相同 fingerprint 连续失败达到 5 次时打开熔断器：当前 run 以 `hook_failure_circuit_open` 失败，任务保持 `failed`，不再自动启动 Agent，并返回“已停止自动运行，请人工介入”的中文提示。
3. 第 1 至第 4 次失败仅在自动调度场景重新排队；手工触发的 Hook 失败不自动重试。普通 scheduler attempt 上限不得提前终止仍处于 Hook 熔断阈值内的任务。
4. Hook 成功完成后清零计数。重复终态事件必须保持幂等，不能重复增加 Hook 失败次数。

## 取舍与后果

- 自动运行有明确上限，配置错误会尽快暴露给人工处理，避免无限重试。
- 任务模型和调度查询增加两个字段及一次 additive migration；旧任务默认计数为 0，可滚动部署。
- fingerprint 只用于相等性判断和审计关联，不向前端暴露原始错误内容。

## 迁移与回滚

使用 additive migration `20260908100000_add_hook_failure_circuit_breaker.sql` 增加可空 fingerprint 和默认 0 的计数字段。回滚时先关闭自动调度或将 Hook 修复后人工重置任务；保留字段和历史终态，不执行破坏性 down migration。重新部署旧版本前必须确认旧版本能读取新增列并不会覆盖任务状态。

## 验收条件

- 相同 Hook 错误第 1 至第 4 次可按策略重试，第 5 次稳定进入 `hook_failure_circuit_open`，且未启动 Agent。
- 不同 fingerprint 或 Hook 成功会重置连续计数。
- 普通 scheduler attempt 已达到上限时，Hook 计数 1 至 4 的任务仍可完成剩余 Hook 重试；非 Hook 错误仍遵守普通上限。
- 重复终态事件不会重复累计、通知或清理副作用。
- PostgreSQL migration、Repository、Scheduler、Supervisor 测试及 `cargo flow verify --components backend` 通过。
