# ADR-0003：仓库工作流契约、运行快照与动态加载

- 状态：Accepted
- 日期：2026-08-26
- 决策人：DevRail 项目维护者
- 关联需求：[SY-WORKFLOW-001 至 SY-WORKFLOW-005](../symphony-devrail-requirements.md#52-workflow-loader-与-workflowmdp1)

## 背景

DevRail 的调度可靠性已由数据库事实、claim 租约和 Harness Supervisor 保证，但仓库尚无稳定的 Agent 工作流契约。如果 worker 在每次执行时临时读取宽松配置，文件变化、拼写错误或恶意字段可能改变运行中行为，也无法复现某个 run 实际采用的提示、工具和质量门禁策略。

## 决策

1. 仓库可在根目录提供 `WORKFLOW.md`，由 YAML front matter 和 Markdown 提示正文组成；缺失时使用版本化的后端安全默认值。
2. loader 使用封闭 schema 和严格模板语义。未知字段、未知变量、未知过滤器、缺失必填字段和非法枚举全部拒绝，不做静默降级。
3. 解析后的 workflow 必须经过安全上限合并：仓库配置只能收紧或选择允许的能力，不能绕过组织权限、审批、脱敏、受控根目录、网络策略和全局资源上限。
4. 创建 run 时固化 workflow 来源、声明版本、内容摘要和规范化解析快照。运行中只读取该快照，不重新解释磁盘文件。
5. worker 通过带防抖的文件变更检测或等价轮询加载新版本。合法版本仅供随后进入 `queued` 的任务建立快照，并影响由这些快照创建的新 run；已经排队的任务和运行中的 run 不变。非法版本保留上一次有效配置，并记录脱敏告警、指标和 System Actor 审计事件。
6. workflow 摘要使用规范化内容计算，作为 run、事件、changeset、质量门禁和问题诊断的稳定关联信息；摘要不替代数据库主键或任务 attempt 幂等键。

## 取舍与后果

### 正面影响

- 每个 run 可复现、可审计，文件热更新不会改变已启动执行。
- 严格失败能尽早暴露仓库配置漂移，避免未知字段被误认为生效。
- 安全默认值与平台上限继续由后端控制，仓库作者不能通过提示或配置扩大权限。

### 代价与风险

- schema 演进必须有明确版本和兼容策略，新增字段不能依赖宽松解析。
- 动态加载引入缓存与文件监控状态，需要测试并发更新、半写入文件和进程重启。
- 快照增加数据库存储，需要限制正文大小、执行脱敏并规划保留策略。

## 拒绝的方案

- 不在每个 turn 开始时重新读取 `WORKFLOW.md`，避免同一 run 的策略漂移。
- 不允许未知字段或变量静默为空，也不在配置非法时自动采用部分新字段。
- 不让仓库 workflow 覆盖平台级审批、网络、数据范围、脱敏和工作区根目录限制。
- 不把 GitHub Actions 或前端状态作为 workflow 运行时事实来源。

## 关联实施

本 ADR 由已归档的 `2026-08-26-tasktracker-workflow-foundation` OpenSpec change 跟踪，已完成实现与验证：

- 迁移：[`20260904100000_add_tasktracker_workflow_foundation.sql`](../../backend/migrations/20260904100000_add_tasktracker_workflow_foundation.sql)
- 模块：[`workflow.rs`](../../backend/src/orchestration/workflow.rs)、[`task_tracker.rs`](../../backend/src/orchestration/task_tracker.rs)、[`workflow_reloader.rs`](../../backend/src/workers/workflow_reloader.rs)
- 端到端与回归测试：`queued_workflow_snapshot_reaches_harness_once_without_drift`、`reload_persists_last_known_good_and_deduplicates_failures`、`run_insert_requires_and_copies_exact_task_workflow_identity`
- 运维与使用：[Symphony Orchestrator 运行手册](../symphony-orchestrator-operations.md)、[仓库工作流契约](../workflow-contract.md)
- OpenSpec：[`2026-08-26-tasktracker-workflow-foundation`](../../openspec/changes/archive/2026-08-26-tasktracker-workflow-foundation/)
