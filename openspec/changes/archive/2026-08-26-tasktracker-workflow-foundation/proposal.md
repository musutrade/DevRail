## Why

当前 Symphony 调度器已经具备可靠的 PostgreSQL claim、恢复与对账能力，但控制循环仍直接依赖 DevRail 数据访问实现，也没有可版本化、可校验、可审计的仓库级工作流契约。现在需要先建立 TaskTracker 边界和 WORKFLOW.md 基础，才能在不扩大调度器耦合与安全风险的前提下继续实现 DAG、workspace 和 continuation。

## What Changes

- 引入 TaskTracker 抽象，并提供保留组织、部门、所有者数据范围的 DevRail PostgreSQL 实现。
- 让 Orchestrator 通过 TaskTracker 读取可调度任务、查询状态、追加历史并更新调度元数据，不直接依赖具体任务 SQL 表。
- 在任务进入 queued 时形成包含目标、仓库、环境、工作流版本和验收标准的不可变调度快照。
- 支持仓库根目录 WORKFLOW.md 的 YAML front matter 与 Markdown 正文加载、严格校验和安全策略约束。
- 为每个新 run 持久化实际使用的 workflow 来源、版本、内容摘要和解析快照；运行中的 run 不受文件变化影响。
- 支持面向新入队任务的动态 reload；非法新配置保留上一有效版本，并产生脱敏告警和审计事件。
- 增加迁移、Repository、Service/worker 集成测试、运维说明和实现状态证据。
- 本变更不实现外部 issue tracker adapter、DAG、per-task workspace、continuation turns 或自动修复 run。

## Capabilities

### New Capabilities

- `task-tracker`: 定义调度器与任务存储之间的接口、DevRail PostgreSQL tracker、任务快照及可审计状态转换。
- `workflow-loader`: 定义 WORKFLOW.md 的严格加载、安全校验、run 快照和动态 reload 行为。

### Modified Capabilities

无。

## Impact

- 后端调度器、任务/run Repository 与模型、配置加载和审计事件将发生变化，并可能新增数据库迁移。
- 仓库将新增 WORKFLOW.md 契约及其运维说明；现有未提供该文件的仓库继续使用受控默认 workflow，不产生破坏性兼容变化。
- 不新增浏览器直连 Harness、外部 tracker、Redis 或 Kubernetes 依赖，不放宽审批、脱敏、网络和受控工作区边界。
