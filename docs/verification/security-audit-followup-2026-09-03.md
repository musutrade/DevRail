# DevRail 安全审计复核（2026-09-03）

日期：2026-09-03

复核基线：`870fa2aeb3b2d728ad940add96b56264a5c6d51b`（PR #94 合并后的 `main`）

原审计：`docs/verification/security-audit-2026-09-01.md`，基线 `801ae7fe6f3efb9fcc2804a38761f353f4ebc045`

复核范围：`git diff 801ae7..870fa2a` 涉及 PR #90-#94（含 PR #94 安全边界修复），外加对照源码与运行中 CI 的逐条核验。

## 方法与本轮实际执行

本轮是「对照复核」，不是从零开始的新审计：

- 对原审计每条结论，先判断相关文件是否在 `801ae7..870fa2a` 区间内变更；
- 变更过的条目回到当前源码逐行核对修复是否成立；
- 未变更过的条目维持原结论（open），只复核了其引用的文件当前仍未被触碰；
- 已实测命令：`gh repo view`、`cargo tree -i rsa`、`cargo deny check advisories`（含去掉 ignore 的对照运行）、`cargo deny check advisories`（当前配置）、对关键谓词的 `rg` 检查、`gh run list` 核验合并后 main 的 CI。

本轮**未重新执行** `cargo flow verify --all`、`cargo clippy`、完整前后端测试与 Playwright。凡依据源码路径而非运行输出的结论，文中均注明。复核开始时 `git status` 干净，版本控制文件数 863（原审计基线为 853）。

## 重要环境变化

仓库可见性已从审计基线时的 **INTERNAL（private=true）变为 PUBLIC**（复核当日 `gh repo view` 返回 `visibility: PUBLIC`）。这是一次实质性运维变更：

- 原 P1 第 1 条的 `private == false` 守卫因仓库公开而不再跳过，CodeQL 与 dependency review 现在真实运行；
- 公开仓库的依赖图、代码扫描与依赖审查默认可用，不再依赖组织 Code Security 配额；
- 公开后仓库全部内容对外可见。仓库内没有已提交凭据（原审计已核实），但需要重新确认不存在仅适合内部暴露的文档、路径、架构细节或临时口令（如 smoke 脚本中的引导口令）后才应长期保持公开。

## P1 复核结果

| 原条目 | 状态 | 证据与说明 |
| --- | --- | --- |
| 1. CodeQL / 依赖审查从未执行 | **已修复并已核实** | `.github/workflows/codeql.yml`、`ci.yml` 中已无 `github.event.repository.private == false`（`rg` 零命中）。PR #94 的 CodeQL 与 CI Dependency review 最终 success；合并后的 `main`（870fa2a）上 CodeQL、CI、Supply chain security、arc-flow platform 四个 push workflow 全部 success。修复过程中还暴露并解决了组织侧 Code Security / Dependency graph 未生效的问题。 |
| 2. RUSTSEC-2023-0071 错误理由压制 | **已按「带日期风险接受」处理，不再使用错误理由；仍有到期执行缺口** | `deny.toml:9-14` 的 ignore 写明真实链路 `web-push -> jwt-simple -> superboring -> rsa`、到期日 2026-12-31 并指向 `docs/security/rustsec-2023-0071-risk-acceptance-2026-09.md`；`security.yml:68-72` 注释同步更新。实测：当前配置 `cargo deny check advisories` → `advisories ok`；去掉 ignore 的临时配置 → `advisories FAILED`（Marvin Attack / rsa 0.9.10）。**缺口**：cargo-deny 与 rustsec/audit-check 均不解释到期日，2026-12-31 之后不会自动失败，需要额外校验。 |
| 3. 外部评审评论可写任意评审并批量软删除 | **已修复** | 新增 `sync_target`（`backend/src/repositories/devrail_external_review_comments.rs:32`，service 调用在 `services/devrail_reviews.rs:144`），要求 review_id 属于 actor 参与的任务、且项目/仓库与请求体一致；INSERT/软删除均带 `organization_id` 谓词；唯一约束升级为 `(organization_id, provider, external_id)`（迁移 `20260911100000_harden_devrail_security_boundaries.sql`）；缺少稳定外部 ID 时拒绝写入。 |
| 4. Webhook 写入目标取自未签名请求头 | **已修复** | `native_repository_id`（`services/devrail.rs:98`）从已验签正文取仓库 ID；如头部也带 `x-devrail-repository-id` 则强制与正文一致（`:140-146`）；事件 ID 缺失时失败关闭或由正文哈希派生（`validate_event_id`、`signed_body_event_id`，`:208`）；空密钥拒绝（`verify_webhook_signature`，`:72`）；更新语句带 `organization_id` 谓词（`repositories/devrail_pull_requests.rs:18-28`）。 |
| 5. 连接池双重获取 12 处 | **已修复** | 12 处事务内 `pool` 二次读取全部改为同连接读取（新增 `*_in_connection` 变体）或上移到 `begin()` 前：`mfa.rs:188/287/503/561`、`users.rs:332/381/498`、`roles.rs:137/336/343`、`devrail_approvals.rs` 的读已移到事务前。**残留**：`validate_batch_user_ids`（`services/users.rs:555-570`）仍无批次上限，原报告补充项未处理。 |
| 6. 编排层并发把关 | **大部分修复，1 个子项仍 open** | 已修复：Harness 启动与审批恢复改为数据库级抢占（`repositories/devrail_runs.rs:357-384`、`workers/harness_supervisor.rs` 双发送端冲突时释放 claim 并报错）；直接质量门禁执行使用持久化租约（`repositories/devrail_runs.rs:405`，service 在 `services/devrail_runs.rs:1080` 以 10 800 秒租约执行）；`mark_quality_gate_failed` 带 `status='completed'` 与 `rows_affected==1` 守卫（`repositories/devrail_runs.rs:518`）；`mark_waiting` 先 `FOR UPDATE` 校验状态（`repositories/devrail_approvals.rs:147`）；审批过期改为失败关闭的 `fail_expired_run` 并携带幂等键。**仍 open**：repair 门禁重跑仍以 `claim_lease_seconds`（默认 60，`workers/task_scheduler.rs:43/319`）领取，而 `execute_gate_rerun`（`services/devrail_repairs.rs:935`）的命令超时为 900 秒；`renew_gate_rerun_claim`（`repositories/devrail_repairs.rs:1497`）在除测试外无调用方，租约仍可能被 `release_expired_gate_rerun_claims` 中途回收导致同 run 重复执行。 |
| 7. 审批可自我批准 | **已修复** | `decide` 的 UPDATE 增加 `a.requested_by <> $3`（`repositories/devrail_approvals.rs:111`）；service 增加 `approval_actor_can_decide`（`services/devrail_approvals.rs:61`）并在决策前拒绝发起人自批；附单元测试 `approval_requester_cannot_decide_own_request`。 |

## P2 复核结果

### 授权类

- `add_member` 忽略调用者数据范围并可枚举任意用户（`devrail_members.rs`）——**open**：相关文件未出现在 `801ae7..870fa2a` 变更列表。
- TOTP 登录因子未接入防重放计数器——**open**：本轮区间只新增了 `list_passkeys_in_connection` 等连接复用变体，`verify_totp_login`/`verify_totp_enrollment` 仍只消费挑战行，未记录单调计数器。
- 畸形 `X-Forwarded-For` 把请求归因到代理自身——**open**：`src/auth.rs` 未变更。
- `assignee_user_id`/`reviewer_user_id` 不按 actor 范围校验——**open**：`services/devrail.rs` 本轮只改了 webhook 段，未触碰任务/评审创建；`create_review` 亦然。
- `GET /metrics` 未鉴权——**open**：`src/app_metrics.rs` 未变更。属发布拓扑下的暴露面决策问题，建议记录为有意的内部端点或加鉴权。
- Webhook / 回调密钥未进配置与生产环境——**部分**：空密钥在代码层已被拒绝（`services/devrail.rs:72`、`:222`）；但 `DEVRAIL_GIT_WEBHOOK_SECRET`、`DEVRAIL_REPAIR_CALLBACK_SECRET` 仍未出现在 `.env.example`、`compose.production.yaml` 或 CI 配置中，生产配置缺口依旧。
- `APP_ENV` 缺省回落 `development`——**open**：`config.rs:257` 未变。

### 门禁与供应链

- cron 周期扫描被 `dorny/paths-filter` 废掉——**open**：`security.yml` 仍对所有任务套用 `needs.changes` 输出，schedule 场景语义未变。
- nginx 配置变更发布未经扫描镜像、`docker/**` 死条目——**open**：`images` 过滤器仍未含 `deployment/**`，`docker/**` 依然无匹配目录。
- `deny.toml`、`security.yml`、`rust-toolchain.toml` 不在后端过滤器内——**已修复**：`security.yml:34-40` 已把三者加入 backend 过滤器。
- 第三方 Action 全部可变 tag 且门禁禁止 SHA 固定——**open**：本轮未改 `scripts/check-supply-chain.mjs` 与任何 action 引用（新增的 `actions: read` 只影响权限声明）。
- 运行基础镜像未固定 digest——**open**：Dockerfile 与 `check-production-deployment.mjs` 均未变更。
- `npm audit --omit=dev` 排除构建工具链——**open**：`ci.yml` 对应步骤未变。
- `actions: write` 授予未固定 action 的任务——**open**：`security.yml:88` 附近未变。
- `unmaintained = "workspace"`、`multiple-versions = "warn"`——**open**：`deny.toml` 除 ignore 注释外未变。
- `sha2_legacy` 缺注释——**open**：`backend/Cargo.toml` 未变。
- `TRUSTED_PROXY_CIDRS` 信任整段子网——**open**：`compose.production.yaml` 未变。
- smoke 脚本硬编码管理员口令——**open**：`scripts/start-fullstack-smoke.sh` 未变。

### 数据层

- outbox 去重迁移可能销毁未投递事件——**open**：迁移文件不在变更区间。
- 90 处 `CREATE INDEX` 无 `CONCURRENTLY`、无幂等重跑——**open**：本轮新增迁移仍使用非 `CONCURRENTLY` 的 `CREATE UNIQUE INDEX`/`CREATE INDEX`（`devrail_external_review_comments`、`devrail_runs.quality_gate_claim`），规模小于原报告最痛的两个 GIN 索引，但模式问题未解决。
- 修复请求列表 N+1（1+100×4 查询）——**open**：`services/devrail_repairs.rs` 对应段落未变。
- worker 与 HTTP 请求共用连接池、无预留——**open**：`src/db.rs` 与 `src/main.rs` 未变。
- schema 卫生——**部分**：外部评论唯一索引已修正为 org 范围；其余冗余索引与 `devrail_run_events` 无索引问题仍 open。
- `scope_sql` 的 `all` 语义与全局 RBAC 目录——**决策项，未变**：仍是设计取舍，需要产品/架构决策而非单纯代码修复。
- 死代码（`next_sequence`/`next_chain_depth`/`approval_satisfied`）——**open**：`next_sequence`/`next_chain_depth` 仍有 `pub` 无生产调用方；同文件已存在带 `FOR UPDATE` 的 `_in_connection` 安全版本。

### 前端

PR #94 未触碰 `frontend/`，原报告全部前端条目维持 open：

- run 页 SSE 重连定时器未清理、销毁后复活连接；
- 生产 CSP `script-src 'self'` 阻断内联主题脚本；
- 任务详情事件流无重连且 nginx 30 秒断开；
- `link.href = artifact.downloadUrl` 绕过 Angular URL 净化；
- 两个 SSE 端点硬编码 `/api/v1`；
- `localStorage.setItem` 无守卫；
- `routerLink` 绑定自由格式 deepLink；
- 原始服务端错误消息逐字渲染；
- 通知对象原地修改与若干清理项。

### 文档与实现一致性

- 版本控制证据死链——**open**：`docs/HANDOFF.md:13`、`docs/verification/mvp-acceptance-2026-08-28.md:38/44` 等仍引用 gitignore 的 `test_result.md`；本轮复核自身也未能在仓库内取得该文件。
- 续轮「Playwright 专项测试已通过」声明——**open**：`docs/devrail-implementation-status.md:26` 仍保留该表述，与 HANDOFF 的「未完成」矛盾未消。
- 真实后端浏览器覆盖、静默空过的 13 个数据库测试、Web Push 零自动化、无测试模块清单、吞吐验收缺 CPU/锁数据、非阻断门禁步骤、`extracted/` 孤立目录——**open**：相关代码/文档均未变。
- 状态文档新增了「2026-09-03 安全边界修复，仍待远端 CI 与产品验收」记录——**有改进**，方向正确。

## 本轮复核中值得单独记录的点

1. **RUSTSEC-2023-0071 现在是「有据可查的到期风险接受」而非「错误压制」**：理由、路径、到期日、文档都已齐备。唯一缺口是没有机制在 2026-12-31 后让门禁自动失败，建议补一个带日期的检查（例如 cron 前置断言或提交时校验），否则会退化为永续 ignore。
2. **修复中新增的迁移仍不符合仓库自身的索引规范**（非 `CONCURRENTLY`、部分失败不可幂等）。新索引规模小，当前风险低于原报告指出的 `audit_logs` GIN 索引，但说明「新代码不再制造旧问题」尚未成为迁移约定。
3. **修复门禁重跑租约是唯一未闭合的 P1 子项**：DB 层已有 `renew_gate_rerun_claim` 与终态守卫，只是没有生产调用方。接线续租或把该路径的租约提高到超过命令超时，是闭环成本最低的一步。
4. **仓库公开改变了这份审计的适用语境**：原「INTERNAL 所以依赖私有特性」的前提已不成立；公开带来的新风险（暴露面、信息泄漏）需要单独评估，不能只当作「让 CI 变绿」的手段。

## 建议下一步

按原报告自身的建议，把剩余问题转成 OpenSpec 变更并逐条附修复 diff + 测试 + 命令输出。建议拆分顺序：

1. P1 剩余项：repair 门禁重跑租约续租/提高 + RUSTSEC-2026-12-31 到期自动化。
2. P2 授权剩余项：`add_member` 数据范围、TOTP 登录防重放、XFF 回落、assignee/reviewer 范围、Webhook 密钥进生产配置。
3. 供应链 P2 集群：Action SHA 固定与 `check-supply-chain.mjs` 同步修改、基础镜像 digest、nginx 变更进扫描、cron 语义修复。
4. 前端 P2：SSE 生命周期、CSP 内联主题、下载 URL 白名单。
5. 文档证据链：把门禁证据从 gitignore 目录改为可提交产物。
6. 全部完成后，以新的 `main` 为基线再跑一次完整审计并独立归档本文件。

本文件不改变原审计任何条目的「已证实」历史含义，也不将任何 MVP 验收行置为「通过」。
