# DevRail 项目公约

## 1. 适用范围

本公约适用于 DevRail 的 Rust 后端、Angular 前端、Codex harness 集成、数据库迁移、通知 worker、部署配置和文档。它补充 arc-admin 基线，不替代根目录 `AGENTS.md`、`docs/architecture.md`、`docs/development.md` 和 `docs/ui-design-system.md`。

## 2. 工程边界

### 2.1 后端分层

请求必须沿 `Router -> Handler -> Service -> Repository -> PostgreSQL` 流动：

- Handler 只解析请求、提取 `ActorContext`、声明权限并映射错误；禁止写 SQL、启动子进程或调用推送供应商。
- Service 负责状态机、策略、事务边界和业务编排；禁止直接写 SQL。
- Repository 是运行时 SQL 读写唯一入口；组织、部门和所有者过滤必须进入 SQL。
- Worker 只能调用 Service；不得绕过权限和审计服务直接修改业务表。
- migration 只追加，不修改已经应用的 migration。

### 2.2 前端分层

前端依赖方向固定为 `app 组合根 -> features/devrail -> core`：

- `core` 只提供认证、运行时配置、拦截器、基础 API 和通用 UI 能力，不依赖 DevRail feature。
- DevRail 页面、数据访问、模型和测试放在 `frontend/src/app/features/devrail/`。
- 生成的 OpenAPI 客户端只能由 Rust 契约生成，禁止手工修改。
- 页面使用 Angular 22 standalone、signals、OnPush/zoneless 兼容模式和 Material M3 token。
- 用户可见文案、按钮、状态、Tooltip 和 ARIA 标签统一使用简体中文。

### 2.3 框架兼容

DevRail 通过 `.arc-project.json` 登记为 arc-admin 派生项目。框架更新必须使用模板仓库的 `upgrade-framework.sh`，不得复制粘贴覆盖业务文件。以下标识暂时保留以兼容 arc-admin 生成契约和质量脚本：

- Rust crate：`arc-admin-backend`；
- Angular project target：`arc-admin`；
- 监控指标和审计维护参数：`arc_admin_*`、`arc_admin.audit_maintenance`。

重命名这些标识必须作为独立迁移，包含三方兼容窗口、数据/指标迁移、回滚方案和全量 CI 证据。

## 3. Codex harness 约定

### 3.1 运行入口

生产和长任务统一经后端 `codex app-server` Supervisor；`codex exec` 或 `@openai/codex-sdk` 仅用于明确登记的 CI/集成场景。浏览器不得连接 app-server 的 stdin/stdout、WebSocket 或 Unix socket。

### 3.2 工作区和权限

- 每个 run 固定 `cwd`、环境变量白名单、资源上限、超时和网络策略。
- 默认 workspace-write、网络关闭、禁止访问 workspace root 之外的路径。
- 生产环境、受保护分支、推送远端、删除文件和读取 Secret 必须被策略标记为 `review_required` 或 `blocked`。
- 审批请求必须包含脱敏命令/工具摘要、工作目录、影响范围、风险级别、策略版本和过期时间。
- run 的任务快照、harness 版本、thread/turn ID、事件游标和退出原因必须持久化。

### 3.3 事件和恢复

- `thread`、`turn`、`item` 事件按服务端游标顺序落库，幂等键由 run 和事件 ID 组成。
- SSE 只传递已脱敏的公开事件；断线后通过 cursor 补拉，不依赖浏览器内存。
- 取消先优雅中断，超时后才强制终止；进程重启后仅恢复数据库标记为可恢复的 run。
- 原始模型隐藏推理不是产品审计数据；界面只展示进度、工具调用摘要、结果和质量门禁。

## 4. 手机推送约定

- 业务事务必须先创建站内通知和 transactional outbox，dispatcher 再异步投递 Web Push/FCM/APNs。
- `event_id + recipient_id + channel` 是投递幂等键；worker 使用租约和 `FOR UPDATE SKIP LOCKED`。
- 临时失败指数退避，永久错误（例如 404/410）立即失效设备，不继续重试。
- 推送 payload 只含通知 ID、事件类型、简短脱敏标题/摘要和深链接；不得包含代码、命令参数、token、Cookie、私钥和完整日志。
- 用户必须显式授权浏览器推送；不支持 Web Push 时降级为站内通知，禁止显示虚假的“已开启”。
- 通知、设备注册/撤销、偏好修改、投递状态和供应商错误必须可审计；推送失败不能删除或隐藏站内通知。

## 5. 数据和安全

- 新业务表必须有 `organization_id`、可空 `department_id`、`owner_user_id`，外键保证组织一致。
- 后端必须用 `RequirePermission` 或等价的已验证上下文；前端按钮显隐永远不是安全边界。
- 不在日志、审计详情、错误响应、事件 payload、OpenAPI 示例或 Git 中保存秘密。
- SQL 写入只能出现在 Repository、migration、测试或 seed；审计表的维护开关只能由受控归档命令使用。
- 所有外部 URL、仓库凭据、环境 Secret 和推送密钥通过部署 Secret 管理注入。

## 6. Git 和 PR

- 分支使用 `<type>/<short-description>`，例如 `feat/devrail-task-runs`、`fix/push-retry`、`docs/governance`。
- `main` 只接受 PR，禁止直接推送、强制推送和在 PR 中混入无关格式化。
- PR 必须说明变更范围、数据迁移、权限影响、通知影响、安全考虑和回滚方案。
- 新增业务功能必须包含后端授权测试、前端权限交互测试和移动端行为（如适用）。
- 生成文件变更必须说明生成命令；禁止手改 `docs/openapi.json` 和 `frontend/src/app/generated/api/`。
- 合并前必须通过 `Quality gate`、`Backend verification`、`Frontend verification`；安全 workflow 失败时不得以重跑代替修复。

## 7. 变更完成定义

- 需求、API、数据模型、权限和 UI 文档同步；
- migration、Repository、Service、Handler、前端 feature 和测试均位于规定边界；
- `cargo flow scope` 与 `cargo flow verify --all` 通过；
- 推送链路验证了授权、幂等、临时失败、永久失败和撤销；
- 审计日志可从用户动作追溯到 run、审批、通知和投递；
- PR CI 成功且工作区无未提交文件。
