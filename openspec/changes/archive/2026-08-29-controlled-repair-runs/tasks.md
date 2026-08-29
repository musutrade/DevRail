## 1. 基线、策略与数据模型

- [x] 1.1 执行 `cargo flow scope`，读取 `review_context.json`、现有质量门禁/任务/run/workspace/审批模板和主规格，形成明确 backend、frontend、migration、workflow 文件清单；验证 scope 只包含预期范围
- [x] 1.2 新增 repair 策略、运行种类、状态枚举和低基数错误码，固化默认关闭、最大次数/成本、诊断大小、审批和人工交接阈值；通过默认值、非法枚举、序列化往返和旧快照兼容测试
- [x] 1.3 新增 additive PostgreSQL migration，创建 repair request、诊断快照、门禁重跑/人工交接事实及必要的 run/task 谱系字段、范围列、状态、claim、证据和唯一约束；通过空库与历史 task/run 正向迁移测试
- [x] 1.4 增加 repair read/create/cancel/approve/handoff 权限及业务权限种子，确保新表包含组织、部门和所有者、项目和任务边界；通过权限矩阵、种子幂等和跨组织不可见测试

## 2. Repository 与事务事实

- [x] 2.1 按 Repository 模板实现诊断快照的脱敏规范化、范围查询、稳定引用和不可变读取；通过 secret、完整命令、绝对路径、凭据和过期证据拒绝测试
- [x] 2.2 实现 repair request 的幂等创建、详情、分页、状态读取和人工交接更新；通过重复质量门禁/CI/审查事件返回原结果且不重复占额的 PostgreSQL 测试
- [x] 2.3 实现 `pending/claimed/dispatched/handed_off/terminal` 的 `SKIP LOCKED` 领取、续租、过期释放、退避和 claim token 条件更新；通过多 worker 竞争、旧 token 写入拒绝和重启恢复测试
- [x] 2.4 扩展 TaskTracker Repository，在单一事务中完成待修复、修复运行中、成功、失败、取消和人工交接投影及不可变状态历史；通过版本冲突、来源终态不变和重复终态测试
- [x] 2.5 扩展 run Repository，按 repair request 唯一创建或复用 child repair run，保存来源 run/turn、repair 序号、attempt、run kind 和稳定 start key；通过并发创建最多一个 child run 的测试
- [x] 2.6 实现受影响质量门禁重跑事实、`repair_request_id + gate_id + changeset_digest` 稳定幂等键及结果关联；通过重复 webhook、摘要漂移和门禁结果不匹配测试
- [x] 2.7 将 repair 创建/取消/拒绝/派发/终态、任务历史、审计、领域事件和 transactional outbox 组合为 Repository 事务边界；通过故障注入验证整体回滚且无重复通知事实

## 3. Service、授权与诊断

- [x] 3.1 实现可信质量门禁、CI 回调和审查事件的来源验证、证据新鲜度和 changeset 摘要校验；通过伪造前端触发、过期证据和跨范围事件测试
- [x] 3.2 实现失败诊断聚合，限制日志/摘要/环境字段大小并在持久化前脱敏；通过敏感字段、长输入、完整上下文和受控路径扫描测试
- [x] 3.3 实现 repair 策略评估，区分低风险建议、需审批操作、禁止操作、预算/容量和 Hook 熔断；通过策略关闭、类别边界、次数/成本上限和 `hook_failure_circuit_open` 测试
- [x] 3.4 实现人工交接服务和受保护的人工重试/取消入口；通过权限、过期审批、重复操作、来源终态不可变和安全错误映射测试
- [x] 3.5 按 Handler 模板增加 repair 查询、创建、详情、取消、审批和人工交接路由，统一映射未找到、无权限、策略拒绝、版本冲突、证据过期和暂时不可用错误；通过路由鉴权集成测试
- [x] 3.6 定义 repair 生命周期事件、审计和 outbox payload，确保推送 payload 只包含通知 ID、事件类型、脱敏摘要和受控深链接；通过 secret scan、重复副作用和 payload schema 测试

## 4. Scheduler、Supervisor 与门禁重跑

- [x] 4.1 在 Scheduler reconciliation 中处理 repair claim，并在普通 queued、continuation 派发边界保持确定性优先顺序和互斥活动 run 约束；通过 `dispatch_prioritizes_continuations_before_queued_tasks`、repair claim 与容量隔离测试
- [x] 4.2 实现 repair 派发前的二次校验：来源失败、诊断、审批、策略、Hook 熔断、任务状态、容量和 workspace；通过 `repair_dispatch_keeps_capacity_transient_but_hands_off_policy_rejections`、可信证据与人工交接测试
- [x] 4.3 实现 workspace 准备后的 repair 派发事务，原子创建/复用 child run、绑定 workspace、更新 request/task 投影并持久化 start key；通过 `repair_claim_child_creation_and_terminal_projection_are_idempotent` 及 repair workspace 重启/幂等测试
- [x] 4.4 扩展 Harness Supervisor，以 repair run 身份启动 Agent，保持来源 run/continuation/retry/follow-up 分支独立；通过 repair launch/recovery 代码路径、run kind/谱系断言和现有 Supervisor 测试验证
- [x] 4.5 将受影响质量门禁重跑接入 repair 终态流程，并以稳定幂等键关联 changeset、原始失败和新结果；通过 `repair_claim_child_creation_and_terminal_projection_are_idempotent`、gate rerun 幂等与重复终态测试
- [x] 4.6 实现 claim 丢失、已存在 child run、审批撤回、启动前取消、Hook 熔断和 worker 重启的 reconciliation；通过 repair claim 过期恢复、workspace cleanup 幂等、策略拒绝和 Supervisor reconciliation 测试

## 5. Workspace、清理与安全边界

- [x] 5.1 扩展 Workspace Manager，从不可变任务快照、诊断引用和受控 changeset 创建独立 repair workspace；通过 `controlled_paths_reject_escape_and_materialize_inside_root`、仓库身份/基础提交和符号链接越界测试
- [x] 5.2 禁止复用来源、retry、continuation、活动 repair 或 `cleanup_failed` workspace 路径，并为 repair 使用独立生命周期/幂等键；通过 `repair_workspace_keys_are_isolated_from_other_run_kinds` 与 `repair_materialization_rejects_path_reuse_and_cleanup_is_idempotent`
- [x] 5.3 在 repair 启动前取消、workspace 失败和进程重启路径执行幂等 cleanup，保留来源诊断和 handoff 审计；通过 `handoff_rebuild_survives_source_cleanup_and_rejects_tampering`、cleanup reconciliation 和重启测试
- [x] 5.4 验证 repair workspace、诊断、事件、数据库响应、日志和通知不包含凭据、完整命令、绝对路径或原始失败正文；通过 diagnosis 脱敏/大小限制测试及 `cargo flow verify --all` secret scan

## 6. OpenAPI、Angular 与用户处理流程

- [x] 6.1 扩展 Rust DTO/utoipa schema，覆盖 repair 能力、诊断摘要、请求状态、审批、人工交接、门禁重跑、谱系和安全错误；重新生成 `docs/openapi.json` 并通过契约测试
- [x] 6.2 重新生成 Angular API client，接入任务/run 详情的 repair 数据加载、SSE 刷新、错误映射和权限显隐；验证生成文件无手工漂移且 TypeScript typecheck 通过
- [x] 6.3 在任务/run 详情展示来源失败、诊断摘要、repair 序号/状态、门禁重跑结果、来源/child run 跳转和人工交接原因；通过 Vitest 覆盖空态、策略关闭、重复请求和中文错误
- [x] 6.4 增加低风险修复建议、审批、取消和人工重试操作，禁止未授权或高风险类别直接执行；通过 Vitest 覆盖审批撤回/过期、Hook 熔断、权限和终态不变
- [x] 6.5 完成修复列表/详情的加载状态、焦点恢复、ARIA 标签、脱敏摘要和移动端布局；通过 Angular 可访问性测试及 Playwright 桌面/移动视口验收

## 7. 指标、测试与运行验收

- [x] 7.1 增加 `arc_admin_*` repair 请求量、诊断拒绝、claim 冲突、派发延迟、门禁重跑、人工交接、预算拒绝、Hook 熔断协同和 child 结果指标；验证标签低基数且不含 request/run/路径/错误正文
- [x] 7.2 执行真实 PostgreSQL 集成套件，覆盖范围隔离、幂等创建、多 worker claim、任务投影、审批/取消竞态、重启恢复、唯一 child run、门禁重跑和来源不可变；验证隔离 `TEST_DATABASE_URL` 下通过
- [x] 7.3 执行受控假 app-server/workspace/质量门禁套件，覆盖单次 Agent 启动、失败诊断、retry/continuation/repair 分支、Hook 熔断、来源 cleanup 后重建和重复终态；验证敏感数据不落盘
- [x] 7.4 执行 `cargo flow verify --components backend`，修复 secret scan、审计、Clippy、编译和 Rust 测试问题；验证 backend 输出通过
- [x] 7.5 执行 `cargo flow verify --components frontend`，修复 lint、typecheck、Vitest、build、可访问性和 Playwright 问题；验证 frontend 输出通过
- [x] 7.6 执行 `cargo flow verify --all`、`openspec validate --all --strict` 和修复 run 演练；验证输出包含 `TEST_SUMMARY: PASS` 且每个 spec scenario 有可追踪测试证据

## 8. 文档、回滚与交付

- [x] 8.1 更新需求、Symphony 证据矩阵、架构、实现状态、HANDOFF、OpenAPI 和运行手册，明确 repair 与 retry/continuation/Hook 熔断边界及当前未验收项；通过 `openspec validate --all --strict`、链接检查和文档漂移复核
- [x] 8.2 更新 Orchestrator 运维手册，说明默认关闭、风险分级、审批、诊断保留、人工交接、指标告警、分阶段启用和 additive migration 回滚；通过配置默认值核对和严格 OpenSpec 校验
- [x] 8.3 用明确文件清单提交并推送；确认当前分支/PR 状态，记录提交 SHA、分支、PR URL 和可查询的 CI workflow 记录（commit `8e1e991a6baa7a1d31d5e3381ce55f4da9320d5c`，分支 `feat/controlled-repair-runs`，PR [#87](https://github.com/musutrade/DevRail/pull/87)，CI `33232529047`、Supply chain security `33232529042`、arc-flow platform `33232529061`）
- [x] 8.4 在 PR 描述列出 ADR、OpenSpec change、迁移、权限、测试、风险、回滚和人工交接；持续监控 CI、arc-flow、供应链和 CodeQL，失败时按日志 trace 修复并重新验证，直到 required checks 成功（CI、Supply chain security、arc-flow platform 均 `success`；CodeQL 按工作流配置 `skipped`）
