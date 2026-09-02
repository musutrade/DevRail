# ADR-0008：Codex app-server 协议顺序与兼容边界

- 状态：已接受
- 日期：2026-09-02
- 决策人：DevRail 项目维护者
- 关联需求：[DevRail 需求](../requirements.md#64-codex-harness-接入)
- 关联架构：[调度器的稳定幂等与对账语义](ADR-0002-scheduler-idempotency-and-reconciliation.md)

## 背景

DevRail 的 Supervisor 原先只等待一条宽松的初始化响应，使用旧版 `clientName` / `clientVersion` 参数，并在未取得 thread ID 时连续发送 `thread/start` 与字符串形式的 `turn/start`。该行为不能满足本机 Codex `0.152.1` app-server 的协议：初始化使用 `clientInfo`，响应后需要 `initialized` 通知；thread 请求必须先完成，随后才能以结构化用户输入和实际 thread ID 启动 turn。新版审批还以带顶层 JSON-RPC id 的服务器请求发送，客户端必须回写同一个 id。

## 决策

1. Supervisor 严格按 `initialize → initialized → thread/start|thread/resume → turn/start` 顺序启动。初始化与 thread 响应分别设置 10 秒上限；响应缺失、错误或结构无效时终止子进程并记录明确失败。
2. `initialize` 使用 `clientInfo`；`turn/start` 使用结构化 `text` 输入并绑定 thread 响应返回的 ID。thread、turn 与 app-server `userAgent` 在收到协议消息时持久化。
3. `turn/completed` 是 run 终态的协议依据。Supervisor 将 `completed`、`interrupted`、`failed` 映射为后端终态，关闭 stdin，并在优雅等待超时后强制终止子进程。
4. 中断请求携带活动 thread/turn ID。新版审批服务器请求的 JSON-RPC id 编码为审批幂等键，批准、拒绝、撤回或过期时以同一 id 返回 `accept`、`decline` 或 `cancel`；历史事件继续保留旧 `approval/resolve` 后备路径。
5. app-server 通知按公开、脱敏的事件类型持久化。thread 状态通知属于生命周期事件，不得误分类为文件变更。

## 取舍与后果

- 顺序握手消除了 thread ID 为空、turn 输入结构错误和成功进程被误判为传输 EOF 的问题。
- 协议版本变化现在会在明确的握手边界失败，而不是让 run 静默悬挂；升级 Codex 时仍需重新生成 schema 并运行真实协议演练。
- 审批幂等键同时承担协议关联标识；其公开 API 字段不包含命令原文，事件 payload 继续经过既有脱敏器。
- 本决策不声称完成真实设备、供应商回调、生产恢复或全部 MVP 验收。

## 验收条件

- 协议请求形状、响应顺序、终态映射、JSON-RPC 审批 id 和事件分类有自动化测试。
- 受控假 app-server 的 workflow、repair、stall、断流和超时场景通过。
- 在隔离 PostgreSQL 与受控 workspace 中使用实际 Codex app-server 完成最小 run，持久化 harness/thread/turn 元数据，收到 `turn/completed` 并完成 workspace cleanup。
- `cargo flow verify --all` 和 PR CI 通过。
