# Grafana 告警通知配置

Grafana 已自动装载告警规则。仓库提供生产联系点和默认通知策略的 provisioning 文件，但机器人地址等部署秘密只通过环境变量注入。每个生产环境都必须独立配置并完成送达验收。

## 前置条件

- `observability/` 日志栈已经启动且三个容器状态正常；
- Grafana 容器可以访问通知服务的公网地址；
- 已准备企业微信群、钉钉群、SMTP 账号或 HTTPS Webhook；
- 操作者具有 Grafana 管理员或告警管理权限。

开发环境只启动基础栈，不创建生产联系点：

```bash
docker compose --env-file observability/.env -f observability/compose.yaml up -d
```

## 生产配置供应

在每个生产环境受保护的 `observability/.env` 中设置以下值：

```env
GRAFANA_ALERT_CONTACT_TYPE=wecom
GRAFANA_ALERT_WEBHOOK_URL=<由部署环境或密钥系统注入>
```

`GRAFANA_ALERT_CONTACT_TYPE` 支持 `wecom`、`dingding` 或 `webhook`。地址必须与类型匹配；使用 `webhook` 时，接收端必须理解 Grafana Webhook JSON。不要把真实地址写入 `.env.example`、Compose、provisioning 文件、工单或日志。

生产环境同时加载基础栈和告警覆盖：

```bash
docker compose --env-file observability/.env \
  -f observability/compose.yaml \
  -f observability/compose.production-alerting.yaml \
  up -d
```

生产覆盖会在类型或地址为空时直接拒绝解析 Compose。Grafana 启动后自动创建 `arc-admin-production-alerts` 联系点，并把默认通知策略绑定到该联系点；通知按 `alertname` 分组，保留恢复通知。

Grafana 默认只监听本机的 `http://127.0.0.1:3000`。管理员用户名默认为 `admin`，查看初始化生成的密码：

```bash
sed -n 's/^GRAFANA_ADMIN_PASSWORD=//p' observability/.env
```

远程服务器不要直接向公网开放 3000 端口，可在管理电脑建立 SSH 隧道：

```bash
ssh -L 3000:127.0.0.1:3000 <user>@<server>
```

然后在管理电脑打开 `http://127.0.0.1:3000`。

## 推荐方案：企业微信

### 1. 创建群机器人

1. 在企业微信中进入用于接收告警的群聊；
2. 打开群设置，进入“群机器人”；
3. 添加机器人并命名为“系统告警”；
4. 复制机器人 Webhook 地址。

地址格式类似：

```text
https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=xxxxxxxx
```

Webhook 地址具有发送权限，按密码管理：不要写入文档、工单、日志或 Git 仓库，泄露后立即在企业微信中重新生成。

### 2. 配置生产环境变量

登录 Grafana，依次进入：

```text
Alerts & IRM -> Alerting -> Notification configuration -> Contact points
```

使用企业微信时设置：

```env
GRAFANA_ALERT_CONTACT_TYPE=wecom
GRAFANA_ALERT_WEBHOOK_URL=<企业微信机器人 Webhook 地址>
```

重启 Grafana 后进入 Contact points，打开只读的 `arc-admin-production-alerts` 联系点，点击“Test”并发送测试通知。

## 钉钉

1. 在钉钉告警群中添加“自定义机器人”并复制 Webhook 地址；
2. 在机器人安全设置中使用自定义关键词，例如“告警”；
3. 设置 `GRAFANA_ALERT_CONTACT_TYPE=dingding`，并通过 `GRAFANA_ALERT_WEBHOOK_URL` 注入机器人地址；
4. 确保自定义通知内容始终包含配置的安全关键词；
5. 发送测试通知，成功后保存。

地址格式类似：

```text
https://oapi.dingtalk.com/robot/send?access_token=xxxxxxxx
```

当前 Grafana DingDing 联系点的核心配置是机器人 URL、消息类型和消息内容。若组织强制使用 Grafana 当前界面未提供的签名方式，应改用受控的中转 Webhook，由中转服务完成签名和转发。

## 邮件

Grafana OSS 必须先配置 SMTP，单独在联系点中填写收件人不会生效。SMTP 参数应由部署环境或密钥管理系统注入 Grafana 容器，至少包括：

| Grafana 环境变量 | 说明 |
| --- | --- |
| `GF_SMTP_ENABLED=true` | 启用 SMTP |
| `GF_SMTP_HOST` | SMTP 主机和端口，例如 `smtp.example.com:465` |
| `GF_SMTP_USER` | SMTP 用户名 |
| `GF_SMTP_PASSWORD` | SMTP 密码或授权码 |
| `GF_SMTP_FROM_ADDRESS` | 发件地址 |
| `GF_SMTP_FROM_NAME` | 发件人名称，例如“系统告警” |

当前 `observability/compose.yaml` 没有注入这些变量。上线邮件通知前，需要在具体部署配置中增加环境变量映射并重建 Grafana 容器，不能把真实账号或密码提交到仓库。

SMTP 生效后，在 Contact points 中新建 `Email` 联系点，填写一个或多个收件地址并发送测试邮件。使用企业邮箱时，应优先使用独立告警账号和专用授权码，不要使用个人邮箱密码。

## 通用 Webhook

接收端应提供 Grafana 容器可以访问的 HTTPS 地址，并能接收 `POST` JSON。创建联系点时 Integration 选择 `Webhook`，至少填写 URL；按接收端要求配置 Bearer Token、Basic Auth、附加 Header 或 HMAC。

不要同时配置 Basic Auth 和 Authorization Header。生产环境建议启用 HMAC-SHA256，并在接收端校验签名和时间戳，避免伪造与重放。

测试时应确认接收端返回 HTTP 2xx，并记录 Grafana 告警指纹或事件 ID；不要在接收端日志中输出认证凭据和完整请求头。

## 检查通知策略

生产 provisioning 已创建默认通知策略。进入：

```text
Alerts & IRM -> Alerting -> Notification configuration -> Notification policies
```

确认默认策略包含以下配置：

| 配置项 | 建议值 |
| --- | --- |
| Default contact point | `arc-admin-production-alerts` |
| Group by | `alertname` |
| Group wait | `30s` |
| Group interval | `5m` |
| Repeat interval | `4h` |

Provisioning 管理的策略不能在界面修改。需要增加分级路由时，修改生产 provisioning 文件并重新部署，例如：

- `severity=critical`：企业微信加邮件或值班 Webhook；
- `severity=warning`：企业微信或钉钉告警群。

一个联系点可以包含多个 Integration。如果严重告警必须同时到达多个渠道，可在同一联系点中增加多个 Integration，避免只配置了其中一个渠道。

## 上线验收

依次完成以下检查：

1. Contact points 中的测试通知成功送达；
2. Notification policies 中的默认策略或子策略已经引用该联系点；
3. `Alert rules` 中“应用错误日志突增”和“HTTP 5xx 持续出现”均正常评估；
4. 使用受控测试触发一条告警，确认收到 `Firing` 通知；
5. 测试条件恢复后，确认收到 `Resolved` 通知；
6. 在 `Alert activity -> Active notifications` 中核对匹配策略、联系点和发送状态；
7. 记录告警接收人、轮值安排、升级方式和机器人密钥轮换责任人。

不要为了测试在生产环境制造真实故障。优先使用 Grafana 的测试通知；必须验证完整规则链路时，应在测试环境或维护窗口使用可回滚的受控测试数据。

## 常见问题

### 测试通知超时

检查 Grafana 容器的 DNS、HTTPS 出网、代理和防火墙配置。通知由 Grafana 容器发送，容器内的 `127.0.0.1` 指向容器自身，不是宿主机代理。

### 测试成功但真实告警没有消息

通常是联系点未绑定通知策略，或子策略标签没有匹配。先将已测试的联系点设为 Default contact point，再检查告警规则的 `severity` 等标签。

### 消息重复过多

检查 `Group by` 和 `Repeat interval`。不要按 `trace_id`、`user_id` 等高基数字段分组；基础规则使用 `alertname` 即可。

### 重建后联系点丢失

确认启动命令包含 `compose.production-alerting.yaml`，且环境变量仍由密钥系统注入。Provisioning 联系点会在 Grafana 重建时恢复，不依赖 Grafana 数据卷中的手工配置。

### 查看发送错误

进入 Contact points 查看最近一次投递状态，红色状态会显示错误详情；再结合 Grafana 容器日志排查网络、认证、限流或消息格式问题。

## 官方参考

- [Grafana 联系点](https://grafana.com/docs/grafana/latest/alerting/configure-notifications/manage-contact-points/)
- [Grafana 通知策略](https://grafana.com/docs/grafana/latest/alerting/configure-notifications/create-notification-policy/)
- [Grafana Webhook 联系点](https://grafana.com/docs/grafana/latest/alerting/configure-notifications/manage-contact-points/integrations/webhook-notifier/)
- [Grafana 邮件联系点](https://grafana.com/docs/grafana/latest/alerting/configure-notifications/manage-contact-points/integrations/configure-email/)
- [Grafana 告警配置供应](https://grafana.com/docs/grafana/latest/alerting/set-up/provision-alerting-resources/file-provisioning/)
- [钉钉自定义机器人](https://open.dingtalk.com/document/orgapp/custom-robot-access)
