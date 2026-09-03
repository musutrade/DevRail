## Context

实现继续遵守 `Handler → Service → Repository → PostgreSQL`，SQL 写操作只在 Repository、迁移和测试中出现。所有新增状态采用 additive migration，旧记录可读取且不改变来源 run 的历史。

## Decisions

### 1. 目标绑定

外部评论同步先用参与者、组织、task/project/repository 联合查询解析 review；Repository 写入再次验证 organization_id。Webhook 原生格式从 GitHub `repository.id` 或 GitLab `project.id` 读取目标，并用 HMAC body 的 SHA-256 摘要作为缺失投递 ID 的稳定身份。

### 2. 生命周期线性化

审批恢复使用 `awaiting_approval → starting` 的条件更新；Harness 启动只接受带 key 且仍为 `starting` 的 run。质量门禁执行在数据库中记录 claim token、开始时间和终态，过期 claim 才能被接管。

### 3. 兼容与回滚

迁移只增加列/索引/约束，不删除历史数据。回滚代码前先关闭新入口，保留已写入的状态和审计；Webhook 客户端升级前可继续使用直接 DevRail payload，但必须包含 `eventId` 或由 body 派生。

## Risks

- 外部平台 payload 结构变化会导致 fail-closed，需要供应商适配测试。
- 900 秒门禁命令与执行 claim 的过期窗口必须有余量，避免健康进程被重复接管。
- 移除 RustSec ignore 可能暴露现有 web-push 传递依赖，需要单独的依赖替换或有期限风险接受。
