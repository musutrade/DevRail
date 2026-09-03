# ADR-0009：DevRail 安全边界与终态并发保护

- 状态：Proposed
- 日期：2026-09-03
- 关联审计：[`docs/verification/security-audit-2026-09-01.md`](../verification/security-audit-2026-09-01.md)
- 关联变更：[`2026-09-03-devrail-security-boundaries`](../../openspec/changes/2026-09-03-devrail-security-boundaries/)

## 背景

DevRail 的组织、评审、Webhook、审批和质量门禁路径已经具备基本数据范围与幂等模型，但审计发现几个边界仍依赖调用方诚实：外部评论同步没有把评审绑定到请求仓库，Webhook 的仓库和投递标识可由未签名请求头改写，审批发起人可以处理自己的审批，质量门禁失败可以覆盖已经完成的运行。审批恢复和 Harness 启动也需要在数据库中明确唯一的启动所有权。

## 决策

1. 外部评审同步必须在访问外部平台前，以组织、评审参与者、任务项目和仓库的联合谓词解析目标；写入和软删除同时携带组织边界。外部评论身份使用 `(organization_id, provider, external_id)` 唯一键。
2. Webhook 只信任 HMAC 覆盖的请求体。原生 GitHub/GitLab payload 从体内读取仓库 ID；缺少供应商投递 ID 时从已签名体派生稳定摘要；拒绝空密钥、缺失事件身份和头体目标不一致。数据库更新必须再次验证组织。
3. 审批决策在 Service 和 Repository 两层拒绝 `requested_by = actor.user_id`，并在同一事务前完成只读快照查询。审批恢复先以条件更新把 run 从 `awaiting_approval` 转为 `starting`，再竞争 Harness 启动租约。
4. 质量门禁失败只允许把仍处于可失败运行状态的 run/task 转为失败；终态已完成、取消或已成功的记录保持不可变。并发执行使用持久化执行标记，避免同一 run 重复启动门禁命令和生成产物。
5. 所有 Harness 启动都必须带稳定 `harness_start_key`，数据库条件更新是唯一抢占点；进程内 controls map 不覆盖已有控制通道。
6. CodeQL、依赖审查和 RustSec 门禁不得因仓库可见性跳过；依赖漏洞忽略只能基于真实依赖路径和有期限的风险接受，并不得误称为未启用驱动。

## 取舍与后果

- 旧的、没有事件 ID 的 Webhook 客户端需要升级 payload；对缺少 ID 的已签名体使用摘要派生保持重试幂等，但相同字节的独立事件会合并。
- 质量门禁执行状态增加 additive migration；异常终止后由过期时间允许一次受控接管，不通过直接改数据库恢复。
- 组织复合唯一键会改变跨组织重复外部 ID 的数据库约束，但不改变 API 响应字段。
- 本 ADR 不把本地隔离演练当作 staging 或生产验收，也不新增测试专用认证入口。

## 验收条件

- 外部评论、Webhook、审批自批、终态保护和 Harness 抢占均有真实 PostgreSQL 或单元回归测试。
- 迁移可在空库和已有数据上正向执行，OpenAPI 不发生无关漂移。
- `cargo flow verify --all`、OpenSpec 严格校验和 PR required checks 成功后，才将本 ADR 更新为 Accepted。
