# ADR-0005：Continuation turn 生命周期与谱系

- 状态：已接受
- 日期：2026-08-27
- 决策人：DevRail 项目维护者
- 关联提案：[continuation-turns proposal](../../openspec/changes/archive/2026-08-28-continuation-turns/proposal.md)
- 行为规范：[continuation-turns](../../openspec/specs/continuation-turns/spec.md)、[symphony-orchestrator](../../openspec/specs/symphony-orchestrator/spec.md)、[task-tracker](../../openspec/specs/task-tracker/spec.md)、[task-workspace-manager](../../openspec/specs/task-workspace-manager/spec.md)
- 迁移计划：[continuation-turns design](../../openspec/changes/archive/2026-08-28-continuation-turns/design.md#migration-plan)

## 背景

DevRail 已有 run retry、同 attempt 传输恢复、受控 follow-up task 和终态 workspace cleanup，但这些机制都不能表达“在原执行结论之后，依据追加上下文或新证据继续同一目标”。复用其中任一机制都会改写来源终态、混淆 run/turn 谱系，或依赖本应被清理的 workspace。

## 决策

1. Continuation 是同一 `codex_thread_id` 上的新 turn，同时创建新的 child run。child run 保存来源 run、来源 turn、continuation request 和序号；来源 run 的状态、终态时间、turn 和 changeset 均不可修改。
2. 四类运行语义使用独立身份与幂等边界：传输恢复继续使用同 run、同 attempt；retry 属于失败 run 的新 attempt；continuation 属于同 task 的新 child run 与新 turn；follow-up 创建具有独立目标和依赖的新 task。
3. 来源 run 终态时，系统必须在 hook 和 workspace cleanup 前固化不可变 handoff。handoff 保存数据范围、快照摘要、仓库身份、固定提交和 changeset 摘要；缺少可验证 handoff 的历史 run 不允许 continuation，也不从残留目录推测输入。
4. Continuation child run 必须从 handoff 在受控根目录内重建新的隔离 workspace。不得复用已清理、`cleanup_failed` 或其他活动 run 的路径；来源与 child 的 hook、占用和 cleanup 使用各自稳定幂等键。
5. 请求创建与派发分为两个可恢复事务。创建事务写入请求事实、任务投影、历史、审计和 outbox；派发事务创建或复用 child run、绑定 workspace、标记已派发并将任务投影为运行中。Agent 只能在派发事务提交后启动。
6. 取消的线性化点是派发事务：取消先提交则阻止 Agent 启动并清理已准备的 child workspace；派发先提交则取消 API 返回已派发冲突，后续使用既有 run cancellation，不产生未关联进程。
7. continuation 策略固化在来源任务快照中，默认关闭；创建和派发前都校验触发类型、活动 run、次数、链深、输入大小与证据新鲜度。重复证据返回原请求，不重复占额或产生副作用。

## 取舍与后果

### 正面影响

- 来源 run 和审计历史保持不可变，每次追加执行都有独立成本、事件、workspace 和终态。
- 数据库请求账本、稳定 child run 身份和 handoff 使多 worker、取消竞态与进程重启可确定性对账。
- cleanup 不再与未来 continuation 冲突，历史 run 的能力边界可以明确说明而无需猜测性回填。
- retry、传输恢复、continuation 与 follow-up 可分别观测、限额和回滚。

### 代价与风险

- 新增请求、handoff、任务状态和 run 谱系会扩大迁移、滚动升级和客户端兼容面。
- workspace 准备位于数据库事务外，可能遗留未绑定目录，必须通过稳定 workspace key 和 reconciliation 清理。
- handoff 固化会增加终态 cleanup 延迟；失败只能阻止后续 continuation，不得改写来源 run 结论。
- 自动 gate/review 触发可能形成成本循环，必须使用默认关闭、固定上限、证据去重和低基数拒绝码。

## 拒绝的方案

- 不重开来源 run，也不修改其终态或终态时间。
- 不把 continuation 作为 retry attempt、断流恢复或 follow-up task 的别名。
- 不在 Handler 请求中同步执行 Git、workspace 或 app-server I/O。
- 不复用来源 workspace，不从 `cleanup_failed` 或历史残留目录推导 handoff。
- 不使用可移动分支头作为唯一重建证据，也不在日志、事件或通知中保存完整上下文和绝对路径。

## 迁移与回滚

落地迁移为 additive migration `20260907100000_add_continuation_turns.sql`：先增加可兼容读取的新表、状态和可空谱系字段，再部署只写 handoff 且策略关闭的版本，之后按组织或环境逐步启用用户、gate 和 review 触发。完整顺序与验证要求以 [OpenSpec 迁移计划](../../openspec/changes/archive/2026-08-28-continuation-turns/design.md#migration-plan) 为准。

回滚时先关闭新请求和 claim，让已派发 child run 走既有终态处理，取消未派发请求并恢复请求前任务投影。保留 additive schema、handoff、审计和 outbox 事实，不执行破坏性 down migration。

## 验收条件

- 同 thread 新 turn/new child run 可验证，来源 run 的终态、终态时间和来源 turn 保持不变。
- 四类运行语义使用不同的状态分支、谱系字段和幂等键，断流与 retry 不会误建 continuation。
- handoff 在 cleanup 前持久化并校验；来源目录删除后仍可在新 workspace 重建等价输入。
- 取消与派发并发只产生“取消成功且未启动”或“已派发冲突”之一。
- PostgreSQL 并发/重放、假 app-server、workspace、OpenAPI/Angular 和脱敏测试通过，并完成 `cargo flow verify --all` 与 OpenSpec 严格校验。

## 关联实施

本 ADR 由已归档的 [continuation-turns OpenSpec change](../../openspec/changes/archive/2026-08-28-continuation-turns/tasks.md) 跟踪。迁移、Repository、Service、Handler、Scheduler、Supervisor、Workspace Manager、OpenAPI、Angular 和测试证据只有在对应任务通过后才视为已实现。
