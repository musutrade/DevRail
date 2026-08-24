# DevRail 实现状态

更新日期：2026-08-23

本文档是 DevRail 当前实现范围的唯一状态口径，用于避免把 arc-admin 基线或 `arc-flow` 审计工具误认为 Codex Harness 产品 MVP。产品需求和完成条件仍以 [requirements.md](requirements.md) 为准。

## 结论

DevRail 的 Codex Harness 开发系统 MVP **尚未实现完成**，因此不能标记为 MVP Done。

当前已完成的是：

- arc-admin 基线：登录、会话、CSRF、MFA、组织、部门、用户、角色、权限和基础审计；
- `arc-flow` 审计工具生产化：配置 schema v2、词法边界、稳健性测试、性能基准、SBOM、跨平台 CI 和操作文档；
- 工程治理：项目公约、审计门禁、CI、供应链检查和交付流程；
- Phase 0 首批产品骨架：`devrail` 业务权限、项目/仓库/环境/任务迁移、受数据范围约束的 Rust CRUD API、OpenAPI/Angular 客户端生成和 `/devrail/projects` 基础页面；仓库远端 HEAD、默认分支和分支数量同步以及环境健康检查已加入；
- Phase 1 Harness 基础闭环：后端 `HarnessSupervisor` 受控启动 `codex app-server`，清空环境变量并限制工作区/并发/时限；运行快照、运行元数据、脱敏 JSONL 事件、单调游标、幂等键、异常退出摘要、优雅中断/强制终止、运行查询和 SSE API 已加入；
- Phase 1 审批、重试与质量门禁基础闭环：审批迁移、数据范围查询、决策追加审计、Supervisor resolve 控制消息、终态 run 重试 API、指定 turn 的 thread/resume、审批中心 UI、审批撤回和过期 worker，以及受限命令白名单的独立质量门禁执行器已加入；
- Phase 2 通知基础能力：站内通知事实表、transactional outbox、通知查询/未读计数/已读 API、终态 run 通知、审批状态通知、用户通知偏好 API/页面和 Angular 通知中心已加入；Web Push 设备注册、列表、撤销及加密存储已加入，dispatcher、VAPID 初始化、delivery 重试和投递审计仍未完成；
- PR #23（审批撤回与过期 worker）和 PR #24（changeset/质量门禁查询）已合并到 `main`，合并提交分别为 `f50bb5c` 和 `2fc8d0c`，对应 CI、`arc-flow platform` 和供应链检查均成功。

这些内容是产品 MVP 的工程基础或配套能力，不等于需求文档第 2.1 节和第 16 节定义的 DevRail 业务系统已经交付。

## 需求覆盖状态

| 需求域 | 当前状态 | 说明 |
| --- | --- | --- |
| arc-admin 认证、MFA、组织和 RBAC | 已有基线 | DevRail API 已增加业务权限标记和组织/部门/所有者数据范围过滤。 |
| 项目、仓库、环境、成员、策略 | 部分实现 | 项目/仓库/环境列表与详情、仓库/环境创建表单、成员和项目策略 API/页面已加入；仓库远端 HEAD、默认分支、分支数量同步、受控环境工作树状态检查以及环境健康检查已完成，完整资源同步仍待补齐。 |
| 任务、快照和运行 | 部分实现 | 已有任务详情页、服务端关键词/状态/负责人/标签筛选、分页、不可变任务快照、run 生命周期字段、单任务活动运行唯一约束、幂等创建、终态重试、指定 turn 恢复和运行详情页；仓库环境关联和完整状态验收仍待补齐。 |
| Codex `app-server` Harness Supervisor | 基础实现 | 后端独占启动受控 `codex app-server`，完成初始化等待、thread/turn 启动、thread/resume、活动 run 数据库重启恢复、超时、stderr 摘要、恢复建议和优雅中断；审批等待状态仍需通知与人工恢复验收。 |
| thread/turn/item 事件与 SSE | 基础实现 | JSONL 事件按安全类型脱敏持久化，提供 cursor 补拉、Last-Event-ID SSE、质量门禁事件映射和运行详情展示。 |
| 工具命令审批 | 部分实现 | 已有审批表、数据范围 API、审批中心列表/详情、批准/拒绝/撤回决策、过期时间、过期 worker、追加决策审计、策略版本强校验和 Supervisor resolve；请求、批准、拒绝、撤回和过期均已写入站内通知/outbox。 |
| 变更集与质量门禁 | 部分实现 | 运行详情可从脱敏文件变更事件生成 changeset，并查询质量门禁事件；已支持从项目模板执行受限白名单质量门禁，记录命令摘要、执行器版本、稳定日志引用、退出码/耗时/脱敏摘要并在失败时联动 run/task 失败；更丰富的日志后端仍待补齐。 |
| 站内通知、outbox 和 Web Push | 部分实现 | 已有通知事实表、transactional outbox、run/审批通知、通知 API、用户偏好 API、通知中心，以及设备注册/列表/撤销和加密存储；VAPID 初始化、dispatcher、投递重试和投递审计仍待补齐。 |
| DevRail Angular 功能页 | 部分实现 | 已有项目 CRUD、成员、策略、任务列表/详情、仓库/环境列表与详情、受控工作树状态检查、资源创建、审批列表/详情、运行详情、通知中心和通知设置页面及生成 API 服务。 |
| MVP 自动化验收 | 未完成 | 全量工程门禁通过不代表 requirements.md 第 16 节全部条件通过。 |

## 当前不应作出的结论

- 不能说“DevRail Codex Harness MVP 已完成”；
- 不能把 `cargo flow verify --all` 的通过解释为产品功能验收通过；
- 不能把审计工具的 run/step 任务当作 Codex Agent run、thread 或 turn；
- 不能把 arc-admin 的通知、审计或认证能力描述为 DevRail 的 outbox/Web Push/Harness 闭环。

## 后续实现顺序

应按 [requirements.md](requirements.md) 的迭代计划推进：

1. Phase 0：已补齐项目/仓库/环境/任务 CRUD 的主要 API、资源创建入口、仓库远端 HEAD/默认分支/分支数量同步、受控环境工作树状态检查和环境健康检查；继续完成完整资源同步、集成测试和最终验收闭环；
2. Phase 1：补齐过期通知和更丰富的质量门禁日志后端，并完善数据库重启恢复与运行验收；审批撤回、过期 worker、策略版本校验、changeset/质量门禁查询、受限命令质量门禁执行、结构化门禁元数据、质量门禁失败联动和活动 run 自动恢复已完成。
3. Phase 2：已完成 transactional outbox、站内通知、通知中心和设备管理第一步；继续实现 VAPID 初始化、Web Push dispatcher、投递重试和投递审计；
4. Phase 3：评论、提及、审查、补丁导出和可选 Git 平台集成。

每个阶段完成后，都必须同步需求、API、数据模型、权限、UI 和测试状态；只有 requirements.md 第 16 节全部满足后，才能将 MVP 标记为完成。
