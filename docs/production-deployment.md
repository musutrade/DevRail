# 最小生产部署

## 组成

根目录的 `compose.production.yaml` 提供 PostgreSQL、一次性迁移、Axum API 和 Angular/Nginx 四个服务。后端与前端均使用多阶段镜像并以非 root 用户运行；只有 Nginx 暴露 80/443，数据库和 API 不映射宿主机端口。

Nginx 负责 TLS 终止、HTTP 到 HTTPS 跳转、SPA 静态资源、`/api/` 反向代理和安全响应头。后端只信任 Compose 固定的 `172.30.0.0/24` 代理网段，来源 IP 仍由服务端按受信代理链解析。

## 首次部署

1. 通过 `scripts/init-project.sh` 创建的业务项目已生成 `deployment/.env.production`；直接使用框架源仓库时，先从 `deployment/.env.production.example` 复制。设置 `APP_HOST`、强随机 `POSTGRES_PASSWORD`、完整的 URL 编码 `DATABASE_URL` 和 `MFA_ENCRYPTION_KEY`。该文件已被 `.gitignore` 排除。
2. 将受信 CA 签发的完整证书链和私钥放到 `deployment/tls/tls.crt`、`deployment/tls/tls.key`，私钥在宿主机设为仅部署账号可读。
3. 从仓库根校验并构建：

```bash
docker compose --env-file deployment/.env.production -f compose.production.yaml config --quiet
docker compose --env-file deployment/.env.production -f compose.production.yaml build
```

4. 先启动数据库和迁移，再启动应用：

```bash
docker compose --env-file deployment/.env.production -f compose.production.yaml up -d database
docker compose --env-file deployment/.env.production -f compose.production.yaml run --rm migrate
docker compose --env-file deployment/.env.production -f compose.production.yaml up -d backend frontend
```

5. 使用 `docker compose ps` 确认数据库、后端、前端健康，再执行登录、CSRF、权限拒绝和审计日志冒烟测试。

## 发布与回滚

每次发布先构建不可变版本镜像并完成数据库备份，再运行新镜像的 `/app/migrate`。迁移成功后替换 API/前端副本。SQL migration 只能向前兼容上一版应用；涉及删除列、重命名或收紧约束时使用“扩展、迁移数据、切换代码、最后清理”的多版本流程。

应用回滚只切回旧镜像，不自动回滚数据库。若迁移不向后兼容，必须使用已评审的补偿 migration 或按恢复手册恢复到新实例，不能直接修改 `_sqlx_migrations`。

## 生产检查

- `AUTO_MIGRATE=false`，所有 API 副本使用相同镜像和配置；
- `DB_MAX_CONNECTIONS × API 副本数` 小于 PostgreSQL 为应用保留的连接预算；
- TLS 证书自动续期，并在续期后滚动重载前端容器；
- 主机防火墙只开放 80/443 和受控运维入口；
- 容器日志由现有 Alloy/Loki 收集，告警联系点按 Grafana 文档配置；
- Compose 的 CPU、内存和进程限制按压测结果调整，不取消健康检查和最小权限配置；
- WAL 归档卷不是备份，必须持续复制到加密、版本化、跨账号或跨区域的对象存储。

规模增长到需要多主机调度、自动扩缩容或零停机证书管理时，再迁移到 Kubernetes 或托管容器平台；本模板不提前引入该复杂度。
