# 日志与故障定位

后端使用 `tracing` 输出运行日志。开发环境默认使用紧凑文本，生产环境默认使用 JSON Lines；每一行都是独立 JSON 对象，可直接由容器平台或日志采集器处理。

## 运行配置

```env
LOG_FORMAT=json
RUST_LOG=info,tower_http=info,sqlx=warn
SERVICE_NAME=arc-admin-backend
```

`LOG_FORMAT` 只允许 `pretty` 或 `json`。未设置时，`APP_ENV=production` 默认使用 `json`，其他环境默认使用 `pretty`。项目初始化命令会按项目标识生成独立的 `SERVICE_NAME`。

## 请求关联

后端为每个请求生成 `x-request-id`，并同时作为日志中的 `trace_id`。合法的上游 ID 会被保留；空值、超过 64 字符或包含非字母数字及 `.`、`_`、`-` 的值会被替换。

所有响应都会返回 `X-Request-ID`。HTTP 500 错误响应示例：

```json
{
  "error": {
    "code": "INTERNAL_ERROR",
    "message": "服务器内部错误",
    "traceId": "e2f39c0d-ef21-4bed-9c8b-b67e8f71a743"
  }
}
```

前端会显示“问题编号”，管理员可使用该编号查询同一次请求的访问日志和错误根因。panic 也会转换为相同的 JSON 500 契约。

## 字段规范

请求日志固定记录 `service`、`environment`、`version`、`trace_id`、`method`、匹配后的 `route`、`status_code`、`latency_ms` 和认证后的 `user_id`。错误事件额外记录 `event`、`error_code` 与服务端根因。

访问日志不记录原始 URI 查询串、完整 Header、请求体或响应体。禁止写入 `Authorization`、Cookie、JWT、密码、验证码、Token、数据库连接串和 SQL 参数值。auditor 会拦截完整 Header 和显式敏感日志字段。

数据库 `audit_logs` 负责“谁修改了什么”，JSON 运行日志负责“请求为什么失败”，二者用途不同，不应互相替代。

审计日志会保存产生该记录的 `trace_id`。管理端“审计日志”页面支持按追踪号搜索和复制，因此可以从用户看到的“问题编号”同时定位运行日志与业务变更。后台任务不在 HTTP 请求上下文中执行时，该字段为空。

## 指标与集中日志栈

`observability/` 提供单机 Prometheus、Blackbox Exporter、Loki、Alloy 和 Grafana：

- Prometheus 抓取后端 `/metrics`，包含 HTTP 请求量、耗时和 PostgreSQL 连接池状态；
- Blackbox Exporter 从监控网络探测 Nginx 健康入口；
- Alloy 从 Docker stdout 或本地 JSONL 文件读取日志，解析 P0 的结构化字段；
- Loki 使用 TSDB v13 保存和查询日志；
- Grafana 自动装载 Prometheus/Loki 数据源、运行指标/集中日志仪表盘和基础告警。

生产 Compose 会创建专用 `arc-admin-monitoring` 网络，后端 `/metrics` 和内部 Nginx 健康入口只对该网络开放，不通过公网反向代理。若只启动本地可观测性栈，先执行一次 `docker network create arc-admin-monitoring`。然后创建本地配置并修改管理员密码；通过项目初始化命令创建的业务仓库已经自动生成该文件和随机密码：

```bash
cp observability/.env.example observability/.env
docker compose --env-file observability/.env -f observability/compose.yaml up -d
```

入口仅绑定到本机：Grafana 为 `http://127.0.0.1:3000`，Prometheus 为 `http://127.0.0.1:9090`，Blackbox 调试端点为 `http://127.0.0.1:9115`，Loki 为 `http://127.0.0.1:3100`，Alloy 调试界面为 `http://127.0.0.1:12345`。停止服务但保留数据：

```bash
docker compose --env-file observability/.env -f observability/compose.yaml down
```

不要在需要保留历史日志时添加 `--volumes`。

## 接入日志

本地直接运行后端时，将 JSON Lines 同时写入 Alloy 监听目录：

```bash
cd backend
APP_ENV=development LOG_FORMAT=json cargo run 2>&1 \
  | tee ../observability/logs/backend.jsonl
```

生产环境采集容器 stdout。后端容器必须输出 JSON，并使用以下 Docker 标签显式加入采集范围：

```yaml
services:
  backend:
    environment:
      APP_ENV: production
      LOG_FORMAT: json
      SERVICE_NAME: stock-analysis-backend
    labels:
      observability.logs: enabled
      observability.service: stock-analysis-backend
      observability.environment: production
```

`observability.service` 和 `observability.environment` 是应用配置失败、尚未建立 tracing span 时的兜底值。正常日志以 JSON 中的 `SERVICE_NAME` 和 `APP_ENV` 为准。

Alloy 只把低基数的 `service_name`、`environment`、`level` 建为 Loki 标签。`trace_id`、`user_id`、`event`、`route`、`status_code`、`latency_ms` 和 `error_code` 保存为结构化元数据，避免请求号或用户号造成索引基数爆炸。

## Grafana 查询

“应用集中日志”仪表盘可按服务、环境、级别和追踪号筛选。也可以在 Explore 中直接使用 LogQL：

```logql
{service_name="stock-analysis-backend", environment="production"}

{service_name="stock-analysis-backend", level="ERROR"}
  | trace_id="e2f39c0d-ef21-4bed-9c8b-b67e8f71a743"

{service_name="stock-analysis-backend"} | status_code >= 500
```

## 保留与容量

默认配置面向单机和中小规模项目：日志保留 30 天，租户摄入上限 8 MB/s、突发 16 MB，每个日志流 3 MB/s，单行最大 256 KB。Alloy 还会在采集端把超长日志截断到 256 KiB。

Loki 数据存放在 Docker 命名卷 `loki-data`。应监控宿主机磁盘并在使用量达到 70% 前扩容或缩短保留期。正式多节点、高可用或合规归档场景不要继续使用本地文件系统，应迁移到 S3、GCS、Azure Blob 或兼容对象存储，并为对象存储配置备份和生命周期策略。

## 告警

Grafana 默认装载以下规则：

- “应用错误日志突增”：5 分钟内 ERROR 超过 5 条并持续 5 分钟，级别 `warning`；
- “HTTP 5xx 持续出现”：5 分钟内 5xx 超过 3 条并持续 2 分钟，级别 `critical`。
- “应用入口不可用”：内部 Blackbox 探测连续 2 分钟失败，级别 `critical`。

规则会立即评估。生产环境使用 `observability/compose.production-alerting.yaml` 额外挂载联系点与默认通知策略，类型和 Webhook 地址只从部署环境注入，不写入模板。首次部署后仍需按照 [Grafana 告警通知配置](grafana-alert-notifications.md)完成测试通知和真实告警演练；`Firing` 与 `Resolved` 均成功送达后，才可视为生产告警闭环完成。

## 外部心跳

同一宿主机上的 Blackbox 只能覆盖应用、反向代理和容器网络故障，无法发现宿主机断电、出口中断或整个可用区故障。生产环境必须再使用位于本系统和部署区域之外的 HTTP 心跳服务，每 1 分钟请求一次 `https://<APP_HOST>/nginx-healthz`，连续 2 至 3 次失败即通知值班联系人。

外部平台中至少设置 HTTPS 证书校验、期望状态码 `200`、5 秒超时、两个不同地区的探测点，以及独立于本机 Grafana 的通知渠道。完成配置后手动停止前端容器进行演练，确认内部 Grafana 告警与外部心跳通知都能到达；再临时阻断整台主机的外网入口，确认只有外部心跳仍能告警。外部平台账号、API 密钥和联系人不进入模板仓库。

## 可选链路追踪

后端支持 OTLP/HTTP 和 W3C `traceparent`。默认 `OTEL_EXPORTER_OTLP_ENDPOINT` 为空，不创建导出器；启用本机 Tempo 时，将生产环境值设为 `http://tempo:4318`，再启动 tracing profile：

```bash
docker compose --env-file observability/.env \
  -f observability/compose.yaml --profile tracing up -d
```

Grafana 会自动装载 Tempo 数据源，可从 trace 的 `trace_id` 属性回查同一个用户可见 `x-request-id` 日志。Tempo 仅在监控网络接收 OTLP，不暴露 4317/4318 宿主机端口；`http://127.0.0.1:3200` 只用于本机健康诊断。

本地 Tempo 默认保留 72 小时，适合故障排查，不是审计归档。span 属性不得包含密码、Cookie、Token、完整请求体或个人敏感字段；跨公网发送到托管 Collector 时，应由同机 Collector 或服务网格负责 mTLS 和密钥注入，不把凭据写进仓库或 OTLP URL。

## 安全边界

Alloy 为发现容器而只读挂载 Docker socket，但持有该 socket 的进程仍属于宿主机高权限信任边界。日志栈只能部署在受控节点，不要向公网暴露 Loki 或 Alloy。Grafana 对外发布时应置于启用 TLS、访问控制和限流的反向代理之后，并使用密钥管理系统注入管理员密码。

## 本地排查

捕获 JSON 日志后，可按问题编号筛选：

```bash
rg 'e2f39c0d-ef21-4bed-9c8b-b67e8f71a743' application.jsonl
```

也可以生成同一请求的紧凑错误上下文：

```bash
cargo flow parse-logs \
  --input application.jsonl \
  --output /tmp/error-context.json
```

解析器优先选择第一条 ERROR 日志，支持从事件字段、`span`、`spans` 中读取 `trace_id`，并保留同一请求最多 30 条上下文。
