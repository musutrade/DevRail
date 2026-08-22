# DevRail 实现状态

更新日期：2026-08-22

本文档是 DevRail 当前实现范围的唯一状态口径，用于避免把 arc-admin 基线或 `arc-flow` 审计工具误认为 Codex Harness 产品 MVP。产品需求和完成条件仍以 [requirements.md](requirements.md) 为准。

## 结论

DevRail 的 Codex Harness 开发系统 MVP **尚未实现完成**，因此不能标记为 MVP Done。

当前已完成的是：

- arc-admin 基线：登录、会话、CSRF、MFA、组织、部门、用户、角色、权限和基础审计；
- `arc-flow` 审计工具生产化：配置 schema v2、词法边界、稳健性测试、性能基准、SBOM、跨平台 CI 和操作文档；
- 工程治理：项目公约、审计门禁、CI、供应链检查和交付流程；
- PR #10 已合并到 `main`，合并提交为 `9ef8950`，合并后的主 CI、`arc-flow platform` 和供应链检查均成功。

这些内容是产品 MVP 的工程基础或配套能力，不等于需求文档第 2.1 节和第 16 节定义的 DevRail 业务系统已经交付。

## 需求覆盖状态

| 需求域 | 当前状态 | 说明 |
| --- | --- | --- |
| arc-admin 认证、MFA、组织和 RBAC | 已有基线 | 可复用，但还需要为 DevRail API 增加业务权限和数据范围校验。 |
| 项目、仓库、环境、成员 | 未实现 | 尚无 `devrail_*` 业务表、Repository、Service、Handler 和页面。 |
| 任务、快照和运行 | 未实现 | 尚无任务状态机、run 生命周期和不可变快照。 |
| Codex `app-server` Harness Supervisor | 未实现 | 当前仓库没有受控启动、JSONL 握手、进程恢复和取消实现。 |
| thread/turn/item 事件与 SSE | 未实现 | 当前没有 DevRail 事件持久化、游标补拉和实时事件页面。 |
| 工具命令审批 | 未实现 | 当前没有审批表、审批 API、审批中心和决策审计。 |
| 变更集与质量门禁 | 部分具备基础工具 | `arc-flow` 可作为门禁工具，但尚未接入 DevRail run、变更集和任务状态。 |
| 站内通知、outbox 和 Web Push | 未实现 | 当前没有 DevRail 通知表、dispatcher、设备注册、投递重试和推送页面。 |
| DevRail Angular 功能页 | 未实现 | 当前 Angular 页面仍是 arc-admin 基线功能，没有 `features/devrail`。 |
| MVP 自动化验收 | 未完成 | 全量工程门禁通过不代表 requirements.md 第 16 节全部条件通过。 |

## 当前不应作出的结论

- 不能说“DevRail Codex Harness MVP 已完成”；
- 不能把 `cargo flow verify --all` 的通过解释为产品功能验收通过；
- 不能把审计工具的 run/step 任务当作 Codex Agent run、thread 或 turn；
- 不能把 arc-admin 的通知、审计或认证能力描述为 DevRail 的 outbox/Web Push/Harness 闭环。

## 后续实现顺序

应按 [requirements.md](requirements.md) 的迭代计划推进：

1. Phase 0：DevRail 权限、OpenAPI 骨架、业务 migration、项目/仓库/环境/任务 CRUD；
2. Phase 1：Harness Supervisor、run 状态机、事件持久化、SSE、审批、中断/恢复和变更集；
3. Phase 2：Transactional outbox、站内通知、Web Push、设备/偏好、重试和投递审计；
4. Phase 3：评论、提及、审查、补丁导出和可选 Git 平台集成。

每个阶段完成后，都必须同步需求、API、数据模型、权限、UI 和测试状态；只有 requirements.md 第 16 节全部满足后，才能将 MVP 标记为完成。
