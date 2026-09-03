## Why

2026-09-03 的安全审计复核确认 PR #94 已关闭主要 P1 边界问题，但 repair 门禁租约、TOTP 用途分域、剩余授权边界、供应链执行条件、前端安全边界和审计证据可追溯性仍存在缺口。当前这些缺口分散在后端、前端、CI、部署和文档中，缺少统一的可验证要求，容易再次出现“配置看似通过、实际未执行”或修复后无法复核的情况。

## What Changes

- 为 repair 门禁重跑定义续租、失租约终止、接管和 token 绑定的单一并发语义。
- 为登录、TOTP 注册和重新认证定义相互隔离的验证码防重放语义。
- 收紧成员添加、任务/评审负责人、代理地址解析和机器端点密钥配置的数据范围与失败关闭行为。
- 将第三方 Actions、基础镜像、nginx 变更、周期扫描和 RustSec 风险接受纳入可执行供应链门禁。
- 收紧 SSE 生命周期、CSP、服务端 URL/路由值和 5xx 错误消息的前端安全输出。
- 将门禁、审计和验收证据归档为可提交或可长期定位的可复核产物，并建立审计条目到 OpenSpec、测试和 PR 的追踪关系。
- 不改变原始审计文档的历史结论，不直接改变 MVP 验收状态，不通过降低门禁、扩大 ignore 或移除 required check 消除失败。

## Capabilities

### New Capabilities

- `authorization-boundaries`: 组织/部门/所有者范围、TOTP 用途分域防重放、代理地址归因和机器端点密钥配置约束。
- `supply-chain-gates`: 固定 Action/镜像身份、完整扫描触发、RustSec 风险接受到期强制和门禁实际执行条件。
- `frontend-security-boundaries`: SSE 资源生命周期、CSP 兼容启动、服务端值白名单和安全错误展示。
- `audit-evidence`: 审计条目、OpenSpec change、测试、门禁和可复核证据的版本化追踪契约。

### Modified Capabilities

- `controlled-repair-runs`: 增加 repair 门禁执行期间的可续租 claim、失租约终止、单 owner 和安全接管要求。

## Impact

- 后端：repair scheduler/repository、MFA challenge 与用户授权 service/repository、代理地址解析、Webhook/repair 回调配置校验。
- 前端：DevRail run/task SSE、主题初始化、下载链接、通知 deep link、API 错误展示。
- CI/部署：`.github/workflows/**`、供应链检查脚本、Dockerfile、nginx 配置和生产 compose。
- 文档与流程：ADR-0010、OpenSpec 追踪矩阵、审计/验收证据归档。
- 数据库：可能新增 challenge 防重放字段或表，以及 repair claim 所需的迁移；具体迁移必须保持 additive 并提供已有数据升级与回滚说明。
- 本变更只生成规划工件；不包含业务实现、迁移实现或配置修改。
