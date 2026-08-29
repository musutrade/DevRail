## Why

质量门禁、CI 或外部审查失败后，系统目前能够保留失败结论或创建 continuation，但尚不能以独立、受策略约束的修复 run 汇总诊断、执行最小修复、重新验证并在限额后转人工。缺少这一闭环会迫使操作者在失败证据、权限边界和重试历史之间手工拼接，且容易把修复与原 run、普通 retry 或 continuation 混淆。

## What Changes

- 引入受控修复 run：仅由可信的质量门禁、CI 或审查事件在固化策略允许时创建，使用独立 run/attempt、稳定幂等身份、完整父子谱系和单独审计链。
- 在持久化前汇总脱敏诊断上下文，包含受影响门禁、结构化错误、受限日志引用、changeset 和环境摘要；禁止写入凭据、完整命令输出、绝对路径或完整请求头。
- 将低风险自动建议、需审批的逻辑/依赖/远端写入操作和禁止操作严格分层；修复 run 必须继承原任务的数据范围、workspace/网络/命令策略与安全限制。
- 使修复 run 使用新隔离 workspace、重新执行受影响门禁，并将原始失败、修复结果、审查/审批和最终人工结论关联起来。
- 增加修复次数、成本/容量和 Hook 熔断协同限制：达到固化上限、缺少新证据、策略拒绝或 Hook 熔断时停止自动创建并转人工，不覆盖来源 run 的终态。
- 提供查询、取消和人工处理所需的受权限保护 API/UI 契约、脱敏事件、outbox 通知、低基数指标与运行手册/验收证据。

## Capabilities

### New Capabilities

- `controlled-repair-runs`: 失败诊断、受策略约束的修复 run、权限/审批边界、门禁重跑、谱系、人工交接与可观测性契约。

### Modified Capabilities

- `symphony-orchestrator`: 调度器需要以稳定幂等键领取、派发、恢复、限额和终结修复 run，并与 Hook 熔断和普通 retry 保持分离。
- `task-tracker`: 任务状态投影和不可变历史需要表达来源失败、待修复、修复执行、人工交接及最终结论，且不改写来源 run。
- `task-workspace-manager`: 修复 run 需要从受控失败证据和不可变快照创建独立 workspace，并在取消、失败和重启时安全清理。

## Impact

- 后端：新增 repair 数据迁移、模型、Repository、Service、Handler、权限、审计/outbox 事务，以及 Scheduler、Harness Supervisor、质量门禁和 workspace 生命周期编排。
- 前端与契约：扩展 Rust DTO/utoipa、OpenAPI、Angular 生成客户端、任务/run 详情与中文人工处理界面。
- 运维与验证：增加低基数指标、告警、回滚策略、真实 PostgreSQL/假 app-server/质量门禁/供应商回调测试，以及 MVP 验收证据矩阵记录。
- 安全：不新增 Agent 直连数据库、浏览器直连 Harness 或 Handler/Service 中 SQL 写入的例外；所有数据范围和脱敏约束保持强制执行。
