## Why

DevRail 已有按优先级领取 queued 任务的第一阶段调度器，但多 worker、进程重启、网关断流和 app-server 异常仍可能造成重复 run、永久 active 任务或状态与实际进程不一致。现在补齐稳定幂等、对账和失败恢复，是把现有调度基础提升为可持续运行的 Symphony 控制循环的最小 P0 变更。

## What Changes

- 为任务执行建立稳定的 `task_id + attempt` 幂等语义；随机 claim 标识只作为租约实例标识。
- 引入明确的 Scheduler/System Actor，自动调度不再模拟普通用户会话。
- 将控制循环固定为 reconciliation、dispatch、终态回收和指标更新，并处理数据库、claim、Supervisor 子进程与 workspace 的不一致。
- 完善 PostgreSQL 原子领取、`SKIP LOCKED`、租约续期、过期回收和单任务活动 run 约束。
- 为可重试错误增加指数退避、抖动、最大 attempt、stall 检测和明确的失败原因/恢复建议。
- 区分 Agent 传输断流与浏览器 SSE 断开；恢复和补拉均保持幂等，不重复执行 Agent。
- 增加多 worker 竞争、进程重启、断流、超时、取消、子进程清理和终态幂等的 Rust/SQL 集成测试及调度指标。

## Capabilities

### New Capabilities

- `symphony-orchestrator`: 提供任务领取、稳定幂等、System Actor、reconciliation、重试退避、stall 检测和终态处理契约。

### Modified Capabilities

- 无。当前 OpenSpec 根目录尚无已发布的 capability spec；本变更创建第一份规范。

## Impact

- 后端：任务调度 worker、Harness Supervisor 接口、run/claim 状态处理、审计和指标。
- 数据库：必要的 attempt、actor、租约、心跳、重试和状态历史字段/约束及迁移。
- API/OpenAPI：任务状态、run attempt、重试/恢复原因和调度状态的只读展示或受控操作。
- 测试：Rust/SQL 集成测试、故障恢复测试、指标和脱敏审计断言。
- 文档：关联 ADR-0001、ADR-0002，并在实现后更新 `devrail-implementation-status.md`。
