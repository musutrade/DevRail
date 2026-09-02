# DevRail MVP 验收证据矩阵

日期：2026-09-02

## 使用规则

本矩阵对应 [总需求第 16 节](../requirements.md#16-mvp-完成定义definition-of-done) 的十项 MVP DoD。所有项目当前均为“待验收”：已有代码、专项测试或历史 PR 合并不等同于最终验收通过。

仅当同一行附有当前提交 SHA、执行日期、隔离环境/设备范围、命令或运行记录位置，且没有违反安全与数据范围约束时，才能将状态更新为“通过”。生产环境演练必须另外记录责任人、基础设施范围和恢复结果；不得在仓库写入凭据、Cookie、token、私钥、连接串或完整请求头。

## MVP DoD

| DoD | 已有基础 | 仍需形成的验收证据 | 状态 |
| --- | --- | --- | --- |
| 新建项目、仓库、环境、任务和成员全流程可用 | 各资源 API、页面、数据范围与创建入口已加入 | 在隔离 PostgreSQL 与浏览器中完成一条跨资源创建—查询—权限拒绝流程，并关联执行记录 | 待验收 |
| Agent run 生命周期可用 | Harness Supervisor、SSE、审批、中断、恢复、失败与 retry 已加入 | 受控假 app-server 和浏览器端演练覆盖启动、流式事件、审批、取消、恢复、失败和 retry | 待验收 |
| 受保护接口均有权限与数据范围校验 | Handler/Service/Repository 分层和审计门禁已存在 | 路由级权限矩阵、跨组织/部门/所有者拒绝和 SQL 范围断言的汇总记录 | 待验收 |
| ChangeSet、质量门禁和审计可追溯至 run | 任务/run 详情、changeset、质量门禁和审计基础已加入 | 一条从任务到 run、变更集、门禁日志引用和审计事件的双向追溯演练 | 待验收 |
| 站内通知可靠写入且推送失败不丢失 | transactional outbox、delivery、重试与告警基础已加入 | 故障注入证明业务事务、outbox、delivery 与用户可见通知的一致性 | 待验收 |
| Android/iOS PWA 可接收推送 | Web Push 订阅、设备管理和 dispatcher 已加入 | Android Chrome PWA 与符合条件 iOS 已安装 PWA 的真实设备验收，记录脱敏设备能力与深链接重新鉴权结果 | 待验收 |
| 设备撤销、永久失败、临时失败和幂等有测试 | 撤销、失效和重试逻辑已加入 | 模拟供应商 500/超时、404/410、worker 重启和双 worker 竞争的自动化及端到端记录 | 待验收 |
| Push payload 与深链接安全 | payload 最小化和深链接重新授权约束已实现 | secret scan、payload 断言和失效会话/越权深链接拒绝演练 | 待验收 |
| 桌面/移动 E2E、Rust 集成、OpenAPI 与全量门禁通过 | 各组件已有专项测试 | 在当前主干执行 `cargo flow verify --all`、Rust/PostgreSQL、Angular、Playwright 桌面/移动、OpenAPI 契约，并保存脱敏报告索引 | 工程门禁通过；MVP 待验收 |
| 生产配置、备份、告警、保留和恢复文档齐全 | Compose、监控、备份与审计归档文档已存在 | 真实基础设施的 TLS/密钥配置、告警送达、备份/PITR 恢复、保留与不可变证据库演练记录 | 待验收 |

## 当前验证边界

- OpenSpec 静态校验可以验证规格结构，但不能代替产品验收。
- `cargo flow verify --all` 是交付前门禁，不替代真实设备、供应商和生产恢复演练。
- 受控修复 run 的请求、诊断、child 谱系、隔离 workspace、门禁重跑和任务/run UI 工程实现已接通；CI/审查可信事件适配已通过来源验证、证据新鲜度、changeset 校验、跨范围和重复回调的真实 PostgreSQL 测试。2026-08-29 的受控假 app-server/workspace/质量门禁 E2E `workers::task_scheduler::tests::controlled_repair_fake_app_server_workspace_and_gate_e2e` 已验证单一 repair child、来源失败不可变、受控 workspace、child handoff、门禁重跑、终态重放、cleanup/rebuild 幂等及 token/authorization/command/绝对路径不落盘；这仍不替代真实设备与供应商回调、生产恢复演练。在这些证据形成前，质量门禁、CI 或外部审查失败后的自动处理不应被标记为 MVP 已验收。

## 当前门禁记录

- 执行时间：2026-09-02；范围：隔离 PostgreSQL、受控临时 workspace、本机 Codex app-server `0.152.1`；结果：DevRail task `4` 为 `succeeded`、run `4` 为 `completed`，真实 harness/thread/turn 标识已持久化，`turn/completed` 状态为 `completed`，Agent 精确返回 `DEVRAIL_REAL_CODEX_OK`，未发生工具执行，workspace cleanup 为 `completed`。另以拒绝式探针验证命令审批服务器请求使用数值 JSON-RPC id，回写同一 id 的 `decline` 后 app-server 发出 `serverRequest/resolved`；探针未批准或执行命令。该证据只覆盖本机协议链路，不将 Agent run 生命周期或整体 MVP 行更新为“通过”。

- 执行时间：2026-09-02T08:11:22Z；命令：`cargo flow verify --all`；结果：secret scan、架构审计、backend format/Clippy/tests（160 项）、frontend lint/format/tests（113 项）、桌面/移动 Playwright、真实 full-stack smoke、生产构建、OpenAPI/配置/供应链和 arc-flow（74 项）均通过，`TEST_SUMMARY: PASS`。脱敏报告：[test_result.md](../../codex-audit-pipeline/.codex/reports/test_result.md)。
- 执行时间：2026-08-29；命令：`openspec validate --all --strict`；结果：7 个 spec/change 项全部通过。repair E2E 的数据库、事件、诊断、审计、通知和 outbox 断言仅检查固定敏感标记是否缺失，不在仓库写入真实凭据或完整路径。

- 执行时间：2026-08-28T20:29:43Z；命令：`cargo flow verify --all`。
- 范围：工作区 `main` HEAD `b66e4c1`，包含未提交的受控 repair 变更；报告不是已合并提交的 CI 证明。
- 结果：secret scan、架构审计、backend format/Clippy/compile/tests、frontend lint/format/tests/Playwright/full-stack smoke/build、OpenAPI/配置/供应链和 arc-flow 检查均通过；backend/frontend/arc-flow 测试分别为 146/113/69 项，`TEST_SUMMARY: PASS`。
- 脱敏报告：[test_result.md](../../codex-audit-pipeline/.codex/reports/test_result.md)。
- `main` 合并提交 `b66e4c1` 的可读 CI 记录：[CI](https://github.com/musutrade/DevRail/actions/runs/33145794157)、[arc-flow platform](https://github.com/musutrade/DevRail/actions/runs/33145794179)、[Supply chain security](https://github.com/musutrade/DevRail/actions/runs/33145794140)。CodeQL 在该 workflow 条件下 skipped；这些记录不代表当前未提交 repair 变更已合并验证。
