# 审计日志保留与归档

`audit_logs` 是安全审计证据，不是普通业务表。数据库触发器拒绝普通 `UPDATE`、`DELETE` 和 `TRUNCATE`；保留任务只能在归档落盘并生成校验清单后，通过 Repository 的事务级维护入口删除同一批 ID。

## 保留策略

默认在线保留 365 天，每批最多处理 10000 条：

```env
AUDIT_RETENTION_DAYS=365
AUDIT_ARCHIVE_BATCH_SIZE=10000
AUDIT_ARCHIVE_HOST_DIR=./deployment/audit-archives
```

金融、医疗或劳动人事项目应按合同和法规单独评审期限。缩短期限前必须确认远端归档可查询、校验和恢复，不允许直接执行删除 SQL。

## 执行归档

首次执行前创建仅部署账号和容器 UID `10001` 可写的目录，然后运行一次性维护服务：

```bash
mkdir -p deployment/audit-archives
sudo chown 10001:10001 deployment/audit-archives
docker compose --env-file deployment/.env.production \
  -f compose.production.yaml --profile maintenance run --rm audit-archive
```

每批生成一个 JSON Lines 文件和一个 `.manifest.json` 清单。清单记录文件名、记录数、首尾 ID、截止时间和 SHA-256；程序对归档和清单执行 `fsync` 与原子重命名后才提交数据库删除。任务无过期记录时可安全重复运行。

将命令加入受控调度器，每天执行一次。调度失败、归档目录容量超过 70% 或超过 24 小时没有成功清单时必须告警。

## 异地保存与验证

宿主机目录只是交接区，不是最终归档。后续上传任务应把 JSONL 和清单一起复制到启用版本控制、服务端加密、跨账号权限和对象锁定的对象存储；确认远端 SHA-256 一致后才能清理本地副本。

每季度抽取至少一个归档批次，在隔离环境逐行解析 JSON、核对清单记录数与 SHA-256，并验证关键登录、退出、会话撤销及 RBAC 变更事件可检索。数据库所有者或超级用户仍能停用触发器，因此高合规环境还必须限制数据库所有者凭据并使用不可变对象存储，不能把数据库触发器当作密码学签名。
