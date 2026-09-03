# ADR-0010：安全复核遗留加固的执行与验收基线

- 状态：Proposed
- 日期：2026-09-03
- 决策人：DevRail 项目维护者
- 关联 ADR：[ADR-0009 安全边界与终态并发保护](ADR-0009-devrail-security-boundaries.md)
- 关联审计：[2026-09-01 全项目审计](../verification/security-audit-2026-09-01.md)
- 关联复核：[2026-09-03 安全审计复核](../verification/security-audit-followup-2026-09-03.md)

## 背景

ADR-0009 对应的安全边界修复已随 PR #94 合并，并通过了合并提交
`870fa2aeb3b2d728ad940add96b56264a5c6d51b` 的远端 required checks。
2026-09-03 的复核确认：P1 第 1、3、4、5、7 项已修复；第 2 项已转为
带日期的风险接受；第 6 项只剩 repair 门禁重跑租约未闭合。P2 仍有授权、
供应链、前端和证据链遗留问题。

本 ADR 只固化跨变更的架构不变量、选型和验收规则。每个具体遗留项由独立
OpenSpec change 实现和关闭；本 ADR 不把审计历史记录改写为“已修复”，也不
把任何 MVP 验收项直接置为通过。

## 决策

### 1. Repair 门禁重跑采用“续租 + token 绑定”的单一租约方案

Repair 门禁重跑统一采用数据库持久化租约，不采用“把租约一次性延长到
命令超时以上”的方案。

- `claim_gate_reruns` 继续以 `claim_token` 抢占记录；worker 在执行
  `execute_gate_rerun` 的 900 秒命令期间，按固定间隔续租
  `renew_gate_rerun_claim`。续租间隔必须小于租约时长的一半，并受
  `lease_seconds` 的配置上限约束。
- 续租失败视为失去所有权：worker 必须停止接受结果写入，尽快终止正在执行
  的门禁进程，并通过 `claim_token` 守卫完成或释放，不能无条件回写终态。
- `complete_gate_rerun`、`release_gate_rerun_claim` 和接管路径都必须匹配
  `id + claim_owner + claim_token`；`rows_affected` 为零时只记录竞争结果，
  不得覆盖新 worker 的结果。
- 调度 tick 不得在持有有效续租的 worker 上执行过期回收；过期回收只允许
  接管确已失效的 claim。一个 rerun 在任一时刻最多只能有一个有效 owner，
  同一 `cwd` 不得并发执行同一门禁。
- Harness 启动、审批恢复和直接质量门禁执行继续以数据库 claim/lease 为
  唯一仲裁点；进程内 controls map 只保存已成功抢占的唯一控制通道。

### 2. TOTP 防重放按用途分域，不复用重新认证字段

登录和 TOTP 注册不直接复用 `user_mfa_settings.last_reauth_totp_counter`。
该字段继续只表示已成功消费的重新认证验证码。新增的登录/注册防重放状态
必须绑定到 `auth_mfa_challenges` 的挑战用途和生命周期：

- 在锁定的 challenge 事务内解析验证码对应的时间步；
- 仅当 challenge 尚未消费、未过期且对应用途允许时，原子记录该 challenge
  已接受的时间步或等价消费标记；
- 并发提交同一 challenge 或同一时间步时，最多一个请求成功；失败请求不能
  创建会话、完成注册或重复写入成功审计；
- 登录、注册、重新认证三种用途的计数器/消费状态不得互相阻塞，必须分别
  具有测试覆盖。

如果实现选择新增列或独立表，必须使用 additive migration，并明确旧挑战数据
的默认值、回滚策略和清理策略；不得改变现有升权重新认证的语义。

### 3. 授权与数据范围修复以 SQL 谓词和真实数据库测试为准

以下规则是不可绕过的不变量：

- `add_member` 使用与 `list`/`revoke` 相同的数据范围语义；成员用户关联和
  返回值都必须校验组织边界，组织外对象统一返回不可枚举的 not-found/forbidden
  结果。
- `assignee_user_id` 与 `reviewer_user_id` 在任务/评审创建时按 actor 的组织、
  部门和所有者范围校验；前端隐藏不是安全控制。
- 畸形、超长或不可解析的 `X-Forwarded-For` 链从最右侧可解析的不可信跳回落，
  不能把所有用户归因到代理 `peer_ip`；只有可信代理 peer 才能启用该解析。
- Webhook 与 repair 回调密钥在 `.env.example` 中只记录变量名和空占位符，
  生产 compose 使用 `${VAR:?}` 或 Compose secrets 强制注入；禁止空值和真实
  密钥进入示例、仓库、日志或请求。
- 每一项授权修复必须包含至少一个跨组织/跨部门拒绝测试、一个同范围成功测试，
  并使用隔离 PostgreSQL 验证 SQL 结果与响应语义。

### 4. 供应链门禁采用固定引用、完整触发和到期强制

- 第三方 Action 必须固定到完整 commit SHA；`scripts/check-supply-chain.mjs`
  只能校验允许的 SHA 映射及其 action 身份，不能要求可变 tag。
- 生产基础镜像和运行时镜像固定 digest。若暂时需要 tag，必须由构建步骤解析
  并锁定 digest，同时把实际 digest 写入可提交的构建证据。
- `deployment/nginx/**` 的变更必须触发镜像扫描和 SBOM；删除无匹配的
  `docker/**` 过滤项。schedule 运行必须完整扫描默认分支依赖快照，不能只依据
  tip commit 的路径差异决定是否执行。
- CodeQL、dependency review、RustSec、Trivy 和 SBOM 的门禁必须验证实际执行
  条件，不能只 grep workflow 字符串。
- `RUSTSEC-2023-0071` 的风险接受以 UTC **2026-12-31 00:00:00** 为失效
  时刻。供应链 workflow 的第一步和本地 workflow 验证都必须读取独立的接受
  记录并执行日期检查：失效后若仍存在该 ignore，检查必须失败并阻断相关 job。
  只有移除 ignore、替换依赖，或以新的 owner、证据和未来到期日重新接受，才
  能恢复通过。`.env.example`/compose 中不得通过默认值绕过该检查。

### 5. 前端安全修复采用“生命周期关闭 + 白名单输出”

- SSE 重连定时器必须保存句柄，并在组件销毁时清除；销毁标志生效后不得创建
  新的 `EventSource`。任务详情流必须具备可测试的重连策略，代理超时只能作为
  辅助措施，不能替代客户端生命周期控制。
- 主题初始化不得依赖生产 CSP 会拦截的内联脚本。实现统一选择将逻辑移入
  Angular 启动代码；如确需内联，必须使用每次响应绑定的 nonce/hash，并有
  CSP 浏览器回归。
- `downloadUrl` 只允许 `https:`（必要时加部署允许的同源路径）；下载文件名
  仅允许受限字符集和安全扩展名；通知 `deepLink` 只允许已知应用路由形状。
  所有服务端字符串在进入 DOM 前先通过共享校验函数。
- 5xx 错误只向用户展示通用简体中文文案和受约束的 trace ID；服务端内部错误
  不得逐字透传到 snackbar 或错误横幅。

### 6. 审计与门禁证据必须可提交、可复现、可追踪

- 原始审计和复核文档是不可变的时间点记录；修复状态写入复核文档、ADR、
  OpenSpec change 和 PR，不在原始审计正文内覆盖历史结论。
- 门禁证据必须存放在版本控制目录或 CI artifact 中，且至少包括命令、UTC
  执行时间、提交 SHA、范围、结果摘要和失败详情。被 `.gitignore` 排除的
  `codex-audit-pipeline/.codex/reports/test_result.md` 不能作为唯一证据。
- 每个 OpenSpec change 必须在追踪表中绑定一个或一组明确审计条目、代码范围、
  迁移、测试、门禁和关闭证据；不把不相关的 P1/P2 问题打包成一个“全部修复”。
- 文档中“已加入/已通过”只有在同仓库可复现命令或受永久链接保护的 CI artifact
  支持时才能使用；状态文档、HANDOFF、验收矩阵之间出现冲突时必须阻断发布。

## 决策追踪矩阵

| 追踪 ID | 复核条目 | 实现载体 | 最低验收证据 | 当前状态 |
| --- | --- | --- | --- | --- |
| SEC-REPAIR-001 | P1-6 repair 门禁租约 | 本 change；`task_scheduler`、`devrail_repairs`、迁移 | 两 worker 竞争、续租、失租约终止、重复 cwd 测试；PostgreSQL；`cargo flow verify --all` | Open |
| SEC-RISK-002 | P1-2 RUSTSEC 到期 | 本 change；workflow + 可执行日期检查 | 到期前通过、到期后失败、ignore 路径检查；UTC 时间测试 | Open |
| SEC-AUTH-003 | P2 授权与数据范围 | 本 change；members/MFA/auth/services/repositories | 跨组织拒绝、同范围成功、重放拒绝；隔离 PostgreSQL | Open |
| SEC-SUPPLY-004 | P2 供应链与 CI | 本 change；workflow、脚本、Dockerfile、deployment | SHA/digest 检查、schedule 执行、nginx 触发扫描；CI required checks | Open |
| SEC-FRONT-005 | P2 前端 | 本 change；SSE/CSP/URL/error UI | Chromium 桌面与移动浏览器回归；Angular tests | Open |
| SEC-EVIDENCE-006 | P2 证据链 | 本 change；报告归档与文档引用 | 干净 clone 可重建证据；文档一致性检查 | Open |

追踪矩阵是 ADR 的关闭入口：任何实现 PR 必须先更新对应 OpenSpec 和矩阵状态，
不能只提交代码后把本 ADR 改成 Accepted。

## 取舍与后果

### 正面影响

- 续租方案避免 900 秒长命令期间发生重复执行，同时保留失效 claim 的可接管性。
- TOTP 用途分域，避免登录安全修复意外改变重新认证语义。
- SQL 谓词、浏览器回归和供应链执行条件都成为可验证的发布门槛。
- 到期检查和可提交证据消除“临时 ignore”与“本机绿、仓库不可复核”的长期漂移。

### 代价与风险

- 续租需要后台定时任务、取消子进程和新的指标；网络分区或进程暂停时可能提前
  失去 lease，必须接受一次受控重试/接管。
- TOTP migration 增加状态字段或表，并需要清理已过期 challenge。
- Action SHA 和镜像 digest 降低直接跟随上游 tag 的便利性，需要配套升级流程和
  依赖机器人。
- 前端 CSP、SSE 与白名单会拒绝部分历史或非标准服务端值，必须提供明确错误和
  迁移说明。
- 仓库当前为 PUBLIC；本 ADR 不接受由公开可见性带来的额外信息暴露风险。该风险
  需要独立的可见性决策或风险接受记录。

## 非目标

- 不把原审计或复核报告的历史条目改写为“已修复”。
- 不把本 ADR、单元测试或本地演练直接当作 MVP 产品验收。
- 不在单个 PR 中关闭全部遗留项；每个 OpenSpec change 独立评审、测试和归档。
- 不通过移除 required check、降低 lint 等级、扩大 allow/ignore 范围来消除失败。

## 验收条件

### 共同门槛

- 每个追踪条目有对应 OpenSpec change、PR、变更文件清单和关闭证据。
- `cargo flow scope` 输出范围与实际变更一致；`cargo flow verify --all`、
  OpenSpec 严格校验和 PR required checks 全部成功。
- 证据包含 UTC 时间、提交 SHA、命令完整名称、结果摘要和可复核 artifact；
  不依赖被 gitignore 的本机报告。

### 按问题类型的最低门槛

- 编排/数据库：真实 PostgreSQL 竞争测试，覆盖续租、失租约、接管、终态和
  `rows_affected` 守卫；迁移在空库和已有数据上正向执行。
- 授权/MFA：跨范围拒绝、同范围成功、TOTP 同 challenge 并发重放拒绝；不改变
  重新认证计数器的既有行为。
- 供应链/CI：SHA/digest 固定检查、RUSTSEC 到期前后模拟、schedule 全量扫描、
  nginx 变更触发扫描、CodeQL/依赖审查实际执行证据。
- 前端：Angular 单元测试 + Chromium 桌面/移动浏览器回归，覆盖组件销毁、CSP、
  URL/路由白名单和 5xx 文案。
- 证据链：从干净 clone 可定位源代码、命令、UTC 时间、SHA 和结果；文档之间无
  互相矛盾的“已通过”声明。

只有在追踪矩阵全部达到“已验证”、下一轮以新 `main` 为基线的安全复核没有未记录
差异，并由维护者明确批准后，才能将本 ADR 从 Proposed 更新为 Accepted。
