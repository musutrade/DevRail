## Context

DevRail 已有 `devrail_tasks -> devrail_runs -> devrail_events` 控制面、TaskTracker、带租约的调度 claim、Harness Supervisor、Codex app-server thread/turn 标识、任务快照、changeset、隔离 workspace、状态历史、审计和 transactional outbox。现有恢复语义服务于同一 run/attempt 的传输中断或终态 retry，终态 workspace 会进入 hook 与 cleanup，尚无独立的 continuation 事实或可在清理后重建追加 turn 的证据边界。动机见 `proposal.md`，可观察行为见本 change 的四份 delta spec。

实现继续遵守 DevRail 的数据库写入分层、组织/部门/所有者 SQL 数据范围、受控工作区、默认断网、命令审批、全链路脱敏和 outbox 约束。新增字段和表必须兼容已有 run、历史任务和已部署 worker 的滚动升级。

## Goals / Non-Goals

**Goals:**

- 建立可持久化、可领取、可取消、可重放的 continuation 请求账本，使并发和进程重启下最多创建一个 child run。
- 在不修改来源 run 终态的前提下，为同一 Codex thread 创建新 turn，并让 retry、传输恢复、continuation 和 follow-up task 具有不同谱系。
- 在来源 workspace 清理前固化 handoff 证据，使 continuation 始终在新 workspace 中可复现地恢复受控变更。
- 将权限、审计、事件、outbox、指标、OpenAPI 和中文 UI 纳入同一验收闭环。

**Non-Goals:**

- 不让质量门禁或审查事件直接生成任意补丁；本 change 只把可信反馈作为 continuation 输入。
- 不实现自动合并、外部 Tracker adapter、任意分支写入或生产环境自动发布。
- 不复用来源进程、来源 workspace 或 app-server 的中断 attempt；同 attempt 恢复继续使用现有 recovery/retry 机制。
- 不为历史 run 猜测或回填不可验证的 changeset；证据不足的历史 run 明确不可 continuation。

## Decisions

### 1. 使用独立 continuation 请求账本

新增 `devrail_continuation_requests`，而不是在 `devrail_runs` 上放一个布尔标记。每行包含组织、部门、所有者、项目和任务范围，来源 run/turn、根 run、触发类型、触发证据引用与摘要、经脱敏和规范化的输入、输入摘要、幂等键、序号、链深、请求前任务状态、策略版本、状态、claim owner/token/expiry、child run、低基数结果码及生命周期时间。

请求状态采用 `pending -> claimed -> dispatched -> completed` 主路径，并允许在 Agent 启动前进入 `cancelled`、确定性校验失败进入 `rejected`。临时错误释放或过期 claim 后回到 `pending` 并记录下一次调度时间。数据库唯一约束覆盖 `(organization_id, task_id, idempotency_key)`、`child_run_id` 和 `(task_id, continuation_sequence)`；幂等键由规范化的来源 run、触发类型和稳定证据 ID 计算，用户请求使用服务端签发或客户端提交的稳定请求 ID，不依赖自由文本哈希作为唯一身份。

选择独立表是因为请求在 run 创建前就需要可见、可取消和可 claim，也需要记录拒绝结果。替代方案是在 task 或 run 上增加临时字段，但无法表达并发请求、重启窗口和重复事件，且会迫使修改来源 run。

### 2. task、run 和 turn 使用分离的谱系

任务状态新增 `continuation_pending`。创建有效请求的事务锁定 task 和来源 run，将请求前的 `succeeded` 或 `failed` 投影保存到请求，写入 `continuation_pending` 与状态历史；派发事务再转换为 `running`。取消或拒绝且尚未启动时恢复保存的请求前状态；child run 终态通过现有 TaskTracker 终态规则更新任务。所有转换均以 task 版本和“无活动 run”为条件。

`devrail_runs` 增加可查询的 `run_kind`、`parent_run_id`、`parent_turn_id`、`continuation_request_id` 和 `continuation_sequence`。普通 run 默认 `primary`，retry 仍增加该 run 语义下的 attempt，follow-up 仍属于新 task；continuation 创建同 task 的新 run，复用来源 `codex_thread_id`，调用 app-server 的“恢复持久 thread 后启动新 turn”操作，并保存 app-server 返回的新 `codex_turn_id`。`continuation_request_id` 使用唯一约束，确保恢复时复用既有 child run。

选择新 child run 而不是重开来源 run，是为了保持终态不可变、让每次追加执行拥有独立事件、workspace、attempt、成本和清理结论。选择同 task 而不是 follow-up task，是因为 continuation 延续同一目标和验收标准，不创建新的任务依赖节点。

### 3. 请求创建和派发采用两个可恢复事务边界

创建入口统一调用 Continuation Service；Service 做权限、触发证据、输入脱敏和策略校验，Repository 在单一事务中完成幂等请求写入、task 投影、状态历史、领域事件、审计和 outbox 事实。所有 SQL 写入继续只存在于 Repository/db/migration/test 层。重复幂等键返回原请求，不重复转换 task 或产生通知。

Scheduler reconciliation 在普通 queued claim 前，通过 `FOR UPDATE SKIP LOCKED` 领取到期的 `pending` 请求并签发有期限的 claim token。workspace 准备是事务外的受控 I/O；准备完成后，派发事务再次校验 request/task/source run/claim/version，创建或复用唯一 child run、绑定 workspace、标记 `dispatched` 并把 task 转为 `running`。事务提交后 Supervisor 才能启动 Agent；若提交后进程崩溃，下一轮 reconciliation 根据稳定 child run/start key 恢复启动，不再创建 run。

取消使用请求状态和 claim token 的条件更新。取消先获胜时，派发事务失败并清理新 workspace；派发先获胜时，取消 API 返回“已派发”冲突，后续取消走既有 run cancellation。该顺序避免数据库持有长事务等待 Git 或 app-server I/O。

替代方案是创建请求时同步创建 workspace 和 run，但请求 Handler 会承担长时间外部 I/O，无法满足 outbox、重启恢复和请求延迟边界。

### 4. 策略在来源任务快照中固化并统一执行

workflow/task snapshot 增加 continuation 策略：是否启用、允许触发类型、每个根 run 的最大 continuation 数、最大链深、追加上下文字节数、claim 租约、最大派发重试和退避。初始默认值为关闭；显式启用后默认最多 3 次、最大链深 3、UTF-8 输入最多 16 KiB，且同一 task 同时只允许一个活动 run 或未终结 continuation。自动触发必须带受信任的 gate result ID 或 review event/thread ID 及当前 changeset 摘要；过期或摘要不匹配时拒绝。

策略在请求创建和派发前各校验一次，最终以来源快照版本为准。这样可以阻止质量门禁或审查事件形成无限循环，并避免部署中修改全局配置改变已开始任务的行为。替代方案是只使用当前环境配置，但会使重放结果随时间变化。

### 5. 终态 handoff 是 workspace cleanup 的前置事实

新增按来源 run 唯一的 `devrail_run_handoffs`，包含完整数据范围、任务/workflow/environment 快照引用及摘要、仓库身份、基础提交、受控 head commit 或分支引用、changeset ID 与内容摘要、工具版本、证据状态和创建时间。终态 reconciliation 在执行 cleanup 前调用 Workspace Manager 固化并校验 handoff；凭据、绝对路径和完整命令输出不进入 handoff。已有 run 若没有可验证 handoff，只能显示 continuation 不可用，不能从残留目录推导输入。

child workspace 使用新的 workspace ID 和受控根目录路径：先检出固定基础提交或受控 head commit，再按摘要校验并应用持久化 changeset，最后校验仓库身份、当前提交和工作树摘要。来源路径无论是 `completed`、`failed` 还是仍在 cleanup，都不会被复用。来源与 child 的 hook、占用和 cleanup 幂等键分别绑定各自 run/workspace，避免交叉删除。

选择显式 handoff 记录而不是仅保存旧路径，是因为终态目录按设计应被删除，`cleanup_failed` 目录也不可信且不可复用。选择固定提交和 changeset 摘要而不是只记可移动分支，是为了保证重建不受远端分支推进影响。

### 6. 三类触发共享领域服务但保留不同信任入口

用户入口提供创建、列表/详情和取消 API，并使用 continuation 的 read/create/cancel 权限；请求体只允许追加上下文和稳定幂等 ID，来源 run 从受范围约束的路由资源解析。质量门禁与审查入口由现有受信任后端集成调用领域服务，分别使用 gate result ID 和 review event/thread ID 作为证据，不接受前端伪造触发类型。

读接口按 task/run 谱系分页返回状态、序号、触发类型、脱敏摘要、来源与 child run 引用和安全错误码。Rust DTO/`utoipa` 是契约源，重新生成 `docs/openapi.json` 和 Angular client。任务与 run 详情用中文展示谱系时间线；仅当后端能力响应允许时显示“追加上下文”，取消操作仅对未启动请求可用。SSE 更新复用领域事件，页面刷新后仍以查询 API 为事实源。

### 7. 事件、副作用与可观测性全部使用稳定键

为请求创建、领取、派发、取消、拒绝和终态定义脱敏领域事件；审计和 outbox 以 continuation request ID、状态版本和事件类型构成幂等键。推送只包含通知 ID、事件类型、脱敏摘要和指向 task/run 详情的受控深链接，打开后重新鉴权和执行数据范围校验。

指标沿用框架兼容的 `arc_admin_*` 前缀，增加请求计数、待处理深度、派发延迟、claim 冲突、重放、拒绝、取消、恢复和 child run 结果；标签仅使用触发类型、状态、低基数原因和策略版本。trace 关联 request、source run、child run、workspace 和 outbox，但日志不记录用户完整上下文、证据正文或路径。

### 8. 用 ADR 固化不可互换的运行语义

实现时新增 `ADR-0005-continuation-turn-lifecycle.md`，记录以下结论：continuation 是同 thread 新 turn 和新 child run；来源 run 终态不可变；retry、transport recovery、continuation、follow-up 分离；handoff 必须先于 cleanup；取消以派发事务为线性化点。ADR 链接 proposal、delta specs 和落地迁移。

ADR 不在提案阶段单独创建，避免规划尚未通过时让架构目录提前宣称能力已实现。替代方案是扩写现有 retry/workspace ADR，但会让多个生命周期概念继续耦合且难以单独回滚。

## Risks / Trade-offs

- [新增 `continuation_pending` 使旧 worker 不认识状态] -> 先部署可读新 schema/枚举但不领取 continuation 的兼容版本，再启用创建与 worker；旧 worker 的 queued 查询明确排除未知状态。
- [请求与 workspace I/O 分属不同事务，可能留下已创建但未绑定的目录] -> 使用稳定 request/workspace key、准备状态和 reconciliation 清理，Agent 仅在数据库绑定提交后启动。
- [终态 handoff 增加 cleanup 延迟和 changeset 存储] -> 只持久化受控重建证据和摘要，限制大小；handoff 失败保留可诊断状态但不阻止来源 run 结论落库。
- [用户或自动反馈包含秘密或超大输入] -> 在持久化前做大小、类型、secret 和路径策略校验；存储规范化脱敏输入，日志、事件、指标和通知只记录摘要。
- [自动 gate/review continuation 形成成本循环] -> 策略默认关闭并固化总次数、链深和触发类型；重复证据由唯一键去重，超过预算明确拒绝。
- [远端分支在来源 run 后推进导致重建漂移] -> handoff 固定 commit 与 changeset digest，不以可移动分支头作为唯一证据；不匹配时拒绝派发。
- [历史 run 无 handoff，功能覆盖不完整] -> 不做猜测性回填；UI/API 返回稳定的“缺少可验证交接证据”，只对新终态 run 开放 continuation。

## Migration Plan

1. 以 additive migration 创建 continuation/handoff 表、索引和唯一约束，扩展 task 状态与 run 谱系字段；所有新字段先允许旧记录使用兼容默认值。
2. 部署能读取新字段、写入终态 handoff、但 continuation 创建和 worker 均关闭的版本，验证 handoff 指标、脱敏和 cleanup 顺序。
3. 重新生成 OpenAPI/Angular client，部署只读谱系 UI；随后按组织或环境启用用户触发，最后分别启用 gate 和 review 自动触发。
4. 观察 pending 深度、claim 过期、重复率、拒绝原因、workspace 清理和成本指标，再扩大策略范围。
5. 回滚时先关闭所有新请求和 claim；让已派发 child run 走普通 run 终态处理，将未派发请求取消并恢复请求前 task 状态。保留 additive 表、字段和审计事实，不执行破坏性 down migration。
