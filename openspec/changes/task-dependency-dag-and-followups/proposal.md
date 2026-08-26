## Why

TaskTracker 与不可变 workflow 快照已经稳定了单任务派发，但任务之间仍不能表达前置关系，Agent 也无法通过受控运行时接口创建可追溯的后续任务。缺少 DAG 会让依赖未完成的任务被过早派发，并迫使用户在系统外维护拆分顺序和失败传播。

## What Changes

- 新增带组织、部门、所有者范围的任务依赖模型，支持事务内环检测、幂等写入和依赖关系查询。
- 为依赖定义 `success`、`failure`、`cancelled` 与超时传播策略，使下游任务进入明确的等待、跳过或失败状态。
- 将依赖满足条件加入 TaskTracker 的候选 SQL 和 claim 事务，保持优先级 aging、多 worker 与活动 run 约束。
- 提供受控的 Agent 后续任务提议 API，执行 schema、权限、范围、配额和幂等校验；Agent 不直接写数据库。
- 在任务详情 API、OpenAPI 和 Angular 页面中展示前置任务、下游任务、阻塞原因和创建来源。
- 为依赖变化、自动传播和后续任务创建增加 System Actor 审计、事件、低基数指标和 SSE 更新。
- 增加迁移、Repository/Service/Handler、真实 PostgreSQL 并发测试、前端测试、运维与需求证据。
- 本变更不实现 per-task workspace/worktree、continuation turns、外部 tracker adapter 或质量门禁自动修复 run。

## Capabilities

### New Capabilities

- `task-dependency-dag`: 定义同组织任务依赖、环检测、终态传播、后续任务提议、幂等与可追溯查询。

### Modified Capabilities

- `task-tracker`: 将依赖满足和阻塞原因纳入可派发候选、claim 与等待诊断契约。
- `symphony-orchestrator`: 在 reconciliation 和终态处理中传播依赖结果，并保证重复事件不会重复创建后续任务或重复传播状态。

## Impact

- 后端任务模型、数据库迁移、Repository、TaskTracker、调度 reconciliation、Service/Handler、审计、事件和指标将发生变化。
- 任务详情 API/OpenAPI 和 Angular 任务详情页将新增依赖图与后续任务信息。
- 候选 SQL 将增加依赖满足条件，但保持现有组织/部门/所有者数据范围、`SKIP LOCKED`、稳定 attempt 和优先级 aging 语义。
- 不引入 Redis、图数据库、Kubernetes 或浏览器直连 Harness；PostgreSQL 继续作为事务、环检测和幂等事实来源。
