# ADR-0001：DevRail 内嵌 Symphony 控制平面

- 状态：Accepted
- 日期：2026-08-26
- 决策人：DevRail 项目维护者
- 关联需求：[SY-TRACK-001](../symphony-devrail-requirements.md#51-tasktracker-与任务生命周期p1)、[SY-ORCH-001](../symphony-devrail-requirements.md#53-orchestrator-调度循环p0)、[SY-SEC-001](../symphony-devrail-requirements.md#7-安全权限与数据治理)

## 背景

DevRail 已拥有 PostgreSQL 任务、run、事件、审批、通知和 Harness Supervisor。引入 Symphony 的目标是补齐任务驱动的编排、恢复和工作流能力，而不是再部署一个与 DevRail 平行的服务。额外控制平面会重复权限、审计、数据范围和运行状态，增加一致性风险。

## 决策

DevRail 在现有单体控制平面内实现 Symphony 能力：

1. `TaskTracker` 第一实现使用 DevRail PostgreSQL；Orchestrator 通过抽象接口访问任务，不直接绑定具体外部 tracker。
2. 只有后端 Orchestrator/Harness Supervisor 能创建和管理 Agent run；浏览器只调用 DevRail API 和 SSE。
3. 任务、run、workspace、审批、changeset、质量门禁和通知继续使用 DevRail 的组织/部门/所有者数据范围与审计边界。
4. 外部 issue tracker adapter 作为后续扩展，不作为 MVP 的运行时前置条件。

## 取舍与后果

### 正面影响

- 复用现有认证、RBAC、事务、outbox、审计和 CI，减少重复基础设施。
- 任务到 Agent、质量门禁和 PR 的链路可在一个数据库和审计模型内追踪。
- 不需要 Redis、Kubernetes 或独立 Symphony 服务即可完成第一阶段。

### 代价与风险

- 初期扩展性受单体 worker 和 PostgreSQL 容量约束，必须使用有界并发、claim 租约和背压。
- 将来接入外部 tracker 时必须实现 adapter、事件去重和权限映射，不能绕过 DevRail Task。
- Orchestrator 与 Supervisor 的接口需要稳定契约和集成测试，避免把调度逻辑泄漏到 Handler。

## 约束

- SQL 写操作只能位于 Repository、migration、测试或 seed 层。
- 自动调度使用明确的 Scheduler/System Actor，不借用普通用户会话。
- Agent 运行默认网络关闭，工作区位于受控根目录，高风险动作必须审批。
- 需求、ADR、OpenSpec change、DevRail Task、测试和 PR 必须通过稳定 ID 关联。

## 关联实现

本 ADR 的第一批实现由 `symphony-orchestrator-reconciliation` OpenSpec change 跟踪。

- 控制循环：`backend/src/workers/task_scheduler.rs`。
- Agent 进程边界：`backend/src/workers/harness_supervisor.rs`。
- 任务/run/审计事实：`backend/src/repositories/devrail.rs`、`backend/src/repositories/devrail_runs.rs`、`backend/src/repositories/audit_logs.rs`。
- 迁移：`backend/migrations/20260826030000_add_symphony_scheduler_reliability.sql`。
- 运维：[Symphony Orchestrator 运行手册](../symphony-orchestrator-operations.md)。
- 验收：[P0 证据矩阵](../symphony-devrail-requirements.md#124-p0-调度可靠性证据矩阵2026-08-26)。

本批实现不引入外部 tracker、Redis、Kubernetes 或独立 Symphony 服务，未改变本 ADR 的控制平面边界。
