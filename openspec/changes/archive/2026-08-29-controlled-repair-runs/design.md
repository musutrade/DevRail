## Context

当前 run、质量门禁、continuation、workspace、审批、审计与 transactional outbox 已分别具备持久化事实，但质量门禁失败尚没有独立的 repair 请求与 repair run 谱系。实现必须继续遵守 `Handler → Service → Repository → PostgreSQL`、SQL 写入仅在 Repository/迁移/测试层、后端独占 Harness、默认网络关闭、受控 workspace、数据范围和敏感信息脱敏等项目硬边界。具体行为契约见本 change 的四份 delta spec。

## Goals / Non-Goals

**Goals:**

- 建立独立的 repair request/run/handoff/诊断事实与状态机，来源 run 终态不可变。
- 在单一数据库事务中完成请求幂等、任务投影、审批事实、审计/事件/outbox 与 child run/workspace 绑定。
- 复用现有质量门禁、Harness Supervisor、TaskTracker、Workspace Manager 和 SSE/通知能力，但不混用 retry、transport recovery、continuation 或 follow-up 身份。
- 让 worker 重启、claim 过期、重复事件、取消竞态和门禁重跑具备确定性恢复路径。

**Non-Goals:**

- 本 change 不实现自动合并、自动修改受保护分支、任意依赖升级或未审批的逻辑修复。
- 本 change 不引入 Redis、独立执行集群或新的模型供应商；Agent 仍只能由 Harness Supervisor 启动。
- 本 change 不把浏览器截图/视频、Chrome DevTools MCP 或外部 issue tracker adapter 作为实现前置条件。

## Decisions

### 1. 独立 repair request 与 repair run 谱系

新增 repair request 事实表及 run kind/parent 字段，稳定幂等键由组织、任务、来源 run、失败证据身份和修复序号组成。repair run 使用新的 attempt/child run 与 workspace，但沿用来源任务快照和权限边界。选择独立谱系而不是复用 continuation，是为了让“继续目标”与“修复失败门禁”分别限额、审计、重试和展示。

### 2. 诊断快照先持久化，正文只保留受限引用

由质量门禁/CI/审查 Service 生成规范化低敏感度诊断，Repository 在创建 repair request 同一事务内固化摘要、证据 ID、changeset 摘要和受限日志引用。原始长日志继续由既有受控日志读取边界管理，repair 记录不复制正文。这样可以保证重启后输入可复现，同时避免把 secret scan 与日志脱敏责任分散到多个 worker。

### 3. 双阶段事务：数据库绑定后才允许外部执行

Workspace 物化、Git 检查和质量门禁执行在事务外完成；只有 repair request claim、任务投影、child run、workspace 绑定、审批满足条件和 start key 持久化提交后，Supervisor 才能启动 Agent。若外部步骤失败，按临时/确定性错误分类释放 claim 或交接人工，数据库不留下孤儿进程或未关联 workspace。

### 4. 风险分级而不是“自动修复万能开关”

策略把触发来源、修复类别、最大次数/成本和审批要求固化到任务快照。格式化/未使用导入等低风险类别可自动执行或仅生成建议；逻辑、依赖、远端写入及安全策略变化必须走审批；Hook 熔断、预算、容量和证据新鲜度是更高优先级的 fail-closed 条件。

### 5. 门禁重跑使用稳定执行身份

repair run 完成后只重跑诊断标记的受影响门禁，使用 `repair_request_id + gate_id + changeset_digest` 的稳定身份去重。门禁结果、来源失败和修复结果都通过 ID 双向关联，重复终态只投影一次，避免把每次 webhook 重放误计为新的修复次数。

### 6. 人工交接是终态而非隐式失败

达到次数/成本上限、策略拒绝、证据漂移、审批失败或 Hook 熔断时，将 repair request 标记为人工交接并生成受权限保护的处理项、低基数指标和脱敏通知。人工重试必须显式经过受保护入口，不能通过直接改数据库字段恢复自动化。

## Risks / Trade-offs

- [诊断证据过期或 changeset 漂移] → 派发前重新校验摘要、来源终态和仓库基础提交；不匹配时 fail closed 并交接人工。
- [修复 run 与普通 retry/continuation 混淆] → 新增独立运行种类、父子字段、幂等键、状态历史和 UI 标签，并在服务层拒绝跨语义复用。
- [worker 在事务提交边界崩溃] → 使用 claim lease、唯一 child run/start key、dispatched 未启动 reconciliation 和有界过期释放。
- [自动修复扩大权限或泄露敏感内容] → 复用来源快照安全交集，所有高风险类别审批，诊断与通知只保存摘要/引用，运行前执行 secret scan 与路径边界校验。
- [重复门禁重跑导致成本失控] → 受影响 gate 的稳定执行身份、请求级次数/成本预算、Hook 熔断协同和人工交接终态。

## Migration Plan

1. 先部署 additive migration、可读模型和权限/指标定义；repair 策略默认关闭，旧 worker 忽略新状态。
2. 部署只生成诊断快照、记录失败证据和人工交接事实的兼容版本，确认数据范围、脱敏和 outbox 一致性。
3. 启用低风险 repair 及其门禁重跑，观察 claim、容量、workspace、审批和人工交接指标；再按组织或 workflow 分阶段启用更高风险类别。
4. 回滚时关闭 repair 触发和 worker 领取，已派发 run 按普通终态流程完成，未派发请求转人工；保留新增表、历史、审计、事件和 outbox，不执行破坏性 down migration。

## Open Questions

- 低风险修复类别的初始白名单和组织级预算阈值可在实现阶段依据实际门禁命令目录确定，不改变本 change 的谱系、权限或人工交接契约。
