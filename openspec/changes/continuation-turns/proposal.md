## Why

当前 `thread/resume` 只用于同 attempt 传输恢复和终态 retry，缺少在质量门禁、审查意见或用户追加上下文后创建同 thread 新 turn 的独立业务语义。如果继续复用 retry，会混淆原 run 终态、attempt 谱系、workspace 清理和幂等边界，无法形成 Symphony 要求的可审计 continuation 闭环。

## What Changes

- 引入受数据范围限制的 continuation 请求事实，保存来源 run/turn、触发类型、脱敏原因、幂等键、状态和结果 child run。
- 将 continuation 定义为同一 Codex thread 上的新 turn 与新 child run；原 run 保持不可变终态，传输恢复、retry、continuation 和 follow-up 保持可区分谱系。
- 支持授权用户追加上下文、质量门禁失败和审查要求修改三类受控触发，并对数量、链深、活动 run、任务状态和重放进行确定性限制。
- 扩展 TaskTracker 状态历史和 Orchestrator reconciliation，使待派发 continuation 在重启、并发、取消和重复事件下只创建一个 child run。
- 为 continuation 按不可变快照、受控分支/变更集和基础提交重建新 workspace；不复用已清理或 cleanup failed 的路径。
- 增加 continuation 创建/查询/取消 API、OpenAPI/Angular 客户端、任务/运行详情谱系和追加上下文操作，以及脱敏事件、审计、指标和 outbox 通知。
- 记录 continuation 与 retry/follow-up/workspace 的架构决策；保持自动代码修复、自动合并和外部 Tracker adapter 在本 change 范围外。

## Capabilities

### New Capabilities

- `continuation-turns`: 同 thread 新 turn 的请求事实、谱系、幂等、触发、权限、调度、恢复和用户可见性。

### Modified Capabilities

- `task-tracker`: 任务在 continuation 请求、派发和终态之间的合法状态投影与历史。
- `symphony-orchestrator`: 待处理 continuation 的幂等 claim、child run 派发、重启对账和终态处理。
- `task-workspace-manager`: continuation child run 的可复现 workspace 重建、证据保留与清理竞态。

## Impact

- 后端新增 continuation 数据迁移、Repository、Service、Handler、权限与路由，并扩展 run/task/workspace 模型、Harness Supervisor、TaskTracker 和 scheduler reconciliation。
- Rust DTO 与 `utoipa` schema 扩展后重新生成 `docs/openapi.json` 和 Angular client；任务与运行详情页增加 continuation 谱系、状态和简体中文操作。
- 增加 PostgreSQL 并发/重放、受控假 app-server、workspace 重建/清理竞态、跨组织拒绝、重启恢复、Angular/Vitest/Playwright 和全链路验收。
- 新增 ADR 并同步总需求、Symphony 证据矩阵、架构、运维手册和实现状态。
