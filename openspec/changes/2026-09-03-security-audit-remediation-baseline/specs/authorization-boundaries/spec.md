## Purpose

为组织范围内的成员、负责人、MFA、代理地址和机器端点建立不可绕过的授权与失败关闭契约，防止跨范围枚举、验证码重放和配置缺失导致的安全降级。

## ADDED Requirements

### Requirement: Authorization mutations enforce data scope

系统 MUST 在成员添加、任务负责人设置和评审人设置时，于服务端和持久化查询条件中同时校验组织、部门和所有者范围。范围外对象 MUST 返回与资源不存在等价的安全结果，不得通过状态码、响应字段或错误差异泄露对象存在性。

#### Scenario: In-scope member is added

- **WHEN** 授权操作者添加同组织且在其数据范围内的用户到项目
- **THEN** 系统创建成员关系并返回不含范围外身份信息的成功响应

#### Scenario: Out-of-scope member is added

- **WHEN** 操作者尝试添加其他组织、不可见部门或范围外用户
- **THEN** 系统拒绝操作并返回与用户不存在等价的安全结果，不写入成员关系

#### Scenario: Out-of-scope assignee or reviewer is selected

- **WHEN** 创建任务或评审时指定的负责人/评审人不属于操作者可见范围
- **THEN** 系统拒绝创建或更新，不保存该跨范围身份

### Requirement: TOTP replay protection is purpose-scoped

系统 MUST 对登录、TOTP 注册和重新认证分别维护验证码消费状态。一个用途成功消费的时间步 MUST NOT 使另一个用途失效或被错误视为已使用；同一挑战的并发提交最多一个成功，失败提交不得创建会话、完成注册或重复写入成功审计。

#### Scenario: Login TOTP is accepted once

- **WHEN** 有效登录挑战提交一个当前允许的 TOTP 时间步
- **THEN** 系统至多接受该挑战一次并创建一个登录会话，随后相同挑战的重放被拒绝

#### Scenario: Enrollment TOTP does not consume reauthentication state

- **WHEN** 有效注册挑战提交一个当前允许的 TOTP 时间步
- **THEN** 系统完成注册流程，但不改变重新认证用途的消费状态

#### Scenario: Concurrent TOTP submissions race

- **WHEN** 两个请求并发提交同一挑战和同一验证码
- **THEN** 只有一个请求成功，另一个请求收到安全失败结果且不产生第二次副作用

### Requirement: Proxy client identity fails closed without collapsing users

系统 MUST 仅在请求来自可信代理时解析 `X-Forwarded-For`。畸形、超长或不可解析的链 MUST 回落到最右侧可解析的不可信地址；不得将代理自身地址作为所有用户的共享身份，且不得允许不可信 peer 伪造来源地址。

#### Scenario: Trusted proxy supplies a valid chain

- **WHEN** 请求来自可信代理且转发链包含可解析的客户端地址和可信跳
- **THEN** 系统使用最近的不可信地址作为客户端身份

#### Scenario: Trusted proxy supplies a malformed chain

- **WHEN** 请求来自可信代理但转发链包含畸形、超长或不可解析值
- **THEN** 系统选择最右侧可解析的不可信地址；若不存在则使用安全的代理身份，并保持限流隔离语义

#### Scenario: Untrusted peer supplies forwarding headers

- **WHEN** 请求直接来自不可信 peer 且携带任意转发头
- **THEN** 系统忽略转发头并使用 peer 地址，不接受伪造的客户端身份

### Requirement: Machine endpoint secrets are explicit and non-empty

系统 MUST 在配置契约中声明 Webhook 和 repair 回调所需的密钥变量。示例配置只能包含变量名和空占位符；生产部署 MUST 通过强制变量或 secrets 注入非空值。空值、缺失值或仅空白值 MUST 使对应端点失败关闭，不得生成可计算的默认签名。

#### Scenario: Production endpoint has a valid secret

- **WHEN** 生产部署注入非空机器端点密钥并收到签名请求
- **THEN** 系统按该密钥验证请求，并继续执行已有的来源、范围和幂等校验

#### Scenario: Machine endpoint secret is missing or empty

- **WHEN** 密钥缺失、为空或只包含空白字符
- **THEN** 对应端点拒绝请求，配置检查报告缺失，且系统不接受任何由空密钥生成的签名
