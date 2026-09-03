## Purpose

为 DevRail 前端的事件流、内容安全策略、服务端可控 URL 与错误展示建立浏览器可观察的安全边界，确保组件销毁、异常响应和非标准输入不会造成连接泄漏、脚本执行或内部信息暴露。

## ADDED Requirements

### Requirement: Event streams respect component lifecycle

前端 MUST 在组件销毁时关闭当前事件流、清除所有待执行的重连任务并禁止创建新连接。任务和 run 事件流 MUST 在连接异常后按可测试策略重连或明确显示不可用状态；代理超时不得成为唯一恢复机制。

#### Scenario: Run page is destroyed during reconnect delay

- **WHEN** run 页面在事件流错误后、重连定时器触发前被销毁
- **THEN** 系统清除定时器，不创建新的事件流，也不向已销毁组件发起请求

#### Scenario: Task event stream disconnects

- **WHEN** 任务详情事件流因网络或代理关闭而断开
- **THEN** 页面按固定退避重连或显示明确的实时更新不可用状态，不能静默永久停止

### Requirement: Browser output uses safe policy-compatible values

前端 MUST 使主题初始化逻辑符合生产 CSP。所有服务端可控下载 URL、文件名和应用内深链接 MUST 在进入 DOM 或路由前通过共享白名单校验；不符合协议、来源、路由形状或字符集约束的值 MUST 被拒绝并显示安全错误。

#### Scenario: Production theme initialization runs under CSP

- **WHEN** 用户在启用生产 CSP 的页面上加载应用
- **THEN** 主题初始化不会触发 CSP 违规，且运行时应用名称按配置更新

#### Scenario: Unsafe artifact URL is returned

- **WHEN** 服务端返回非允许协议或非允许来源的下载 URL
- **THEN** 前端不创建可点击的危险链接，并显示通用错误提示

#### Scenario: Notification deep link is malformed

- **WHEN** 通知包含未知路由、外部 URL 或不符合已知参数形状的 deep link
- **THEN** 前端拒绝导航并保留通知可见，不把用户导向未授权位置

### Requirement: Internal errors are not exposed verbatim

5xx 响应 MUST 向用户展示通用简体中文错误文案，并仅保留经过约束的 trace ID。服务端内部错误消息、文件路径、数据库细节和敏感标识 MUST NOT 逐字显示在 snackbar、错误横幅或页面正文中。

#### Scenario: Backend returns an internal server error

- **WHEN** API 返回 5xx 且响应包含内部错误详情
- **THEN** 页面只显示通用错误文案和安全 trace ID，不显示原始内部详情

#### Scenario: Validation error is user-actionable

- **WHEN** API 返回可安全展示的 4xx 校验错误
- **THEN** 页面显示经过长度和字符约束的用户提示，不执行或解释其中的标记内容
