# DevRail 实现状态

更新日期：2026-09-03

本文档是 DevRail 当前实现范围的唯一状态口径，用于避免把 arc-admin 基线或 `arc-flow` 审计工具误认为 Codex Harness 产品 MVP。产品需求和完成条件仍以 [requirements.md](requirements.md) 为准。

## 结论

DevRail 的 Codex Harness 开发系统 MVP **尚未实现完成**，因此不能标记为 MVP Done。

当前已完成的是：

- arc-admin 基线：登录、会话、CSRF、MFA、组织、部门、用户、角色、权限和基础审计；
- `arc-flow` 审计工具生产化：配置 schema v2、词法边界、稳健性测试、性能基准、SBOM、跨平台 CI 和操作文档；
- 工程治理：项目公约、审计门禁、CI、供应链检查和交付流程；
- Phase 0 首批产品骨架：`devrail` 业务权限、项目/仓库/环境/任务迁移、受数据范围约束的 Rust CRUD API、OpenAPI/Angular 客户端生成和 `/devrail/projects` 基础页面；仓库远端 HEAD、默认分支、分支数量、完整远端分支列表、受控工作区提交摘要和环境健康检查已加入；
- Phase 1 Harness 基础闭环：后端 `HarnessSupervisor` 受控启动 `codex app-server`，清空环境变量并限制工作区/并发/时限；运行快照、运行元数据、脱敏 JSONL 事件、单调游标、幂等键、异常退出摘要、优雅中断/强制终止、运行查询和 SSE API 已加入；受控 follow-up 工具事件支持 EOF/重启重放幂等。2026-09-02 已在隔离 PostgreSQL 与受控 workspace 中使用本机 Codex `0.152.1` 完成真实最小协议演练，覆盖 `initialize → initialized → thread/start → turn/start → turn/completed`、harness/thread/turn 元数据持久化与 workspace cleanup；该记录不替代审批全流程、浏览器、供应商或生产验收；
- Symphony P0 调度可靠性：控制循环按 `reconcile → dispatch → reap/metrics` 运行并支持优雅停止；任务使用 `task_id + attempt` 稳定幂等键、PostgreSQL `SKIP LOCKED`、可续租 claim、System Actor、优先级 aging、指数退避和最大 attempt；Supervisor 提供心跳/stall 清理、同 attempt 的 thread/resume 断流恢复、重启扫描和幂等终态通知；任务/run API 与 Angular 页面已展示 attempt、重试时间、原因、恢复建议和清理状态；
- Symphony P1 工作流基础：调度器已切换到可注入 `TaskTracker` 领域端口；`WORKFLOW.md` 支持严格 schema、封闭模板、平台安全交集、受控路径和安全默认值；任务在入队时固化不可变 workflow 快照，run 通过 SQL 身份校验复用同一快照；动态 reload、持久化 last-known-good、失败去重、System Actor 审计、指标以及任务/run 诊断字段和简体中文页面已加入；
- Symphony P1 任务工作区：已按 task/attempt 持久化独立 workspace，完成受控根目录与符号链接越界校验、Git worktree/等价目录物化、基础提交校验、受限 hooks、Supervisor 绑定、终态清理、失败退避对账、审计/指标/outbox、API 与 Angular 诊断操作；
- Hook 失败熔断：相同脱敏 Hook fingerprint 连续失败 5 次后，任务以 `hook_failure_circuit_open` 停止自动运行并要求人工介入；第 1 至第 4 次只在自动调度场景按策略重试，Hook 成功会清零计数，重复终态不会重复累计。迁移、回滚和排障说明见 [ADR-0006](adr/ADR-0006-hook-failure-circuit-breaker.md) 与 [Orchestrator 运维手册](symphony-orchestrator-operations.md)；
- continuation turns：已新增请求与 handoff 迁移、数据范围与权限、幂等 claim/取消、TaskTracker 状态投影、唯一 child run、同 thread 新 turn、重启对账、handoff 前置 cleanup、隔离 workspace 重建、终态通知/outbox、低基数指标和任务/运行详情 UI；Rust 真实 PostgreSQL、受控假 app-server/workspace 测试及 Angular Vitest 已通过；针对 continuation 的真实后端 Playwright 专项验收仍待完成；
- Phase 1 审批、重试与质量门禁基础闭环：审批迁移、数据范围查询、决策追加审计、Supervisor resolve 控制消息、终态 run 重试 API、指定 turn 的 thread/resume、审批中心 UI、审批撤回和过期 worker，以及受限命令白名单的独立质量门禁执行器已加入；质量门禁输出以稳定 log_ref 关联，提供脱敏、分页日志读取 API；
- 受控修复 run（工程实现已完成，产品验收进行中）：已加入 repair 请求/诊断/审批/人工交接事实、独立 repair child run、受控 workspace、任务和 run 谱系 API/UI、过期 claim 恢复、门禁重跑幂等记录，以及 Supervisor 终态到门禁重跑的闭环；可信 CI/审查事件适配已接通，并通过来源白名单、HMAC、证据新鲜度、changeset 摘要、跨范围和重复回调的真实 PostgreSQL 测试。2026-08-29 受控假 app-server/workspace/质量门禁 E2E 与全量 `cargo flow verify --all` 已通过（报告见 [MVP 验收证据矩阵](verification/mvp-acceptance-2026-08-28.md)），但真实设备、供应商/生产端到端演练仍未完成，不能视为 MVP 已验收；
- Phase 2 通知基础能力：站内通知事实表、按通知来源幂等的 transactional outbox、通知查询/未读计数/已读 API、终态 run 通知、审批状态通知、用户通知偏好 API/页面和 Angular 通知中心已加入；Web Push VAPID 配置校验、受保护公开配置接口、Service Worker 订阅初始化、设备注册、列表、撤销及加密存储已加入，dispatcher、delivery 重试、投递审计和 Grafana 投递告警已完成；完整自动化验收仍待补齐；
- Phase 3 协作基础能力：任务评论持久化、数据范围查询、评论发布、编辑、软删除、变更审计、`@用户名` 提及通知和任务详情页评论区已加入；代码审查请求、指定审查人、通过/驳回决策、组织边界、审计、运行详情页审查区和逐文件意见已加入；受控工作区补丁导出、敏感文件拒绝和敏感字段脱敏、GitHub/GitLab 仓库识别、安全创建合并请求深链接、API 自动创建、状态同步和外部审查意见同步已加入；临时分支创建与删除 API 已加入，run 可持久化绑定临时分支并由后台 worker 通过受控凭据删除 GitHub/GitLab 远程分支后清理绑定（远程删除失败时保留绑定并重试）；GitHub/GitLab 外部评论的全部可见 note、编辑内容、删除标记以及 GitLab 和 GitHub 原生线程 resolved 状态已归一化，并通过 `changesetMatched` 完成基于 run 文件变更事件的文件级关联；
- 2026-09-03 安全边界修复（工程实现，仍待远端 CI 与产品验收）：外部评审同步现在绑定参与者、任务、项目、仓库和组织；Webhook 目标/事件身份来自验签正文并拒绝空密钥、缺失身份和头体不一致；审批禁止发起人自批，等待/恢复/过期状态转换有数据库条件守卫；Harness 启动与审批恢复使用单一数据库抢占，质量门禁使用持久化执行租约并保护终态；事务内快照查询复用同一连接；CodeQL/依赖审查不再因 INTERNAL 可见性跳过，`rsa` 风险按真实依赖链和 2026-12-31 到期日记录。对应 ADR-0009 和 OpenSpec 变更仍为 Proposed；不表示 MVP 已验收。
- PR #23（审批撤回与过期 worker）和 PR #24（changeset/质量门禁查询）已合并到 `main`，合并提交分别为 `f50bb5c` 和 `2fc8d0c`，对应 CI、`arc-flow platform` 和供应链检查均成功。
- PR #84（continuation turns，合并提交 `acb3bc0`）和 PR #86（Hook 失败熔断，合并提交 `b66e4c1`）已合并到 `main`。本仓库不在状态文档中补写无法复查的历史 CI 结论；当前 MVP 验收以 [MVP 验收证据矩阵](verification/mvp-acceptance-2026-08-28.md) 的可追溯记录为准。

这些内容是产品 MVP 的工程基础或配套能力，不等于需求文档第 2.1 节和第 16 节定义的 DevRail 业务系统已经交付。

## 需求覆盖状态

| 需求域 | 当前状态 | 说明 |
| --- | --- | --- |
| arc-admin 认证、MFA、组织和 RBAC | 已有基线 | DevRail API 已增加业务权限标记和组织/部门/所有者数据范围过滤。 |
| 项目、仓库、环境、成员、策略 | 部分实现 | 项目/仓库/环境列表与详情、仓库/环境创建表单、成员和项目策略 API/页面已加入；仓库远端 HEAD、默认分支、分支数量、完整远端分支列表、受控工作区提交摘要、工作树状态检查以及环境健康检查已完成。 |
| 任务、快照和运行 | 部分实现 | 已有任务详情页、服务端关键词/状态/负责人/标签筛选、分页、不可变任务快照、任务与项目仓库/环境关联、run 生命周期字段、单任务活动运行唯一约束、幂等创建、终态重试、指定 turn 恢复、repair 谱系和运行详情页；完整状态验收仍待补齐。 |
| Symphony 任务调度器 | P0、DAG/follow-up、任务工作区与 continuation 已实现 | queued 任务已支持稳定 attempt、优先级 aging、活动 run 排除、依赖资格、claim 租约/恢复、取消传播、重启 reconciliation、低基数指标和 System Actor 审计；continuation 在普通 queued 派发前优先 claim，并以 handoff 证据重建独立 workspace、绑定唯一 child run 后启动。 |
| Codex `app-server` Harness Supervisor | P0 恢复与 continuation turn 已实现 | 后端独占启动受控 `codex app-server`，按响应顺序完成初始化、thread/turn 启动，支持同 attempt thread/resume 断流恢复（最多 2 次）、continuation 同 thread 新 turn、JSON-RPC 审批关联、稳定 start key、心跳/stall/超时清理、活动 run 重启恢复、不可恢复通知、审批等待状态恢复、stderr 摘要、恢复建议和携带 thread/turn ID 的优雅中断；2026-09-02 本机 Codex `0.152.1` 最小真实协议演练通过。 |
| thread/turn/item 事件与 SSE | 基础实现 | JSONL 事件按安全类型脱敏持久化，提供 cursor 补拉、Last-Event-ID 与 after_cursor SSE 补拉、固定心跳和质量门禁事件映射；运行详情断线重连会从最后游标继续。 |
| 工具命令审批 | 部分实现 | 已有审批表、数据范围 API、审批中心列表/详情、批准/拒绝/撤回决策、过期时间、过期 worker、追加决策审计、策略版本强校验和 Supervisor resolve；请求、批准、拒绝、撤回和过期均已写入站内通知/outbox。 |
| 变更集与质量门禁 | 部分实现 | 运行详情可从脱敏文件变更事件生成 changeset，并查询质量门禁事件；已支持从项目模板执行受限白名单质量门禁，记录命令摘要、执行器版本、稳定日志引用、退出码/耗时/脱敏摘要，并通过稳定引用读取受限分页日志；失败时联动 run/task 失败。 |
| 站内通知、outbox 和 Web Push | 部分实现 | 已有通知事实表、transactional outbox、run/审批通知、通知 API、用户偏好 API、通知中心，以及 VAPID 配置校验、公开配置接口、Service Worker 订阅初始化、设备注册/列表/撤销和加密存储；dispatcher 已通过 VAPID 异步投递，持久化 delivery、临时失败重试、永久失败设备失效和 Grafana backlog/失败/失效设备告警已加入；完整验收仍待补齐。 |
| DevRail Angular 功能页 | 部分实现 | 已有项目 CRUD、成员、策略、任务列表/详情、任务依赖按权限编辑（添加/删除/整批保存及环冲突反馈）、continuation 时间线与追加上下文、repair 诊断/审批/门禁重跑谱系、仓库/环境列表与详情、远端分支/提交同步、受控工作树状态检查、资源创建、审批列表/详情、运行详情谱系、通知中心和通知设置页面及生成 API 服务。 |
| 评论与提及 | 基础实现 | 任务评论 API、权限、数据范围、提及通知、编辑、软删除、审计和任务详情页评论区已加入。 |
| 代码审查 | 基础实现 | 审查请求、逐文件意见（文件路径、意见、作者编辑）、权限、运行关联、列表、通过/驳回决策、事务审计和运行详情页入口已加入；受控工作区补丁导出、敏感信息防护、Git 平台识别、临时分支创建、API 创建、状态同步、PR/MR 状态持久化、签名 Webhook 自动同步、事件去重、状态通知/outbox、外部审查意见同步及运行详情页展示入口已加入。 |
| MVP 自动化验收 | 未完成 | continuation 与受控修复 run 代码、受控 fake app-server/workspace/质量门禁 E2E 及专项前端测试已落地；可信 CI/审查事件和后端真实 PostgreSQL 套件已通过当前门禁，且工作区于 2026-08-29T03:26:11Z 通过全量 `cargo flow verify --all`（backend/frontend/arc-flow 测试 151/113/69，`TEST_SUMMARY: PASS`）。完整状态不以单次门禁替代；仍需完成真实设备、端到端供应商回调/生产运行演练，以及 requirements.md 第 16 节的逐项验收。缺口和证据入口见 [MVP 验收证据矩阵](verification/mvp-acceptance-2026-08-28.md)。 |

## 当前不应作出的结论

- 不能说“DevRail Codex Harness MVP 已完成”；
- 不能把 `cargo flow verify --all` 的通过解释为产品功能验收通过；
- 不能把审计工具的 run/step 任务当作 Codex Agent run、thread 或 turn；
- 不能把 arc-admin 的通知、审计或认证能力描述为 DevRail 的 outbox/Web Push/Harness 闭环。

## 后续实现顺序

应按 [requirements.md](requirements.md) 的迭代计划推进：

1. Phase 0：已补齐项目/仓库/环境/任务 CRUD 的主要 API、任务与仓库/环境关联、资源创建入口、仓库远端 HEAD/默认分支/分支数量/分支列表/提交摘要同步、受控环境工作树状态检查和环境健康检查；继续完成集成测试和最终验收闭环；
2. Phase 1：DevRail DB tracker 的 Symphony P0 调度可靠性、TaskTracker 抽象、仓库级 `WORKFLOW.md` 严格加载/动态 reload、不可变 task/run 快照、数据库重启恢复、断流/stall 运行验收、任务依赖 DAG、终态传播、受控 follow-up、per-task workspace/hooks、continuation turns 和受控 repair run 工程实现已完成；当前已接通 repair child、隔离 workspace、可信 CI/审查事件、门禁重跑和任务/run UI 谱系，仍需补齐假 app-server/workspace/质量门禁/Playwright 及端到端验收。审批撤回、审批等待人工恢复、过期 worker、策略版本校验、changeset/质量门禁查询、受限命令质量门禁执行、结构化门禁元数据、质量门禁失败联动和 SSE 断线补拉已完成。
3. Phase 2：已完成 transactional outbox、站内通知、通知中心、VAPID 配置/订阅初始化、设备管理、dispatcher 投递基础和 Grafana 投递告警；继续完善并发恢复验收和投递自动化测试；
4. Phase 3：评论、提及、编辑、软删除、代码审查、逐文件意见、补丁导出、Git 平台识别、API 创建、状态同步和外部审查意见同步已加入；继续进行 MVP 运行验收。

每个阶段完成后，都必须同步需求、API、数据模型、权限、UI 和测试状态；只有 requirements.md 第 16 节全部满足后，才能将 MVP 标记为完成。
