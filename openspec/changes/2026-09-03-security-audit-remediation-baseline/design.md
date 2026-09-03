## Context

本变更横跨 repair scheduler、MFA challenge、授权查询、供应链 workflow、前端事件流和审计证据。现有系统已经具备：

- repair gate rerun 的持久化状态、claim owner/token、过期回收和条件完成；
- 重新认证 TOTP 的单调消费字段；
- DevRail 的组织/部门/所有者数据范围助手；
- CodeQL、dependency review、RustSec、Trivy 和 SBOM 工作流；
- run/task 页面事件流、生产 CSP 和运行时 API 基址；
- OpenSpec、ADR、审计报告和 CI artifact 目录。

当前缺口不是缺少单个 API，而是多个边界的仲裁方式不一致：长时间门禁执行可能失去 claim、不同 MFA 用途共享不清晰、CI 条件与实际执行可能脱节，前端资源和证据生命周期也没有统一的不变量。实现必须遵守仓库的分层、SQL 写入位置、数据范围、脱敏、迁移只追加和“先 scope 再 verify”约束。

## Goals / Non-Goals

**Goals:**

- 使每个 repair gate rerun 在长命令执行期间保持单一、可续租、可接管的数据库执行权。
- 在不改变现有重新认证行为的前提下，为登录和 TOTP 注册建立用途分域的防重放状态。
- 将组织/部门/所有者范围和失败关闭行为扩展到复核中列出的授权边界。
- 使供应链身份、扫描触发、风险接受到期和门禁条件都能由机器验证。
- 使浏览器连接、CSP、服务端可控输出和错误消息满足可测试的安全契约。
- 使审计和验收证据可从干净 clone 或稳定 CI artifact 重建，并能追踪到单一 OpenSpec change。

**Non-Goals:**

- 不重写 ADR-0009 或原始安全审计的历史结论。
- 不在本变更中改变 DevRail 的业务状态机、公开 API 版本或仓库可见性。
- 不把本地协议演练替代供应商、生产、真实浏览器或 MVP 验收。
- 不通过删除 required check、降低 lint/扫描等级、扩大 ignore 或放宽工作区边界解决失败。

## Decisions

### 1. Repair gate 使用逐条 lease、可中止执行和条件终态

每个被领取的 gate rerun 使用独立的 owner/token/expiry 身份。claim 批次可以继续批量读取，但不能让多个 rerun 共享不可区分的完成权限。执行 worker 为每个命令建立 keepalive 生命周期：命令运行、租约续期和取消信号并行观察。

默认租约保持短于最长命令超时，以保留故障接管能力；续期周期固定为当前租约的三分之一，并设置最小/最大边界。续租请求只匹配当前 owner/token 且要求旧租约仍未过期。若续租失败、数据库连接不可用或收到取消信号，worker 立即停止将命令视为可接受结果，终止子进程，并只允许以原 token 尝试记录“失去所有权”的安全结果。

完成、释放、过期回收和接管全部依赖 `id + owner + token` 条件。完成返回零行时视为竞争结果而不是异常覆盖；接管只能从已过期的 `running` 状态开始。任务投影与人工交接必须在持有相同事务锁的情况下执行，避免旧 owner 先完成 gate、后覆盖新 owner 的 repair 状态。

选择逐条 lease 而不是一次性延长到 900 秒，是为了同时满足长任务不重复执行和 worker 丢失后可恢复。选择 worker 侧 keepalive 而不是数据库触发器，是因为租约有效性依赖进程是否仍实际运行，数据库无法安全判断外部命令是否存活。

### 2. TOTP 状态按 challenge kind 分域

现有 `last_reauth_totp_counter` 继续只服务重新认证。登录与注册挑战的防重放状态绑定在 `auth_mfa_challenges` 记录上，优先使用 challenge 内的消费字段；只有在字段无法表达用途或并发约束时才新增按用途分域的表。

验证流程在同一事务中锁定 challenge，解析当前允许的时间步，执行一次条件消费，再提交会话创建或注册完成事实。challenge 的 `kind`、过期时间、消费时间和计数器必须共同参与条件更新。登录、注册和重新认证使用不同的状态命名空间，不能以共享的“最近验证码”字段互相阻塞。

选择 challenge 绑定而不是扩大 `user_mfa_settings` 的全局计数器，是为了避免一次登录尝试使正在进行的重新认证或注册失效，也避免不同 challenge 之间因同一 TOTP 时间步产生错误冲突。

### 3. 授权修复使用现有范围谓词，测试验证响应不可枚举

成员、负责人和评审人的校验在 Service 层先做业务输入检查，在 Repository SQL 中再次使用组织、部门和所有者范围。范围外对象不依赖调用方是否知道其 ID，统一映射为安全的 not-found/forbidden 结果；成功响应只返回调用方已授权的身份字段。

代理地址解析保持“只有可信 peer 才解析转发头”的前提。解析失败时从右向左寻找最后一个可解析的不可信地址；若链中不存在这样的地址才退回 peer。该规则通过单元测试固定，并用限流集成测试证明畸形头不会把所有用户合并到一个代理桶。

机器端点密钥只由配置/secret 注入，示例文件保持空占位符。启动或端点处理在空白值时失败关闭；不得通过空密钥生成 HMAC。配置校验和运行时校验分别覆盖部署错误与热路径错误。

### 4. 供应链门禁采用允许映射、触发矩阵和独立日期检查

Action 固定为完整 commit SHA，供应链脚本维护“action 身份 → 允许 SHA”的显式映射，并拒绝 tag、分支和未知 SHA。镜像使用 digest；若构建工具必须从 tag 解析，则将解析出的 digest 作为构建证据并在扫描阶段校验一致性。

workflow 触发矩阵按事件拆分：pull request 检查变更风险，默认分支 push 验证合并结果，schedule 无条件刷新依赖与镜像安全状态。路径过滤只用于减少不相关 job，不得让 schedule 变成仅比较 tip commit 的空扫描。

RUSTSEC 风险接受检查独立于 cargo-deny 的 ignore 解析：读取结构化接受记录，按 UTC 当前时间比较到期时刻，并在过期且 ignore 仍存在时返回非零状态。这样既保留当前有证据的短期接受，也确保到期日成为机器门槛。

### 5. 前端采用显式资源生命周期和共享安全输出函数

SSE 连接由页面持有明确的 connection state、timer handle 和 destroyed 标志。重连调度、连接关闭和组件销毁统一经过一个生命周期函数；测试同时覆盖正常断线、销毁窗口和重复错误回调。REST API 的运行时基址与 SSE 共用同一配置来源。

主题初始化逻辑移入 Angular 启动代码，避免在生产 CSP 中维护内联脚本。服务端返回的 URL、文件名和 deep link 在进入 DOM/Router 前经过共享校验器：协议、来源、路由形状、字符集和长度均有固定规则。错误展示层按状态码分流，5xx 只输出通用文案和受约束 trace ID。

选择移除内联主题脚本而不是为其生成 nonce，是为了让静态资源缓存、反向代理和本地预览不依赖每响应动态 nonce；如果未来必须引入内联脚本，须另开安全变更并重新定义 CSP 生成链路。

### 6. 证据以“来源文件 + 命令 + artifact + 追踪 ID”组成

审计、复核和验收文档继续作为时间点记录，不在原文覆盖历史状态。每个整改 OpenSpec change 必须提供追踪 ID，并在任务、PR、测试和 CI artifact 中重复该 ID。

门禁摘要提交到版本控制目录，详细日志保留在具备永久定位的 CI artifact。被 `.gitignore` 排除的本机报告可作为调试输入，但不能作为唯一关闭证据。文档一致性检查对“已通过/已完成”声明要求同仓库源文件或稳定 artifact 反向定位。

## Risks / Trade-offs

- [Risk] worker 在数据库短暂不可用时误判为失去 lease，终止一个实际健康的门禁。→ [Mitigation] 续租失败先进入短暂 grace window；只有超过 grace 且 token 条件无法恢复时才终止，所有接管和终止原因写入低敏感度事件。
- [Risk] 逐条 lease 增加 scheduler 查询和更新次数。→ [Mitigation] claim 仍批量读取，续租按固定低频执行，并记录 lease 冲突/续租失败指标。
- [Risk] TOTP challenge migration 遗留旧记录或默认值处理不当。→ [Mitigation] additive migration、空库/已有库演练、过期 challenge 清理和并发回归；重新认证字段保持独立。
- [Risk] 固定 SHA/digest 增加依赖升级维护成本。→ [Mitigation] 提供集中映射、升级任务模板和 Dependabot 更新流程，禁止手工改动导致门禁失配。
- [Risk] schedule 全量扫描增加运行时间和供应商 API 调用量。→ [Mitigation] 只复用已有 job，限制并发，保留 artifact，按失败类型重试而不静默跳过。
- [Risk] 严格 URL/deep link 白名单拒绝历史非标准数据。→ [Mitigation] 记录安全拒绝原因、提供迁移统计，并先在只读诊断中盘点现存值。
- [Risk] 将证据提交到仓库可能增加文档噪声。→ [Mitigation] 提交短摘要和机器可读索引，详细日志使用稳定 CI artifact，不提交秘密和完整运行输出。

## Migration Plan

1. 先提交 OpenSpec 规划与追踪 ID，不改业务代码；评审确认本 design 与各 capability spec 一致。
2. 按独立变更实施：repair lease、MFA/授权、供应链、前端、证据链。每个变更先执行 `cargo flow scope`，仅在对应组件范围内修改。
3. 数据库变更只使用 additive migration；先在空库执行，再在脱敏生产快照或等价规模数据上验证锁、时长和回滚前置条件。
4. 逐项部署时保留兼容窗口：旧 challenge 记录可安全完成或过期；旧 action/tag 配置在门禁切换前同时更新检查脚本；前端先发布兼容的安全拒绝提示再收紧白名单。
5. 若任一变更失败，回滚应用代码到上一版本并保留已追加的迁移；通过兼容读取忽略新字段，禁止删除已应用 migration 或直接修改业务终态。
6. 每个变更完成后上传可复核 artifact，并更新本 ADR 的追踪矩阵；全部条目完成后，以新的 `main` 运行一次完整安全复核，再决定是否将 ADR 更新为 Accepted。

## Open Questions

- 具体的 lease grace window 秒数、指标名称和 CI artifact 保留期可在实现变更中确定，不改变本设计的单 owner、续租和失败关闭语义。
- TOTP challenge 消费字段采用现有表扩展还是独立按用途表，可在数据库设计评审中确定；必须满足本 ADR 的用途分域和并发条件。
