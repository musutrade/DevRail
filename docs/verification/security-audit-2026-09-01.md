# DevRail 全项目审计报告

日期：2026-09-01

提交 SHA：`801ae7fe6f3efb9fcc2804a38761f353f4ebc045`
分支：`chore/archive-backend-test-throughput`
仓库可见性：INTERNAL（`gh repo view --json visibility`）

## 使用规则

本报告是一次静态审计的结果，不是验收记录。所有结论均由代码路径推导，除注明「已实测」者外，均未观测到实际利用。凡标注「已证实」的条目，指审计者已亲自回到源码或命令输出复核过该结论；标注「疑似」者指代码路径成立但可达性未验证。

本次审计未修改任何文件。审计期间 `git status` 为空，受版本控制文件数 853，与开始时一致。

不得将本报告任何条目直接转写为 [MVP 验收矩阵](mvp-acceptance-2026-08-28.md) 的「通过」状态。修复后需另附命令输出与执行日期。

## 覆盖范围与方法

分七个方向并行审计：认证授权、编排执行链路、并发与状态机可靠性、数据层与 SQL、供应链与 CI、前端、文档与实现一致性。

已实测执行的命令：`cargo clippy`（全绿）、OpenAPI 客户端漂移检查（零漂移）、`cargo tree -i rsa`、`cargo deny check advisories`（含移除 ignore 后的对照运行）、`gh repo view`。

覆盖缺口：数据层方向的专项扫描超时未返回，该方向结论（SQL 注入面、审计日志约束、连接池、分页、事务原子性、迁移锁）由审计者直接核实，已覆盖主要风险项。

## 总体判断

安全基线（会话、CSRF、密码哈希、审计日志追加保护、容器加固）扎实，超出多数同类项目。问题集中于两处：

1. **质量门禁存在「看起来通过、实际没运行」的空转**，且门禁脚本本身在维护更弱的姿态。
2. **DevRail 编排层的并发与授权把关不严**，RBAC 基线部分的质量明显高于后叠加的 DevRail 控制面。

## 重要范围前提

授权类缺陷目前**大多尚不可达**，原因有两条，两条都可能随运维动作失效：

- 迁移只播种一个组织且无组织管理 API，因此「跨租户」是潜在而非当前可达，现阶段影响面是**组织内数据范围绕过**。
- 被授予 DevRail 权限的角色（`organization_admin`、`project_admin`、`developer`、`reviewer`）**从未被创建**。全部 `INSERT INTO roles` 只产出 `super_admin`、`editor`、`viewer`、`compliance_auditor`、`support_tier2`、`billing_manager`。相关 `INSERT INTO role_permissions` 在全新安装上是空操作，今天只有 `super_admin` 持有这些权限。

一旦运维按文档创建预期的非管理员角色，下列 P1/P2 授权条目将立即变为活的漏洞。

## P1 — 需优先处理

### 1. CodeQL 与依赖审查从未执行过（已证实）

`.github/workflows/codeql.yml:22` 与 `.github/workflows/ci.yml:192` 均带条件 `github.event.repository.private == false`。仓库实际可见性为 INTERNAL，即 `private` 为 `true`，因此两个任务在每次 PR、push 与周期触发时全部跳过。SAST 与依赖差异审查对本代码库从未运行。

之所以看起来是绿的：`scripts/check-supply-chain.mjs:40-41` 只 grep 工作流中是否存在 `github/codeql-action/init@v4` 字符串，不检查 `if:` 条件。门禁通过，而被它认证的两个扫描是空转。

INTERNAL 仓库在具备 Advanced Security 时可使用 CodeQL，因此该守卫也无必要。

修复：删除两处 `github.event.repository.private == false` 守卫。

### 2. 依赖树中唯一真实漏洞被错误理由压制（已证实，已实测）

`deny.toml:10-11` 与 `.github/workflows/security.yml:68-69` 同时忽略 `RUSTSEC-2023-0071`，注释称其来自 SQLx 未启用的 MySQL 驱动。**该说明不成立。**

实测 `cargo tree -i rsa`：

```
rsa v0.9.10
└── superboring v0.1.14
    └── jwt-simple v0.12.17
        └── web-push v0.11.0
            └── arc-admin-backend v0.1.0
```

`backend/Cargo.lock:3410-3434` 中 `sqlx-mysql` 0.9.0 的依赖列表不含 `rsa`。漏洞crate 经由 `web-push`（`backend/Cargo.toml:56`）进入，位于**正在启用的 Web Push VAPID 签名路径**上。

移除 ignore 后对照运行：

```
error[vulnerability]: Marvin Attack: potential key recovery through timing sidechannels
advisories FAILED
```

即门禁本可拦下。当前配置报 `advisories ok`。`rustsec/audit-check` 与 `cargo-deny` 携带同一条错误 ignore，无第二意见。

修复：移除两处 ignore，随后替换 `web-push`，或以准确且带日期的注释接受风险，注释须写明真实路径 `web-push → jwt-simple → superboring → rsa`。

### 3. 外部评审评论同步可写入任意评审并批量软删除其原有评论（已证实）

`src/services/devrail_reviews.rs:135` 的 `sync_external_comments` 校验了请求体的 `project_id`/`repository_id`（经 `get_git_provider` 与 `find_repository`，均带 actor 范围），但路径参数 `review_id` 全程未经过任何带数据范围的查询。

下游两处写入均无租户谓词：

- `src/repositories/devrail_external_review_comments.rs:30` 的 INSERT 为 `SELECT r.organization_id,$1,... FROM devrail_reviews r WHERE r.id=$1`，行的 `organization_id` 直接继承目标评审。
- 同文件 `:41`、`:44` 的 `mark_missing_deleted` 为 `UPDATE ... SET deleted_at=now() WHERE review_id=$1 AND provider=$2`，既无 org 也无参与者谓词。

利用路径：持 `devrail:review:write` 且拥有任一自有仓库者，向 `/api/v1/reviews/{受害者 review_id}/external-comments/sync` 提交自己的 `project_id`/`repository_id` 与任意 PR 编号。其 PR 的评论正文、文件路径与作者名落入受害者评审线程，同时 `mark_missing_deleted` 软删除该评审同 provider 下全部既有外部评论。若自有仓库返回零评论，`ids.is_empty()` 分支无条件清空受害者行。

该路径绕过了读路径已有的参与者限制（`:10` 的 `list` 要求 `r.requested_by=$3 OR r.reviewer_user_id=$3`），且因读路径有范围限制，攻击者自身响应为空，**写入是静默的**。

加重项：`migrations/20260828100000_add_devrail_external_review_comments.sql:14` 的 `UNIQUE(provider, external_id)` 无租户列，冲突时互相覆盖。

数据库层无兜底：`migrations/` 中零 `CREATE POLICY`、零 `ROW LEVEL SECURITY`、零 `GRANT`/`REVOKE`，仓库层谓词缺失即无缓解。

### 4. Webhook 签名仅覆盖请求体，写入目标取自未签名请求头（已证实）

签名机制本身正确：`src/services/devrail.rs:70-83` 的 `verify_webhook_signature` 为 HMAC-SHA256 over body，含长度检查与 `subtle::ConstantTimeEq::ct_eq` 常量时间比较，密钥缺失时以 403 失败关闭（`:175-176`）。

问题在签名未覆盖的部分：

- `repository_id` 取自 `x-devrail-repository-id` 头（`:95-99`）。
- `event_id` 被 `x-github-delivery` / `x-gitlab-event-uuid` 覆盖，且**头优先于已签名的体内字段**（`:185-190`，`.or(payload.event_id)`）。
- 写入目标无 org 谓词：`src/repositories/devrail_pull_requests.rs:18` 为 `UPDATE devrail_pull_requests SET url=$4,status=$5 WHERE provider=$1 AND repository_id=$2 AND number=$3`。

利用路径：观测到一次合法签名投递者（镜像请求、代理日志、捕获的重试），可用同一请求体配不同 `x-devrail-repository-id` 将 PR URL 与状态写到任意仓库行，并用新的投递头绕过去重。同时会向该仓库所有者触发通知（`:229-247`）。

独立缺陷：去重是条件式的。`:193` 为 `if let Some(event_id) = ...`，因此**既无投递头又无体内 `eventId` 的请求完全跳过 `claim_event`**，可无限重放。无时间戳与 nonce 绑定。

对照：修复回调把这点做对了——幂等键由**已签名体内**字段派生（`src/services/devrail_repairs.rs:106`），新鲜度对已签名的 `evidence_observed_at` 校验。

### 5. 连接池双重获取，12 处，登录路径首当其冲（已证实）

连接池为 `max_connections: 10`、`acquire_timeout: 5s`（`src/db.rs:58,60`）。以下 12 处在持有一条事务连接的同时用 `pool` 再取第二条：

| 位置 | 事务开启行 |
| --- | --- |
| `src/services/mfa.rs:188` | 170 |
| `src/services/mfa.rs:287` | 270 |
| `src/services/mfa.rs:502` | 491 |
| `src/services/mfa.rs:559` | 548 |
| `src/services/users.rs:331` | 329 |
| `src/services/users.rs:379` | 376 |
| `src/services/users.rs:495` | 493 |
| `src/services/roles.rs:137` | 127 |
| `src/services/roles.rs:331` | 328 |
| `src/services/roles.rs:332` | 328 |
| `src/services/devrail_approvals.rs:101` | 100 |
| `src/services/devrail_approvals.rs:197` | 196 |

失效场景：10 个并发请求各占一条连接、各等一条不可能存在的第 11 条，全部阻塞至 5 秒超时后集体回滚并返回 500——同步的延迟墙加错误风暴。

`mfa.rs:188`（`verify_totp_login`）位于交互式登录路径，血溅面最大：一次并发尖峰会把用户挡在认证之外。`users.rs:379`、`:495` 位于对 `req.user_ids` 的 `for` 循环内。`roles.rs:331`、`:332` 单次调用取两次。

`devrail_approvals.rs` 那处尤能说明是无意为之：`:98` 已正确地在 `begin()` **之前**用 `pool` 读过一次，`:101` 紧接着又在事务内读一次。

这些全部是事务前快照读（审计旧值、范围校验），上移到 `begin()` 之前或改用 `&mut tx` 即可，行为不变。

补充：循环内的获取是顺序的，因此单个大批次同时只需两条连接——耗尽场景需要并发请求。`validate_batch_user_ids`（`src/services/users.rs:552-566`）拒绝重复与自我指向，但**未设批次上限**。

### 6. 编排层并发把关缺陷（已证实）

**`claim_harness_start` 的守卫在单副本下即可绕过。** 两个并发的审批恢复请求：`src/services/devrail_approvals.rs:284-294` 的 `recover` 在未加锁的连接池读上用 Rust 判断 `approval.status != "pending"`，`src/workers/harness_supervisor.rs:396` 的 `recover_run` 以同样方式重读 run。两者皆通过，两次 `claim_harness_start` 皆返回 true，在同一 `cwd` 启动两个进程。更糟：`harness_supervisor.rs:185` 的 `controls.lock().await.insert(run_id, tx)` 只保留第二个发送端，第一个进程成为**孤儿**——`interrupt` 与 `resolve_approval` 均无法触达，且因子进程由分离的 spawn 持有，`kill_on_drop` 永不触发。这在随仓库发布的单副本 compose 配置下即可达。

**质量门禁完全没有租约。** `src/services/devrail_runs.rs:1068-1199`（handler 在 `src/handlers/devrail.rs:1018`）在 `:1075` 用 Rust 检查状态，不标记「门禁运行中」，随后每个门禁在任何事务外运行最多 900 秒。两个并发 POST 会在同一 `run.cwd` 启动相同命令并双双调用 `devrail_artifacts::store`——每门禁产生两行产物与两份文件。事件插入是幂等的，因此journal 掩盖了磁盘上确实发生的重复。

**`mark_quality_gate_failed` 写终态无 OLD 状态守卫。** `src/repositories/devrail_runs.rs:474` 为裸 `WHERE id=$1`，无 `rows_affected` 检查，随后紧跟同样无守卫的 `update_task_status(..., "failed")`。900 秒门禁窗口结束后，这会覆盖已 `completed`/`cancelled` 的 run 与已 `succeeded` 的任务，绕过用户路径内联强制的状态转换表。

**门禁重跑租约 60 秒对 900 秒命令超时，且从不续租。** `claim_lease_seconds` 默认 60（`src/workers/task_scheduler.rs:43`，在 `:319` 传入），门禁命令在 900 秒超时下运行（`src/services/devrail_repairs.rs:974-986`）。`renew_gate_rerun_claim` 存在于 `src/repositories/devrail_repairs.rs:1497` 但**在其自身文件外无任何调用方**。于是 `release_expired_gate_rerun_claims` 在执行中途回收该行，第二个 worker 在同一 `cwd` 跑同一门禁，原 worker 的 `complete_gate_rerun` 匹配零行被静默丢弃（`devrail_repairs.rs:1012`）。表无 attempts 列（`migrations/20260909100000_add_controlled_repair_runs.sql:177-212`），因此任何慢门禁会**每个 tick 永久重复**。顺序 await 配 `CLAIM_BATCH_SIZE = 16` 意味着批次中较晚的项在开始前就已失去租约。

**`mark_waiting` 无守卫地把任务强制为 `awaiting_approval`。** `src/repositories/devrail_approvals.rs:152-153`：run 更新带 `status IN ('starting','active')` 但丢弃 `rows_affected`，任务更新无任何谓词。harness 停顿与审批请求竞争时，会留下任务为 `awaiting_approval` 而无活动 run 的非法边，且无 worker 对账。

**审批过期的第二次提交同样丢弃返回值。** `src/services/devrail_approvals.rs:383-396` 丢弃 `update_run_terminal` 的 bool，随后把任务从 `succeeded` 盖成 `failed`。提交后的 `resolve_approval` 是代码库中**唯一未镜像到 `devrail_outbox_events` 的生命周期事件**，因此持久化的挂起无恢复路径。

**修复移交接受 0 行任务投影（疑似）。** `src/repositories/devrail_repairs.rs:1996-2006` 缺 `!= 1` 检查，而同文件五个同类转换全部以此失败关闭。缺失检查已证实，具体可达交错为疑似。

### 7. 审批可自我批准（已证实）

`src/repositories/devrail_approvals.rs:111` 的 `decide` 携带 status、`expires_at` 与数据范围谓词，但**缺 `a.requested_by <> $3`**。`src/services/devrail_approvals.rs:175` 只检查待批状态、策略版本新鲜度与范围，从不比对 `actor.user_id` 与 `approval.requested_by`。由于 `requested_by` 取自 run 的 `owner_user_id`（`:118`），发起高风险工具调用者可自行放行，人工闸门失效。

紧邻的两处实现了恰好这个检查，说明是遗漏而非设计：`withdraw` 固定 `a.requested_by=$3`（`src/repositories/devrail_approvals.rs:125`），评审创建拒绝自我评审（`src/services/devrail_reviews.rs:420`）。修复审批共享此缺陷（`src/repositories/devrail_repairs.rs:1833` 仅对 `withdrawn` 固定请求者）。

对 `SelfOnly` 范围账号最为尖锐：审批的 `scope()` 助手将其限制在 `a.owner_user_id = $3`（`:11`），因此自己发起的审批是他们**唯一**能操作的审批。

## P2 — 中等优先

### 授权

**`add_member` 忽略调用者数据范围且接受任意 `user_id`（已证实）。** `src/repositories/devrail_members.rs:34` 为 `... FROM devrail_projects p JOIN users u ON u.id=$4 WHERE p.id=$6 AND p.organization_id=$1`。两处缺陷：项目仅按 `organization_id` 校验，未用 `list`（`:16`）与 `revoke`（`:52`）都在用的 `scope()` 助手，且 service（`src/services/devrail_members.rs:43`）不像 `list` 那样先经带范围的 `find_project` 预解析；`u` 的 JOIN 无 `u.organization_id=$1`，而 `RETURNING` 含两个无过滤子查询（`SELECT username FROM users WHERE id=...` 与 `display_name` 同理），使 201 响应泄露目标用户身份——遍历 `user_id` 即得一个**忽略调用者范围的目录枚举探针**。schema 层亦无兜底：`migrations/20260823100000_add_devrail_project_members.sql:18` 的 `user_id` 是普通 FK，而 `:23-24` 的 `project_id`/`department_id` 使用复合 `(id, organization_id)` FK，`user_id` 是唯一的例外。被注入的成员本身不获得权限：`devrail_project_members` 仅出现在该仓库文件与 `openapi.rs` 中，从未参与授权谓词。

**TOTP 防重放机制未接入登录路径（已证实）。** `verify_totp_code`（`src/services/mfa.rs:876-887`）解析匹配的计数器并经 `consume_reauth_totp_counter`（`src/repositories/mfa.rs:102-121`，单调 `last_reauth_totp_counter < $2`）消费，因此**升权与模块解锁受防重放保护**。而 `verify_totp_login`（`src/services/mfa.rs:192-231`）与 `verify_totp_enrollment`（`:291-295`）调用 `check_current(...).is_some()`，仅消费挑战行，不记录任何计数器。配合 `skew(1)` 与 30 秒步长（`src/mfa.rs:118-120`），验证码在登录因子上约有 90 秒有效窗口。迁移文件名即为 `20260810080000_prevent_totp_replay.sql`；登录路径是唯一未应用它的地方。利用需同时掌握口令（需要新的挑战令牌，且先前挑战在 `src/repositories/mfa.rs:148-162` 失效），因此这在钓鱼中继场景下才有意义。

**畸形 `X-Forwarded-For` 把请求归因到代理自身，污染共享限流桶（已证实）。** `resolve_client_ip`（`src/auth.rs:80-88`）用 `collect::<Option<Vec<_>>>()` 收集链，因此**单个**不可解析跳、或超过 `MAX_FORWARDED_FOR_HOPS`（16）、或值超 1024 字节，都会丢弃整条链回落到 `peer_ip`——在反向代理后即代理自身地址。项目自身测试断言了此行为（`src/auth.rs:407-419`）。nginx 使用 `proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for`（`deployment/nginx/nginx.conf:64`），是**追加**而非覆盖，因此攻击者发 `X-Forwarded-For: junk` 产生 `junk, <真实IP>`，整条链被丢弃。其 `source_ip` 限流键（`src/services/auth.rs:58`）遂哈希 nginx 容器 IP，为代理后全体用户共享。按默认 `LOGIN_IP_MAX_FAILURES=50`、900 秒窗口与 900 秒锁定（`src/config.rs:304-313`），50 次携带畸形头的失败登录即锁死该桶，而 `locked_until` 检查全部三个维度（`src/services/auth.rs:289-305`），于是**全体用户的登录在 15 分钟内均返回 429**。同时把攻击者真实地址与按 IP 计数解耦。按账号锁定仍保护个体账号。正确回落应是最右侧可解析的不可信跳，而非 peer。

需说明未坏的部分：可信代理遍历本身正确。任何头被采信前 `peer_ip` 必须匹配 `TRUSTED_PROXY_CIDRS`（`:69-71`），循环自右向左返回最近的不可信地址，因此在发布拓扑下不可信客户端无法伪造自身地址。

**`assignee_user_id` / `reviewer_user_id` 从不按 actor 范围校验（已证实，低）。** `create_task` 原样透传 `req.assignee_user_id`（`src/services/devrail.rs:2759`），`validate_task_resources`（`:2542`）只校验仓库与环境；`create_review`（`src/services/devrail_reviews.rs:415`）同形。影响有限，因两字段都不放宽读取权限：评论范围将 `t.organization_id=$1` 与受派人判定以 AND 相连（`src/repositories/devrail_comments.rs:14`），评审列表同理（`src/repositories/devrail_reviews.rs:11`）。只能把行挂到范围外的用户 ID 上。

**`GET /metrics` 未鉴权（已证实，低）。** `app_metrics::render`（`src/app_metrics.rs:520`）只取 `State`，无 extractor、无 `route_layer`。内容敏感度低——标签来自 `MatchedPath` 故不泄露路径参数，`record_authenticated_user` 把用户 ID 写入 tracing span 而非指标（`src/telemetry.rs:262-264`）。发布拓扑下 nginx 仅代理 `location /api/`（`deployment/nginx/nginx.conf:59`）且后端不发布端口，故非公网可达；但它向同 Docker 网络内任何一方泄露路由清单、流量量级、错误率与连接池饱和度。

**两个机器端点密钥以 `std::env::var` 在请求时读取，且不在配置与生产环境中（已证实，低）。** `DEVRAIL_GIT_WEBHOOK_SECRET`（`src/services/devrail.rs:175`）与 `DEVRAIL_REPAIR_CALLBACK_SECRET`（`src/services/devrail_repairs.rs:81`）在 `config.rs` 与 `compose.production.yaml:3-40` 的 `x-backend-environment` 块中均不存在。两者失败关闭为 403，故后果是生产环境端点静默失效而非绕过，且无启动校验可捕获。一处锐边：`Hmac::new_from_slice` 接受零长度密钥，因此 `DEVRAIL_GIT_WEBHOOK_SECRET=""` 会产生任何人都能计算的签名，**无任何逻辑拒绝空密钥**。

**`APP_ENV` 缺省回落 `development`（已证实，配置失误放大器）。** `src/config.rs:257` 为 `app_env.as_deref().unwrap_or("development")`。判定为 Development 时连带后果：`secure_cookies` 为 false（`src/main.rs:128`）、`AUTO_MIGRATE` 默认开启（`src/config.rs:275`）、CORS 空来源不再报错（`:323`）。生产部署若漏注入该变量，会话 Cookie 丢失 Secure 标记且容器启动即自动迁移。`compose.production.yaml` 确实设置了它，故这是放大器而非当前缺陷。建议生产改为 `${APP_ENV:?}` 或在 Production 校验中强制显式设置。

### 门禁与供应链

**周期扫描被自身路径过滤器废掉（已证实）。** `.github/workflows/security.yml:7-8` 配置了 cron，但两个实际任务都受 `dorny/paths-filter` 输出约束（`:50`、`:86`）。按该 action 文档行为，非 `pull_request` 事件下 `base` 默认为默认分支，当 base 与触发分支相同时「从最近一次提交检测变更」。cron 在 `main` 上触发，故只考虑 tip 提交的 diff。除非最近一次提交恰好触及 `backend/**` 或 `frontend/**`，周期扫描什么都不做——这恰好废掉了周期扫描的全部意义：对未变更代码发现新披露的漏洞。`codeql.yml` 正确地没有路径过滤（但见 P1 第 1 条）。

**nginx 配置变更会发布未经扫描的镜像（已证实）。** `frontend/Dockerfile:12` 将 `deployment/nginx/nginx.conf` 复制进运行镜像，而 `.github/workflows/security.yml:38-43` 的 `images` 过滤器列出 `backend/**`、`frontend/**`、两个 Dockerfile 与 `docker/**`，**不含 `deployment/**`**。因此改动 TLS 协议列表、CSP 或 `nginx.conf:42-52` 的安全响应头，会产出实质不同的生产镜像却无 Trivy 扫描、无 SBOM。另外该过滤器中的 `docker/**` 匹配不到任何东西——仓库无 `docker/` 目录。

**依赖策略文件在其自身门禁之外（已证实）。** `.github/workflows/security.yml:34-37` 的 `backend` 过滤器为 `backend/**`、`Cargo.toml`、`Cargo.lock`，**不含 `deny.toml`**，也不含 `.github/workflows/security.yml` 自身。因此为 `deny.toml` 添加一条通配 `ignore`、或弱化 `unmaintained`/`yanked` 的 PR，不会触发应用该策略的 `rust-dependencies` 任务，策略变更未经执行即合并。另注：根 `Cargo.toml`/`Cargo.lock` 不存在（无 workspace 根），这两条过滤项是死条目，仅因 `backend/**` 覆盖了真实清单才无害。`rust-toolchain.toml` 同样不在 CI 的 backend 过滤器内（`ci.yml:32-36`），故工具链升级会跳过后端验证。

**第三方 Action 全部按可变 tag 引用，且门禁禁止修复（已证实）。** 全部第三方 action 按 tag 而非 SHA 固定，SHA 固定数为 0。最差的是浮动主版本 tag：`dorny/paths-filter@v3`（`security.yml:30`、`ci.yml:28`、`arc-flow-platform.yml:27`）与 `actions-rust-lang/setup-rust-toolchain@v1`（六处调用）——上游 tag 移动即在具备仓库检出与缓存写权限的任务中执行新代码。加重细节：`scripts/check-supply-chain.mjs:30-37` 硬编码了精确 tag 字符串（`rustsec/audit-check@v2.0.0`、`EmbarkStudios/cargo-deny-action@v2.1.1`、`aquasecurity/trivy-action@v0.36.0`、`anchore/sbom-action@v0.24.0`），改为 SHA 固定会**导致供应链门禁失败**。门禁在主动维护更弱的姿态，故修复须同时改动该脚本。

**运行时基础镜像未固定，却通过了仅查 `:latest` 的检查（已证实）。** `backend/Dockerfile:10` 为 `FROM debian:bookworm-slim`，无补丁版本、无 digest。构建阶段与两个前端阶段均有版本 tag（`rust:1.97.1-bookworm`、`node:24.18.0-alpine3.23`、`nginx:1.30.4-alpine3.24`），但均未用 digest。`scripts/check-production-deployment.mjs:23` 只拒绝 `/^FROM\s+\S+:latest\b/`，故 `bookworm-slim` 顺利通过。后果：CI 中记录的 Trivy 结果与后续重建实际发布的层不对应，构建不可复现。

**`npm audit --omit=dev` 隐藏了编译型 SPA 的构建工具链（疑似）。** `.github/workflows/ci.yml:173` 为 `npm audit --omit=dev --audit-level=high`。Angular 应用的发布产物由 `@angular/build`、`@angular/compiler-cli`、`typescript` 产生，三者均在 `devDependencies`：它们在构建期执行且其输出**就是**生产资产，因此该处被投毒会直达用户，而 `--omit=dev` 恰好排除这一面。`--audit-level=high` 还丢弃 moderate 级发现，而使用 `fail-on-severity: moderate` 的 `dependency-review` 按 P1 第 1 条从不运行。标注疑似：未实际运行 `npm audit`（需网络）。

**`actions: write` 授予运行四个未固定 action 的任务（已证实）。** `.github/workflows/security.yml:88`。`actions: write` 允许操作缓存以及删除/重跑工作流运行。该任务运行 `docker/setup-buildx-action@v3`、`docker/build-push-action@v6`、`aquasecurity/trivy-action@v0.36.0`、`anchore/sbom-action@v0.24.0`，均未 SHA 固定。`type=gha` 需要缓存写权限，但运行操作能力是附带的爆炸半径。跨 main/PR 边界的缓存投毒风险较低，因 Actions 缓存按分支隔离。

**`unmaintained = "workspace"` 收窄了告警检查（已证实，低）。** `deny.toml:8-9` 将 `unmaintained` 与 `unsound` 都限定到 workspace 成员，故传递性的 unmaintained crate 不被报告。以 `unmaintained = "all"` 重跑仍报 `advisories ok`，故今日无隐藏项——这是潜在缺口而非当前遗漏。

**重复版本策略从不阻断（已证实，低）。** `deny.toml:35` 为 `multiple-versions = "warn"` 且 `skip = []`。`cargo deny check bans` 报告 4 个 `base64`（0.13.1/0.21.7/0.22.1/0.23.1）、3 个 `getrandom`、2 个 `sha2`、2 个 `tower-http`、2 个 `reqwest`，随后退出 `bans ok`。`base64 0.13.1` 经 `web-push → sec1_decode → pem 0.8.3` 进入，是长期停滞的路径。

**`sha2_legacy` 双固定是正当的（已证实，非缺陷）。** `backend/Cargo.toml:35`：`sha2 0.11` 广泛使用，而 `sha2_legacy`（0.10）仅用于 `src/services/devrail.rs:72,336` 与 `src/services/devrail_repairs.rs:35,1260,1313`。0.10 与 0.11 产生相同 SHA-256 摘要，故这是对仍停留在 0.10 `digest` trait 的依赖的 API 兼容垫片，不是密码学降级。建议在 `Cargo.toml` 补一条注释记录原因，否则这个名字会招来「删掉 legacy 那个」的清理。

**`TRUSTED_PROXY_CIDRS` 信任整段应用子网（已证实，低）。** `compose.production.yaml:9` 设 `TRUSTED_PROXY_CIDRS: 172.30.0.0/24`，与 `:187` 声明的 `app` 网络子网完全一致。该网络上任何容器——不止 nginx 前端——都能伪造 `X-Forwarded-For`，从而规避按 IP 的登录限流。今日该网络只有前端与后端，暴露面可控；收紧到前端地址是廉价修复。

**smoke 脚本内硬编码管理员口令（已证实，低）。** `scripts/start-fullstack-smoke.sh:20` 为 `BOOTSTRAP_ADMIN_PASSWORD=Fullstack-Smoke-Password-2026!`，随后 `:22-31` 对由 `TEST_DATABASE_URL` 派生的 `DATABASE_URL` 执行 `migrate` 与 `bootstrap_admin`。`:4` 的 `:?` 守卫保证变量已设置，但不约束它**指向何处**，故误指的 `TEST_DATABASE_URL` 会创建带仓库已知口令的管理员账号。

### 数据层与迁移

**outbox 去重迁移销毁未投递事件（已证实）。** `migrations/20260902120000_fix_outbox_event_deduplication.sql:4-15` 删除重复行时以 `ORDER BY id` 保留最小 ID。分区中**无 `status` 过滤**，故当一个重复组同时含已投递事件与仍待投递事件时，已投递行获胜，**待投递的通知被永久删除**，且不可恢复。同文件第二个缺陷：`:17-24` 的唯一索引包含 `COALESCE(payload->>'notificationSource', payload::text)`。btree 索引行上限约 2704 字节，故任何缺 `notificationSource` 且超过该长度的 outbox payload 都会失败——要么在既有数据上中止迁移，要么在部署后拒绝插入并使整个外层用户事务失败。

**90 处 `CREATE INDEX`，零处 `CONCURRENTLY`（已证实）。** 且零处使用 `IF NOT EXISTS`，故部分失败后不可幂等重跑。`CREATE INDEX` 持 `ShareLock` 阻塞该表全部写入。最痛的是 `migrations/20260811033000_optimize_admin_search.sql` 构建 5 个 GIN 三元组索引，其中两个建在 `audit_logs`（`:10-13`）——schema 中数据量最大的表；由于几乎每个写操作都落一条审计记录，建索引期间**全站写入停摆**。该文件不含动态 SQL，无注入暴露面。另 `migrations/20260808081500_add_organization_data_scopes.sql:85-86` 新增两个 `BIGINT REFERENCES` 列，需对被引用表取校验性 `AccessExclusiveLock`。`migrations/20260904100000_add_tasktracker_workflow_foundation.sql:6` 对既有表 `ADD COLUMN ... NOT NULL DEFAULT` 一个 JSONB 字面量，PG 11+ 走元数据快路径但仍需 `AccessExclusiveLock`。需注意 sqlx 默认以事务包裹每个迁移，故采用 `CONCURRENTLY` 需让该迁移退出事务包裹。

**修复请求列表接口的 N+1（已证实）。** `src/services/devrail_repairs.rs:589-591` 对最多 100 项循环调用 `response()`，每次发出 4 个查询——`find_diagnosis` 加一个三路 `tokio::try_join!`（`:529-538`），即每请求 1 + 100×4 = **401 个查询**。`try_join!` 使每项同时获取 3 个连接，因此这也直接加剧上述连接池压力。分页本身各处都正确设限（`clamp(1,100)` / `(1,200)` / `(1,500)`，见 `src/services/users.rs:21`、`audit_logs.rs:15`、`devrail_artifacts.rs:151`、`devrail_runs.rs:962,1050`、`devrail.rs:2303`），无无界结果集缺陷。

**worker 与 HTTP 请求共用连接池且无预留容量（已证实，低）。** 六个后台 worker 加 harness supervisor 全部接收 `state.pool.clone()`（`src/main.rs:140-169`），无独立 worker 池、无预留余量，故 worker 负载与请求路径在 5 秒超时下直接竞争。`statement_timeout` 在 `after_connect` 中按会话正确应用（`src/db.rs:156`，用 `set_config(..., false)`），默认 30 秒；生产将 `max_connections` 提到 20。

**schema 与索引卫生（已证实，低）。** `migrations/20260822110000_add_devrail_harness_runs.sql:76` 声明 `UNIQUE (run_id, cursor)`（已隐含创建唯一索引），`:82` 又在相同列上建第二个同样的索引——在最繁忙的表上产生冗余写放大。另 `src/repositories/devrail_external_review_comments.rs:10` 的 `changeset_matched` 关联子查询按 `event_type` 与 `payload->>'path'` 过滤 `devrail_run_events`，两者均无索引，且每评论行执行一次。

**两项建议记录决策而非修复。** `src/repositories/devrail.rs:222` 的 `scope_sql` 依赖 `AND` 优先于 `OR` 的结合性，使 `all` 范围 actor 绕过 `organization_id`——这是有意为之（只有 `super_admin` 获得 `all`，见 `migrations/20260808081500_add_organization_data_scopes.sql:75`），但与其他四个文件中同名助手的行为不同。此外 `roles`/`permissions`/`user_roles` 完全没有租户列（`migrations/0001_init_rbac_schema.sql:24,59,67`），故 RBAC 是全局目录，任何持 `role:write` 的租户管理员都可为所有组织修改它。

**建议删除的死代码（已证实，低）。** 仍为 `pub` 但无调用方且不安全：`next_sequence` / `next_chain_depth`（`src/repositories/devrail_continuations.rs:691,729`，对 `UNIQUE` 约束做未加锁的 `MAX()+1`）与 `approval_satisfied`（`src/repositories/devrail_repairs.rs:1921`，无范围限制的策略闸门）。

### 前端

**SSE 重连在组件销毁后复活流（已证实，高）。** `frontend/src/app/pages/devrail-run/devrail-run.ts:533-538` 的 `onerror` 用 `window.setTimeout(() => this.connectEvents(), 1500)` 重连，但**定时器句柄从不保存**，而 `ngOnDestroy`（`:85-87`）只关闭当前 `EventSource`。在 1500 毫秒窗口内离开 run 页面，定时器会在已销毁的组件上触发，开出一个**永不会被关闭**的新 `EventSource`，其自身 `onerror` 又调度下一个。用户可见后果：连续点开多个活动 run 会累积永久打开的 SSE 连接，以及向已死组件信号写入的后台 `getRun`/`listContinuations`/`listRepairs` 调用。浏览器每源约 6 连接上限，最终应用完全停止加载数据直到刷新。修复需在 `ngOnDestroy` 清除定时器 ID 并加销毁标志判断。

**生产 CSP 封掉了内联主题引导脚本（已证实，高）。** `deployment/nginx/nginx.conf:52` 发送 `script-src 'self'`，无 `'unsafe-inline'`、无 nonce，而 `frontend/src/index.html:10-27` 是内联 `<script>`，在生产中被阻止。每次加载对每个深色模式用户有两个可见后果：Angular 启动前的浅色主题闪烁（恰是该脚本注释声明要防止的），以及来自 `runtimeConfig.appName` 的 `document.title` 永不生效，标签页显示 `index.html:5` 硬编码的 `RBAC 管理中心` 而非部署配置的名称。另每次页面浏览都产生一条 CSP 违规日志。修复：加 nonce/hash，或把逻辑移入 `main.ts`。

**任务详情事件流永久断开，且 nginx 在 30 秒杀掉它（已证实，高）。** `frontend/src/app/pages/devrail-task-detail/devrail-task-detail.ts:668-670` 的 `onerror` 只有 `source.close()`，与 run 页不同，**完全没有重连**。配合 `/api/` 代理上的 `proxy_read_timeout 30s`（`nginx.conf:67`），空闲的任务事件流在 30 秒后被 nginx 拆除且永不重建。用户可见后果：打开任务详情页等约 30 秒，随后在别处触发修复或续轮——页面静默停止更新（`task.dependencies.changed`、`repair.*`、`continuation.*` 全部停止到达），显示过期状态直到手动刷新。run 页仅因其重试循环才幸免。无论如何都应为流式端点提高 `proxy_read_timeout`。

**服务端可控 URL 直接赋给 `link.href`，绕过 Angular 净化（代码路径已证实，可利用性疑似，若可达则高）。** `frontend/src/app/pages/devrail-run/devrail-run.ts:157-164` 的 `link.href = artifact.downloadUrl`。`downloadUrl` 是 API 返回的必填 `string`。DOM 属性赋值得不到模板 `[href]` 所享有的 URL 净化，故 `javascript:` 或 `data:text/html` 值会在应用源内、携带会话 Cookie 的情况下于点击时执行。经 `devrail-run.html:324` 可达。是否可利用取决于后端能否被诱使输出受攻击者影响的 `downloadUrl`（本次未审计 Rust 侧该点），但前端两种情况下都没有防御。修复：赋值前按白名单校验协议。同一函数族的较低严重项：`:146` 的 `downloadPatch()` 把 `link.download = patch.fileName` 未经净化，使服务端数据控制保存文件名与扩展名。

**两个 SSE 端点硬编码 `/api/v1`，忽略运行时基址（已证实，中）。** `devrail-run.ts:481` 与 `devrail-task-detail.ts:644` 拼接 `` `/api/v1/...` `` 字面量，而其他所有请求都注入 `API_BASE_URL`（`frontend/src/app/core/runtime-config.ts:57-60`），后者可经 `window.__ARC_ADMIN_CONFIG__.apiBaseUrl` 覆盖。任何设置了不同 `apiBaseUrl` 的部署（这正是 `config.js` 存在的意义）会得到正常的 REST 调用但两个事件流 404——实时 run 日志与任务更新双双失效，且因两个 `onerror` 都静默吞掉错误，用户看不到任何提示。

**未加守卫的 `localStorage.setItem` 可能中断应用启动（已证实，中）。** `frontend/src/app/core/theme.service.ts:31-39` 无守卫写入，而 `:13-16` 的读取用 `typeof localStorage === 'undefined'` 仔细守卫过。`apply()` 由构造函数调用（`:18`），`ThemeService` 由 `LayoutComponent` 注入（`layout.ts:49`）。在存储访问抛异常而非缺失的环境下——Safari 禁用 Cookie 与站点数据、部分企业策略、存储配额耗尽——`setItem` 在构造期间抛 `QuotaExceededError`，导致整个已认证外壳无法渲染（白屏，而非降级的主题）。同一个 41 行文件内守卫风格不一致，正是这点让它看起来像疏漏而非决定。

**`routerLink` 绑定到自由格式的服务端字符串（疑似，中）。** `frontend/src/app/pages/devrail-notifications/devrail-notifications.html:42` 的 `[routerLink]="notification.deepLink"` 是应用中唯一不由字面段加 ID 构成的 `routerLink`，其余全部只插入数字 ID。若用户能影响投递给他人的通知的 `deepLink`，即可控制接收者在应用内的落点。范围仅限应用内导航（非外部开放重定向），影响有界，但值得按已知路由形状校验。

**原始服务端错误消息逐字渲染给用户（已证实，中）。** `frontend/src/app/core/api-error.ts:9-13` 未校验、未限长地返回服务端字符串，5xx 亦然（`:11` 仅对 `<500` 提前返回，5xx 只追加 trace ID），触达全应用约 60 处 snackbar 与约 16 处错误横幅。因始终经插值渲染，**这不是 XSS**，风险是信息泄露：任何把数据库错误、文件路径或内部标识放进 `error.message` 的后端 500，都会逐字展示给点击者。注意 trace ID **确实**做了约束（`:24-31`，字符集加 64 字符上限）——消息本身应获得同等处理，或 5xx 回落到通用文案。

**信号中对象的原地修改（已证实，低）。** `frontend/src/app/pages/devrail-notifications/devrail-notifications.ts:33-35` 修改 API 响应对象后再用 `[...this.notifications()]` 强制新数组标识以让 OnPush 察觉。目前可用，但破坏了 `track notification.id` 的复用推理，且在列表被共享或记忆化后会出错。两个方法之下的 `markAllRead`（`:42-44`）已用不可变 `map` 正确实现。

**其他清理项（已证实，低）。** `frontend/src/app/pages/audit-logs/audit-logs.ts:134-138` 的 2 秒「已复制」重置定时器未在销毁时清除，用户复制后立即导航会向已销毁组件写信号（无害，但与 SSE 那条同类）。`frontend/src/app/app.routes.ts:9,18-20` 的 `PUBLIC_PATHS`（`403`/`404`/`500`）是死代码——`authGuard` 只应用于 `security` 路由（`:199`），而该路由从不属于该集合；错误路由完全无守卫，父级 `''`/`LayoutComponent` 路由亦无守卫，故未认证用户访问 `/404` 会渲染完整外壳（无数据泄露，因导航按权限过滤，见 `layout.ts:76-78`，但代码注释表达的意图与接线不符）。`nginx.conf:76-80` 的静态资源 `location` 块自带 `add_header`，而 nginx **不会**把父级 `add_header` 继承进定义了任何 `add_header` 的块，故 CSP、HSTS 与 `nosniff` 不会随 `.js`/`.css`/字体响应发送。

### 文档与实现一致性

本节按「文档声称已完成，而实现或证据不支持」为判定标准。文档中明确标注「待验收」「未实现」的谨慎表述不计为缺陷。

**所有被引用的门禁证据都是指向被 gitignore 文件的死链（已证实）。** 四份文档引用同一份 `codex-audit-pipeline/.codex/reports/test_result.md` 作为其测量数字的依据：`docs/HANDOFF.md:13`、`docs/verification/mvp-acceptance-2026-08-28.md:34` 与 `:40`、`docs/verification/backend-test-throughput-2026-08-29.md:81`。该文件不存在（只有 `test_result.json`），且永远无法提交：`.gitignore:11` 忽略 `codex-audit-pipeline/.codex/reports/`，`codex-audit-pipeline/.gitignore:2-3` 再次强化；对该目录执行 `git ls-files` 只返回 agent 与模板配置，无任何报告。因此每一条 `TEST_SUMMARY: PASS` 声明都是未受版本控制的本机状态。测试计数也在漂移且无对账：`HANDOFF.md:13` 称后端 146，`devrail-implementation-status.md:48` 与 `mvp-acceptance:34` 称 151，吞吐文档各行称 153/154/156/157，本机未跟踪的 JSON 为 157。风险：读者把「151/113/69, TEST_SUMMARY: PASS」当作可审计证据，而仓库内无任何东西能确认它。

**续轮功能的「Playwright 专项测试已通过」不成立（已证实）。** `docs/devrail-implementation-status.md:22` 声称续轮具备「Angular Vitest、Playwright 专项测试已通过」。Vitest 那半成立且分量充足（`frontend/src/app/pages/devrail-task-detail/devrail-task-detail.spec.ts` 有 141 处续轮/修复引用，`devrail-run.spec.ts` 有 22 处）。Playwright 那半**不存在**——全仓恰好两个 Playwright 规格，且对 `frontend/e2e/` 全目录 grep `continuation|harness|/devrail` 零命中。同仓 `docs/HANDOFF.md:66` 与之矛盾，正确地把 Playwright 验收列为未完成。HANDOFF 是对的，`devrail-implementation-status.md:22` 超额声明。

**没有任何 DevRail 功能具备针对真实后端的浏览器覆盖（已证实）。** `frontend/e2e/authenticated-user-flow.spec.ts:93` 安装 `page.route('**/api/v1/**')` 拦截全部 API 调用，3 个测试全部跑在 21 份手写 fixture 上——这是「带 fixture 的 UI 测试」，不是端到端，无法捕获后端契约漂移。`frontend/e2e/fullstack-smoke.spec.ts` 是唯一针对真实后端的规格：1 个测试、70 行，仅覆盖 arc-admin 基线（登录、TOTP、建用户、登出），DevRail 面零覆盖。净结果：项目、任务、run、Harness、审批、质量门禁、通知、评论与评审均无浏览器级的真实后端验证。公允的缓解：smoke 规格**确实**接入了完整门禁（`.arc-flow/flow.toml:329-338`，并挂载了 Postgres 服务），`backend/tests/openapi_contract.rs` 守卫 Rust 到 `openapi.json` 的漂移，且 fixture 按生成的模型定型。另注 smoke 配置只跑桌面端（`playwright.fullstack.config.ts` 单个 `fullstack-chromium` 项目），故 `README.md:20` 所称「移动端 E2E」完全基于 fixture。

**13 个数据库测试在无数据库时静默空过（已证实）。** `backend/src/db.rs:512-518` 在 `TEST_DATABASE_URL` 未设置时返回 `None`，13 个调用点随即提前 `return`——报告成功而零断言执行。例如 `backend/src/repositories/devrail.rs:1568`、`:1691`、`:1813`、`:2165`；`backend/src/services/devrail_repairs.rs:1365`；`backend/src/services/devrail_runs.rs:1285`；`backend/src/repositories/devrail_continuations.rs:2731`；以及 `backend/src/db.rs:582`。正式门禁确实供给 Postgres（`codex-audit-pipeline/tools/arc-flow/src/config.rs:961-990`），故 `cargo flow` 是诚实的。风险在于开发者跑裸 `cargo test` 看到全绿，便认定调度器/修复/续轮路径已通过。

**Web Push 投递链路零测试，且在所有已提交配置中均为惰性（已证实）。** `backend/src/workers/notification_dispatcher.rs`（102 行）无任何测试，且在 VAPID 密钥或 subject 缺失时于 `:29-32` 立即返回。`WEB_PUSH_VAPID_PUBLIC_KEY`/`PRIVATE_KEY` 只在 `backend/src/config.rs:199-200` 读取，且不出现在任何 `.env.example`、`compose.production.yaml` 或 CI 工作流中——因此该 dispatcher 在仓库任何地方都不会真正运行。`backend/src/services/devrail_push.rs` 仅有一个测试覆盖 `fingerprint()`（哈希合理性，`:144-149`），`backend/src/repositories/devrail_push.rs` 无测试。`docs/devrail-implementation-status.md:25` 与 `:44` 称投递重试、投递审计与 Grafana 告警「已完成」。代码确实存在且在 `backend/src/main.rs:165` 接线，这部分站得住；但 `requirements.md` §16 要求对撤销、永久失败、临时重试与幂等提供自动化测试，而这些一个都没有。`mvp-acceptance-2026-08-28.md:21` 正确地标为待验收。

**被称为「已加入」的功能背后存在零测试模块（已证实）。** 对照 `docs/devrail-implementation-status.md:26` 与 `:46-47`：`backend/src/services/devrail_comments.rs`（171 行）与 `backend/src/repositories/devrail_comments.rs` 无测试，而评论/提及/编辑/软删除/审计被称完成；`backend/src/workers/branch_cleanup.rs`（120 行）无测试，而临时分支创建加后台远端删除与失败重试被称完成；`backend/src/handlers/devrail.rs`（1128 行）无测试，路由级覆盖仅有 `backend/tests/api_flow.rs` 中 3 个测试对应 `backend/src/lib.rs` 注册的 109 条路由。其他无测试文件：`repositories/devrail_reviews.rs`、`devrail_review_comments.rs`、`devrail_external_review_comments.rs`、`devrail_pull_requests.rs`、`devrail_notifications.rs`、`devrail_approvals.rs`、`devrail_workspaces.rs`；`workers/approval_expiry.rs`、`workers/artifact_cleanup.rs`。两个单测试模块断言极弱：`services/devrail_approvals.rs:140-152` 只检查一个静态字符串映射（近乎同义反复），`services/devrail_reviews.rs` 只测 GitHub 线程状态 JSON 解析——两者都不触及审批或评审的决策逻辑。

**吞吐文档未达成其自设的验收目标（已证实）。** `docs/verification/backend-test-throughput-2026-08-29.md:61` 将目标 3 设为记录 1/2/4 线程各两轮的成功率、耗时、数据库锁等待、CPU 与峰值内存。结果表（`:24-38`）只记录了耗时；CPU 与峰值 RSS 仅在 `:122` 对整个门禁出现一次，而非分线程档；锁等待从未量化。全文未给出主机核心数，故 `:76` 的「4 线程相对 1 线程缩短约 47%」在另一台机器上不可比。应得的肯定：该文档的结构性声明全部可验证——`.arc-flow/flow.toml:275` 的 `test_threads = 4`、`:276` 的环境变量覆盖、`backend/src/db.rs:357` 的 `test_schema_pool`，以及恰好 13 处 `DATABASE_TEST_LOCK` 点与 `backend-test-inventory-2026-08-29.md` 中的计数吻合。

**门禁存在已定义但不阻断的步骤（已证实）。** `workflow.framework-release-config` 与 `workflow.framework-upgrade-tests` 定义为步骤但不在 `policy.required_steps` 中（`.arc-flow/flow.toml:28-51`），因此不阻断门禁。

**`extracted/` 是孤立且未记录的目录（已证实，低）。** `extracted/stitch_arco_design_dashboard/` 下有 29 个已提交的 Stitch/Arco 设计稿文件（HTML、PNG 截图、token txt）。除 `.arc-flow/flow.toml:191` 把该目录的变更归入 "workflow" 范围组件外，仓库中无任何东西引用它们。该目录不在 `README.md:52-67` 的目录树中，故那份清单不完整。

## 经核实确实做得好的部分

记录这些一方面是为了说明覆盖范围，另一方面是为了避免后续重构误伤这些正确的设计。

### 会话、CSRF 与凭据

会话令牌为 `getrandom` 产生的 32 字节，仅存 SHA-256 哈希（`src/auth.rs:43-61`、`src/services/auth.rs:164-165`）。`auth_context` 在**同一条查询**内强制撤销、绝对过期、空闲超时、`token_version`、`deleted_at`、`status='active'` 与「超管必须有 TOTP」谓词（`src/repositories/auth_sessions.rs:59-78`）。

CSRF 同时是同步器令牌与双提交：头必须等于 Cookie **且**哈希后等于服务端值，两次比较都用 `ct_eq`（`src/auth.rs:221-231`）。由于校验位于 `authenticate()` 内部，每条已认证路由都自动获得它，**无豁免路径**；`POST /auth/logout` 也确实取 `ActorContext`，在覆盖内。Cookie 为 `HttpOnly`/`SameSite=Strict`/`Secure`，生产带 `__Host-` 前缀（`src/auth.rs:125-150`、`src/main.rs:128`）。

权限变更无需轮换会话：`auth_context` 每次请求都从 `user_roles`/`role_permissions` 重算 `data_scope` 与 `permission_codes`，故角色修改立即生效。口令变更会递增 `token_version` 并撤销全部会话；管理员重置、停用、删除、恢复码使用、passkey 撤销与恢复码重新生成同样如此。

Argon2id 配 16 字节随机盐，并有 dummy-hash 路径使未知用户名与已知用户名耗时一致（`src/services/auth.rs:85-93`）。限流覆盖三个维度且账号键小写化，正确阻止了以大小写变体规避大小写敏感的 `find_by_username`。恢复码经 `WHERE used_at IS NULL ... RETURNING` 实现单次使用（`src/repositories/mfa.rs:300-316`）。升权令牌经哈希存储、绑定会话、绑定范围、单次使用、300 秒 TTL（`src/repositories/step_up.rs:29-53`）。

AES-256-GCM 每次加密使用新的随机 nonce 且以 `owner_id` 作为 AAD，故无 nonce 复用且密钥与用户绑定（`src/mfa.rs:72-92`）。`danger-allow-state-serialisation` 使用正确：WebAuthn 状态只存在于服务端 `auth_mfa_challenges.state`，按哈希令牌索引、300 秒 TTL、单次使用，且 `finish_passkey_authentication` 会重新校验返回的 `cred_id` 属于该挑战的用户（`src/services/mfa.rs:595-608`）。内部错误从不进入客户端响应体（`src/error.rs:96-100`，`:180` 有回归测试）。已对全部 `tracing::` 调用 grep 口令/令牌/密钥/Cookie 插值，零命中。

### 数据范围与 SQL

用户、部门、审计日志与仪表盘均通过递归 `visible_departments` CTE 贯穿 `data_scope`/`organization_id`/`department_id`。`validate_role_grant_scope` 通过拒绝授出 actor 自身不具备的权限来阻断提权（`src/services/users.rs:606-613`）。DevRail 侧的裸全局 ID 路由（`/runs/{id}/*`、`/artifacts/{id}`、`/continuations/{id}`、`/repairs/{id}/*`、`/approvals/{id}/*`、`/workspaces/{id}/cleanup`、`/tasks/{task_id}/*`）全部先经带范围的查询解析；通知、推送设备与评论修改都固定到调用者本人。

**SQL 注入：未发现缺陷（已实测统计）。** `src/repositories/` 中全部 164 处 `AssertSqlSafe` 使用只插入编译期固定片段；57 处 `AssertSqlSafe(format!` 的插值项经统计仅为列名常量（`COLUMNS`、`APPROVAL_COLUMNS`、`RUN_COLUMNS`、`ROLE_SELECT`）与 `scope("a")` 这类字面别名。每个动态片段都解析为字面量：`src/repositories/users.rs:167` 的 `{order_by}` 来自对 `UserSort` 枚举的穷尽 match 返回 `&'static str`（`:113-125`）；`devrail.rs:344` 的 `{table}`/`{columns}`/`{alias}` 在三个调用点（`:354`、`:447`、`:532`）全部硬编码；`data_scope.as_str()` 是封闭枚举（`src/access.rs:15-23`）且是绑定而非插值。LIKE 模式与 IN 子句全部参数化（`ILIKE '%' || $6 || '%'`、`= ANY($3)`）。唯一实际问题是语义而非安全：`src/repositories/users.rs:189` 的 `format!("%{k}%")` 未转义 `%`/`_`，故关键词为 `%` 时匹配全部行。

**审计日志追加保护有效，且归档程序与之兼容。** `guard_audit_log_row_mutation`（`migrations/20260808120000_protect_audit_logs.sql:3-16`）对 UPDATE 无条件抛异常，仅当 `arc_admin.audit_maintenance = 'on'` 时放行 DELETE；TRUNCATE 由 `:34` 独立的语句级触发器封堵。INSERT 不受影响，故 `audit_logs::record` 正常工作。`delete_archived`（`src/repositories/audit_logs.rs:36-52`）在自身事务内以 `is_local = true` 设置该标志——这是正确选择，可防止该权限泄漏给下一个使用该池化连接的用户。

**审计与业务变更的原子性由类型系统强制（已实测核实）。** `audit_logs::record`（`src/repositories/audit_logs.rs:56`）的第一个参数是 `&mut PgConnection` 而非 `&PgPool`，因此**无法**在事务外调用。约 90 处调用点全部传入 `&mut transaction`；抽查 `src/services/roles.rs:275-300` 的删除流程确认业务变更与审计写入同事务提交。`src/` 中无任何 `acquire()` 调用。这是数据层设计最好的一处。

**无事务跨外部 await（已实测扫描）。** 对 8 个同时含 `begin()` 与 HTTP/进程调用的文件做了跨度扫描，`reqwest`、`Command::new`、`spawn`、web-push、文件 IO 与 sleep 全部**零命中**落在未提交事务内。那些近似危险的调用都被刻意排在 `begin()` 之前：`src/services/devrail_reviews.rs:182-221`、`src/workers/harness_supervisor.rs:1242-1265`、`src/services/devrail_workspaces.rs:786`/`:893`，以及 `src/services/devrail_artifacts.rs:261`（先写文件，并在回滚与提交失败两条路径上都有 `remove_file` 补偿）。另独立确认两个 worker：`src/workers/branch_cleanup.rs:78-96` 与 `src/workers/notification_dispatcher.rs:44`。

**计数器全部在 SQL 侧完成**（`x = x + 1`），12 处均无读改写。**worker 抢占**全部使用单条原子 CTE 配 `FOR UPDATE SKIP LOCKED` 与状态守卫。**事务性 outbox 完好**——通知与 outbox 写入方都取 `&mut PgConnection`，P2 中 webhook 那处是唯一把外层边界画错的地方。全部事务运行于 READ COMMITTED，`src/` 中无任何隔离级别提升。

以下守卫经核实正确强制：`decide`/`withdraw` 在 UPDATE 内部失败关闭、续轮 `create` 取 `FOR UPDATE OF t, r` 并重校验 revision、`append_event` 在 run 行锁上串行化、`prepare_transport_recovery` 与 `record_hook_failure` 确有上限。

### CI、容器与运维

**无 `pull_request_target`、`workflow_run` 或 `issue_comment` 触发器**（`.github/` 全目录），无 fork PR 密钥泄露路径。**无脚本注入**：`github.event.*` 的唯一使用就是 P1 第 1 条的两处 `if:`，无任何事件数据插入 `run:` 块。

**无提交过的凭据。** `git log -S` 对 `AKIA`、`POSTGRES_PASSWORD=`、`MFA_ENCRYPTION_KEY=` 的命中全部落在 `codex-audit-pipeline/.codex/secrets.toml` 中的检测器**模式**上。`deployment/.env.production`、`backend/.env`、`observability/.env` 存在于磁盘、被正确 gitignore（`.gitignore:5-6`、`backend/.gitignore:2`），`--diff-filter=A` 确认三者从未被提交。

**门禁是真实的，不是装饰。** 任何工作流与脚本中零 `continue-on-error`、零 `|| true`、零软失败模式。`.arc-flow/flow.toml:242-524` 真实执行 `cargo fmt --check`、`clippy -D warnings`、`cargo test --locked`、`eslint --max-warnings=0`。

**容器加固强。** 全部应用服务具备 `read_only: true`、`cap_drop: [ALL]`、`no-new-privileges:true`、`mem_limit`/`pids_limit`/`cpus`；非 root `USER 10001:10001` 与 `101:101`；两个镜像均有健康检查；`data` 网络为 `internal: true`；后端与数据库无宿主端口；TLS 材料经 compose `secrets` 提供，不走环境变量或构建参数。

**可观测性未暴露。** `observability/compose.yaml` 中每个端口都绑定 `127.0.0.1`（Tempo 3200、Prometheus 9090、blackbox 9115、Loki 3100、Alloy 12345、Grafana 3000）。Grafana 经 `:?` 强制口令，并禁用注册与匿名认证。

**密钥失败关闭。** `DATABASE_URL`、`APP_HOST`、`MFA_ENCRYPTION_KEY`、`POSTGRES_PASSWORD`、`GRAFANA_ADMIN_PASSWORD` 全部使用 `${VAR:?...}`。唯一的弱 `:-` 默认值是 `GRAFANA_ADMIN_USER:-admin`（用户名，非凭据）。`.env.production.example:5-6` 保持口令字段为空，`scripts/check-production-deployment.mjs:76-80` 强制该约束。

**`.dockerignore` 正确。** 排除 `.git`、`**/.env`、`**/.env.*`、`deployment/tls`，仅重新纳入 `.env.example` 类文件，故全仓构建上下文（`context: .`）不携带任何密钥。

**脚本干净。** 无 `curl|sh`，无 `--insecure`/`-k`/`NODE_TLS_REJECT_UNAUTHORIZED`/`danger_accept_invalid`/`sslmode=disable`。`start.sh` 使用 `set -Eeuo pipefail`，全部展开加引号并有正确的 trap 处理。全部 Rust 子进程派生使用 `.arg()`（不经 shell），`src/config.rs:369` 拒绝 `DEVRAIL_HARNESS_COMMAND` 中的路径分隔符。

**脚手架是活的，不是废弃的。** `cargo flow` 解析到仓库内 crate（`.cargo/config.toml` 别名 → `codex-audit-pipeline/tools/arc-flow`）。全部 worker 在 `backend/src/main.rs:140-165` 派生。每个脚本都可达——零引用的 `.mjs` 文件由其 `.sh` 包装器调用，`start-fullstack-smoke.sh` 由 `playwright.fullstack.config.ts:20` 使用。

**测试基础设施比文档给人的印象更可靠。** 156 个后端测试，零 `#[ignore]`。集成套件（`backend/tests/api_flow.rs`，2398 行）使用 schema 隔离的真实 Postgres 连接池（`db::test_schema_pool`）构建真实路由，连 CSRF 与 `X-Forwarded-For` 代理信任链都在测试内实际走过，未 mock 掉数据层。

**无隐藏跳过、无桩。** Rust 中零 `#[ignore]`；前端与 E2E 中零 `.skip`/`xit`/`xdescribe`/`.fixme`/`test.fail`。`backend/src` 与 `frontend/src` 全域零 `todo!()`、`unimplemented!()`、`TODO`、`FIXME`、`HACK`。

**openspec 无漂移。** `openspec/changes/` 无未完成变更，7 个全部归档且任务复选框全勾（22/34/30/25/42/42/17）。

**修复流程的可信证据声明测试充分。** `backend/src/services/devrail_repairs.rs` 中 10 个测试覆盖伪造、过期、跨范围、重放幂等、变更集漂移与敏感字段拒绝，与文档声称一致。

**前端基线扎实。** 手写代码中无 `innerHTML`、`bypassSecurityTrust*`、`DomSanitizer`、`eval`；每个 `@for` 都有 `track`；完全没有 `effect()`/`resource()`/`linkedSignal`/`ChangeDetectorRef`，故不存在信号误用类问题；`strict: true` 加 `strictTemplates`；生成代码之外零 `any`。生成的客户端与 `docs/openapi.json` **完全同步**（138 个操作，零漂移，无路径/方法不匹配，已实测）。守卫真实阻断导航（`permissionGuard` 返回 `UrlTree`，不是仅隐藏 UI 的布尔值）且无竞态：`ensureSession()` 经 `sessionCheck` 去重，`refreshPermissions()` 在每次导航时从服务端重取并带 epoch 守卫（`src/app/core/auth.service.ts:267-285`）丢弃在 `clearSession()` 之后到达的响应。`withCredentials` 加仅对非安全方法且仅对 API 目标 URL 附加 `X-CSRF-Token`（`src/app/core/auth.interceptor.ts:36-46`），Cookie 解析容忍畸形值。`localStorage` 中无令牌——唯一存储的键是主题（`'dark'`/`'light'`），升权令牌逐请求传递且从不持久化。两处非空断言（`users.ts:493`、`departments.ts:222`）都在数行之上有可证的守卫。对话框焦点管理对 `topbar`/`main` 使用 `inert` 并处理 Escape 与焦点归还（`layout.html:116,175`、`layout.ts:115-132`），抽样的交互控件都是真实 `<button>`/`<a>` 且带 `aria-expanded`/`aria-controls`/`aria-label`。

**文档在最要紧处是诚实的。** `requirements.md` §16 把 10 项 DoD 复选框全部留空并明确写「不表示当前已经完成」；`mvp-acceptance-2026-08-28.md` 把 10 行全标为待验收；`README.md:7` 警告 MVP 未实现；`devrail-implementation-status.md:50-55` 列举了读者不应得出的结论。

## 建议修复顺序

1. **P1 第 1、2 条。** 各一两行改动，却直接决定后续所有扫描是否真的在保护你。去掉两处 `github.event.repository.private == false` 守卫；移除 `deny.toml` 与 `security.yml` 中的 `RUSTSEC-2023-0071` ignore。
2. **P1 第 5 条（连接池 12 处）。** 机械改动、零行为变更，且 `mfa.rs:188` 在登录路径上。此条不需要任何权限、不需要创建新角色即可触发，只要并发量到了就会发生，因此优先级在授权类之前。
3. **P1 第 6 条中的 `mark_quality_gate_failed` 与门禁租约。** 前者是加 `rows_affected` 检查与 OLD 状态谓词；后者需为 `renew_gate_rerun_claim` 接线或把租约提到超过命令超时，并为表增加 attempts 列以终止无限重复。
4. **P1 第 3、4、7 条（授权与签名覆盖）。** 第 3 条为 `review_id` 加带范围的解析；第 4 条把 `repository_id` 与 `event_id` 移入签名覆盖范围，并让缺失 `event_id` 的请求失败关闭；第 7 条为 `decide` 补 `a.requested_by <> $3`。
5. **P2 第 6 条（门禁 tag 固定）需同时改 `check-supply-chain.mjs`**，否则正确的修复会被门禁拒绝。
6. **P1 第 6 条的 `claim_harness_start`。** 修复面最大（需要真正的数据库级抢占语义与 `controls` map 的所有权修正），建议独立变更处理。

## 未覆盖与后续建议

- 事务原子性与迁移锁风险已在本报告补全，但**迁移在真实数据量下的锁持有时长未实测**，`audit_logs` 上那两个 GIN 索引的实际阻塞窗口需在预生产环境测量。
- P1 第 4 条中 `downloadUrl` 的后端可控性未审计（需追踪 Rust 侧产物 URL 的生成来源）。
- P2 中 `npm audit --omit=dev` 的实际影响未实测（需网络）。
- 建议将本报告的 P1 条目转为 openspec 变更并逐条附修复后的命令输出，而非在本文件内标注「已修复」。
