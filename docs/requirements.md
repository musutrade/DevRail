# DevRail 产品与技术需求文档

## 0. 文档信息

| 项目 | 内容 |
| --- | --- |
| 产品名称 | DevRail（开发轨道） |
| 文档版本 | 0.1.0 |
| 文档状态 | MVP 需求基线（未实现，待评审） |
| 编写日期 | 2026-08-21 |
| 基础项目 | [`musutrade/arc-admin`](https://github.com/musutrade/arc-admin) |
| 后端 | Rust + Axum + SQLx + PostgreSQL |
| 前端 | Angular 22 standalone + Angular Material |
| Agent 运行时 | OpenAI Codex harness（优先 `codex app-server`，兼容 SDK/CLI） |
| 目标用户 | 使用 AI 辅助开发的个人开发者、团队和平台管理员 |

本文件是 DevRail 的第一版产品、架构和验收基线。实现时应继续遵循 arc-admin 的项目公约：

> 实现状态声明：本文件描述的是完整 Codex Harness 开发系统 MVP 的目标，不是当前已交付能力清单。截至 2026-08-23，仓库已完成 arc-admin 基线、`arc-flow` 审计工具生产化、治理文档、Phase 0 项目/仓库/环境 CRUD API、仓库/环境创建入口、成员与项目策略 API/页面、任务列表筛选（含负责人和标签）、任务详情页、仓库/环境列表与详情页、仓库远端 HEAD/默认分支/分支数量同步和环境健康检查，以及 Phase 1 的 Harness Supervisor 基础闭环（受控 app-server、run 快照/事件、SSE、中断、thread/resume）、审批持久化/决策 API、审批中心 UI、审批撤回、审批过期 worker、changeset/质量门禁查询、受限命令质量门禁执行、结构化门禁元数据和终态 run 重试；Phase 2 已完成站内通知事实表、transactional outbox、run 终态通知、审批请求/批准/拒绝/撤回/过期通知、通知 API、用户通知偏好 API/页面、通知中心/设置页面和 Web Push 设备注册/列表/撤销；本地工作树状态、完整资源同步、更丰富的质量门禁日志后端、VAPID 初始化、Web Push dispatcher、投递重试和投递审计仍未完成。详见 [DevRail 实现状态](devrail-implementation-status.md)。

- 后端调用链固定为 `Router -> Handler -> Service -> Repository -> PostgreSQL`；SQL 写入只允许在 Repository、migration、测试或 seed 层。
- 前端业务代码放在 `features/<domain>`，共享认证、配置和权限能力放在 `core`。
- OpenAPI 由 Rust 类型生成，Angular 客户端由 OpenAPI 生成，不手工修改生成产物。
- 所有受保护 API 后端必须重新校验会话、权限码和组织/部门/所有者数据范围；前端守卫和按钮显隐不是安全边界。
- 新增业务表默认包含 `organization_id`、可空 `department_id` 和 `owner_user_id`，并用外键约束组织归属。

---

## 1. 产品概述

### 1.1 产品定位

DevRail 是一个以 Codex harness 为智能体执行核心的 AI 开发工作台。它把代码仓库、需求、Agent 会话、工具调用、审批、测试、代码审查和通知连接成一条可追踪的开发轨道。

用户可以在浏览器中创建或导入项目，向 Codex Agent 提交任务，查看实时执行过程，批准高风险操作，检查文件变更和质量门禁，并在任务完成、失败或需要人工处理时收到手机推送。

### 1.2 核心价值

1. **可控**：应用掌握上下文、工具清单、权限和审批；Agent 不能绕过工作台的安全边界。
2. **可观察**：每次任务都能看到线程、回合、工具调用、命令、文件变更、测试和审批记录。
3. **可恢复**：任务、会话和执行结果持久化，浏览器断线后可恢复查看，失败可重试或从指定回合继续。
4. **可协作**：围绕项目和任务共享上下文、评论、审查和通知，而不是依赖个人聊天记录。
5. **可审计**：敏感操作、权限变化、审批决定和推送送达结果都有审计记录。

### 1.3 目标指标（MVP）

| 指标 | 目标 |
| --- | --- |
| 首次打开工作台到看到项目 | 不超过 3 秒（不含登录网络延迟） |
| Agent 事件从后端到浏览器 | P95 不超过 2 秒 |
| 推送任务从事件入队到调用供应商 | P95 不超过 10 秒 |
| 推送重试 | 临时失败至少重试 5 次，最长不超过 30 分钟 |
| 任务可恢复率 | 进程重启后可恢复未完成任务状态，不丢失已持久化事件 |
| 关键 API 错误率 | 正常负载下 P95 小于 1% |
| 移动端可用性 | Android Chrome PWA；支持 Web Push 的 iOS 已安装 PWA |

---

## 2. 范围

### 2.1 MVP 必须包含

- 复用 arc-admin 的登录、HttpOnly Cookie 会话、CSRF、MFA、组织、部门、用户、角色、权限和审计能力。
- 项目、工作区、代码仓库和运行环境的基本管理。
- Codex harness 任务创建、线程/回合状态、流式事件、工具调用记录和中断/恢复。
- 工具与命令审批；默认拒绝高风险动作。
- 文件变更摘要、Git 分支/提交信息、测试和质量门禁结果。
- 任务列表、任务详情、Agent 事件时间线、审批中心、通知中心。
- Web Push 手机推送注册、偏好设置、可靠投递、重试、撤销和送达状态。
- SSE 任务事件流；移动端不要求保持实时连接，使用推送唤醒后回到任务详情。
- OpenAPI 契约、Vitest 单测、Playwright 桌面/移动端 E2E、Rust 集成测试和迁移。

### 2.2 MVP 不包含

- 自研代码编辑器；第一版只提供变更查看、日志和跳转到外部 IDE 的链接。
- 多模型路由、模型训练或模型供应商计费系统。
- 自动合并到生产分支、自动部署生产环境。
- 原生 iOS/Android 客户端；先用 PWA/Web Push，原生适配作为第二阶段。
- 多租户跨实例调度、分布式 Agent 集群和弹性 Kubernetes 编排。
- 将 Codex app-server 直接暴露给公网浏览器。
- 通过推送载荷传输源代码、密钥、完整命令或敏感审计详情。

---

## 3. 用户、角色与权限

### 3.1 用户角色

| 角色 | 主要职责 | 默认数据范围 |
| --- | --- | --- |
| `super_admin` | 平台配置、组织、供应商密钥和全局审计 | `all` |
| `organization_admin` | 管理组织项目、成员、推送策略和仓库 | `organization` |
| `project_admin` | 管理项目成员、环境、工具策略和任务 | `organization` / 指定项目 |
| `developer` | 创建任务、查看项目、执行 Agent、提交审批 | `department_and_children` |
| `reviewer` | 查看变更、运行质量门禁、批准审查结论 | `department_and_children` |
| `observer` | 只读查看任务、事件和通知 | `self` 或授权项目 |

角色是默认建议，最终授权仍通过 arc-admin 的角色和权限目录完成。禁止通过前端角色名称替代后端权限码。

### 3.2 权限命名

权限前缀固定为 `devrail`，格式为 `<prefix>:<resource>:<action>`。建议首批权限如下：

| 权限码 | 用途 |
| --- | --- |
| `devrail:project:read` / `write` | 查看、创建和维护项目 |
| `devrail:repository:read` / `write` | 管理仓库连接和默认分支 |
| `devrail:environment:read` / `write` | 管理运行环境和环境策略 |
| `devrail:task:read` / `write` | 查看和维护开发任务 |
| `devrail:run:read` / `execute` / `interrupt` / `retry` | 查看、执行、中断和重试 Agent 运行 |
| `devrail:approval:read` / `approve` / `reject` | 查看和处理工具审批 |
| `devrail:review:read` / `write` | 查看和维护变更审查 |
| `devrail:notification:read` / `write` | 查看通知和设置通知偏好 |
| `devrail:push_device:read` / `write` / `revoke` | 管理个人推送设备 |
| `devrail:policy:read` / `write` | 管理项目工具、命令和推送策略 |
| `devrail:audit:read` | 查看 DevRail 业务审计 |

高风险动作（执行生产环境命令、写入受保护分支、批准危险工具）必须使用独立权限，不得复用通用 `write`。

---

## 4. 核心概念与状态机

### 4.1 领域对象

- **组织（Organization）**：租户边界，继承 arc-admin。
- **项目（Project）**：一组仓库、环境、成员、策略和任务的集合。
- **工作区（Workspace）**：一次 Agent 可操作的实际目录；可以是本地目录、临时 worktree 或受控执行目录。
- **仓库（Repository）**：Git 仓库连接、默认分支、远端 URL 和凭据引用。
- **环境（Environment）**：执行命令所需的运行时配置、工具白名单和密钥引用；密钥只在运行时注入，不进入事件和推送。
- **任务（Task）**：用户希望完成的工程目标，包含需求、验收标准、优先级、负责人和项目上下文。
- **运行（Run）**：任务的一次 Agent 执行尝试，关联一个 Codex thread 和多个 turn/item 事件。
- **审批（Approval）**：Agent 请求执行高风险工具或命令时生成的人工决策。
- **变更集（ChangeSet）**：运行期间检测到的文件变更、差异摘要、Git 分支和提交信息。
- **质量门禁（QualityGate）**：Lint、编译、单元测试、E2E、安全扫描和人工审查等可验证步骤。
- **通知（Notification）**：面向用户的站内通知和推送意图；推送送达是通知的一个渠道状态。
- **设备（PushDevice）**：用户授权接收推送的浏览器/PWA 或未来原生设备。

### 4.2 任务状态

```text
draft -> queued -> running -> awaiting_approval -> running
                         |                         |
                         v                         v
                    succeeded                 interrupted
                         |                         |
                         v                         v
                      archived <- failed <- retrying
```

规则：

- `draft` 可编辑；`queued` 之后需求正文、仓库和环境不可静默修改。
- `running` 只能有一个活动 run；同一任务的并发执行默认拒绝。
- `awaiting_approval` 不计为失败，审批超时由策略决定是暂停还是取消。
- `interrupted` 保留最后一个完成 item，可从该位置重试或新建 run。
- `succeeded` 只有在 Agent 完成且配置的质量门禁通过后才能进入。
- `failed` 必须有用户可读原因、原始 trace id 和可选重试入口。

### 4.3 运行状态

`created -> starting -> active -> waiting_for_approval -> active -> completed | failed | cancelled`。

状态转换必须由后端服务完成并写入状态历史；客户端不能通过直接改数据库或伪造事件改变状态。

---

## 5. 用户流程

### 5.1 首次使用

1. 用户使用 arc-admin 登录；`super_admin` 按既有流程完成 MFA。
2. 用户进入“项目”页面，创建项目或导入 Git 仓库。
3. 用户选择默认分支、执行环境、Agent 权限策略和质量门禁模板。
4. 用户在“通知设置”中授权浏览器推送，注册当前设备并选择通知类别。
5. 用户创建第一条任务，填写目标、上下文、验收标准和期望输出。

### 5.2 Agent 开发任务

1. 用户从项目或任务详情点击“开始运行”。
2. 后端创建 run，锁定任务快照，启动受控 Codex harness 会话。
3. 前端通过 SSE 接收 item、命令、文件变更、审批和进度事件。
4. Agent 请求高风险工具时，后端创建审批并将任务置为 `awaiting_approval`。
5. 审批人从桌面或手机通知进入审批详情，查看命令、工作目录、风险级别和影响范围后批准或拒绝。
6. Agent 继续执行；系统运行质量门禁并展示结果。
7. 任务完成、失败或需要人工介入时，系统写入站内通知并按偏好发送手机推送。

### 5.3 断线恢复

- 浏览器刷新后根据 `lastEventId` 补拉 SSE 缺失事件。
- SSE 断开超过重连窗口时，前端从 `/runs/{id}/events?after=<cursor>` 分页补齐。
- Agent 运行不依赖浏览器连接；浏览器关闭不会自动取消任务。
- 后端重启后，supervisor 根据数据库状态恢复可恢复的 run；无法恢复时明确标记 `failed` 并触发通知。

---

## 6. 功能需求

### 6.1 登录与账号安全（继承 arc-admin）

**FR-AUTH-001** 使用 arc-admin 的 HttpOnly、`SameSite=Strict` 服务端会话和 CSRF 双提交校验。

**FR-AUTH-002** 所有 DevRail API 从会话生成 `ActorContext`，同时校验用户、会话、组织、部门、角色、权限和账号状态。

**FR-AUTH-003** `super_admin` 的 TOTP、Passkey、恢复码、会话撤销和敏感操作二次认证沿用基线实现，不在 DevRail 重复实现。

**FR-AUTH-004** 设备注册、项目凭据引用、环境密钥引用和推送供应商配置不得在日志、错误消息、推送载荷或前端运行时配置中暴露。

### 6.2 项目和仓库

**FR-PROJECT-001** 用户可创建、查看、编辑和归档项目；项目必须属于组织，可选归属部门和负责人。

**FR-PROJECT-002** 项目字段至少包括：名称、slug、描述、组织、部门、负责人、状态、默认仓库、默认环境、质量门禁模板、通知策略、创建时间和更新时间。

**FR-PROJECT-003** 导入 Git 仓库时支持 HTTPS URL 和 SSH URL 的元数据校验；凭据只保存为加密引用或外部 Secret Manager 引用。

**FR-PROJECT-004** 项目支持成员和角色，成员变更写入审计；被移除成员不能继续读取项目或运行事件。

**FR-PROJECT-005** 仓库操作至少支持读取默认分支、分支列表、当前 HEAD、工作树状态和提交摘要；写操作必须经过权限和策略检查。

**FR-PROJECT-006** 第一版不在浏览器内直接编辑文件；用户可查看差异、复制路径、打开外部 IDE 链接或下载补丁。

### 6.3 环境与工具策略

**FR-ENV-001** 每个项目可配置一个或多个执行环境，环境包含工作目录来源、运行时版本、允许的网络策略、命令白名单、最大执行时长和资源限制。

**FR-ENV-002** 环境密钥使用名称引用，例如 `DATABASE_URL`、`GITHUB_TOKEN`；前端只能看到名称和是否已配置，不能读取值。

**FR-ENV-003** 默认环境采用最小权限：工作区可写、网络关闭、禁止访问工作区外路径；需要扩展权限时必须生成审批。

**FR-ENV-004** 项目管理员可配置命令风险等级：`safe`、`review_required`、`blocked`。规则匹配必须在后端执行。

**FR-ENV-005** 每次 run 记录最终生效的环境和策略快照，后续修改策略不能篡改历史运行记录。

### 6.4 Codex harness 接入

**FR-CODEX-001** MVP 优先通过后端受控进程调用 `codex app-server`；不能让浏览器直接连接 app-server。

**FR-CODEX-002** 后端负责 app-server 的进程生命周期、stdin/stdout JSONL、初始化握手、thread/start 或 thread/resume、turn/start、事件解析、超时和优雅中断。

**FR-CODEX-003** 每个 run 记录 `thread_id`、`turn_id`、harness 版本、模型标识、工作目录、权限策略、启动参数摘要和退出原因。

**FR-CODEX-004** app-server 的线程、回合和 item 事件按顺序持久化；事件必须包含单调游标、事件类型、发生时间和幂等键。

**FR-CODEX-005** 前端展示以下事件类型：用户输入、Agent 消息、推理摘要（如可用）、命令开始/结束、文件变更、工具调用、审批请求、质量门禁、错误和回合完成。

**FR-CODEX-006** 原始模型思维内容不是产品承诺；系统默认只展示可审计的进度和结果摘要，不展示或持久化不必要的隐藏推理。

**FR-CODEX-007** app-server 异常退出时，supervisor 必须记录 stderr 摘要、退出码、trace id 和恢复建议；不得把完整环境变量或凭据写入日志。

**FR-CODEX-008** 可选兼容层：对简单 CI 任务支持 `codex exec`，对 Node/TypeScript 集成支持 `@openai/codex-sdk`。三种入口必须共享任务、审批、事件和审计模型。

**FR-CODEX-009** 用户可中断活动 run；中断请求幂等，最多等待配置的优雅退出时间，超时才强制终止。

### 6.5 任务与运行

**FR-TASK-001** 创建任务时必填标题、目标和项目；可选填写背景、验收标准、约束、参考文件、负责人、优先级和截止时间。

**FR-TASK-002** 开始运行前生成不可变的任务快照，包括任务正文、项目策略、仓库 HEAD、环境和权限。

**FR-TASK-003** 用户可查看任务历史 run，比较不同 run 的状态、耗时、token 使用（如有）、命令数量、变更文件和质量门禁。

**FR-TASK-004** 运行失败时可一键重试；重试必须创建新的 run，不覆盖原 run；用户可选择从最新状态或指定 turn 继续。

**FR-TASK-005** 对任务和 run 的写操作需要幂等键；重复提交不能创建多个活动 run 或重复审批。

**FR-TASK-006** 列表页提供按项目、状态、负责人、创建时间和标签筛选，支持分页、排序和服务端搜索。

### 6.6 审批中心

**FR-APPROVAL-001** Agent 触发策略命中的工具或命令时，创建审批记录并暂停 run。

**FR-APPROVAL-002** 审批详情至少展示：任务、项目、发起者、命令/工具名、参数脱敏摘要、工作目录、影响范围、风险级别、过期时间和关联事件。

**FR-APPROVAL-003** 审批人只能批准自己有权限且在数据范围内的请求；不能批准已过期、已取消或不属于当前策略版本的请求。

**FR-APPROVAL-004** 批准、拒绝、撤回、过期和自动拒绝都写入审计，并触发相应站内通知；审批决定不可修改，只能追加更正事件。

**FR-APPROVAL-005** 默认审批有效期为 15 分钟，可按项目策略配置 5 至 60 分钟；过期后 Agent 必须收到明确拒绝原因。

### 6.7 变更、Git 与质量门禁

**FR-CHANGE-001** 每个 run 展示工作区状态、变更文件清单、增删行数、差异摘要和敏感文件告警。

**FR-CHANGE-002** 检测到 `.env`、凭据文件、私钥、生产配置或大文件时，显示安全警告；默认不将文件内容发送到推送或通知正文。

**FR-CHANGE-003** 质量门禁支持 Lint、格式检查、编译、单元测试、E2E、安全扫描和自定义脚本；每个门禁记录命令、版本、退出码、耗时、摘要和日志引用。

**FR-CHANGE-004** 受保护分支写入、创建提交、推送远端和合并请求属于独立高风险动作，必须单独权限和可配置审批；MVP 默认只生成补丁或提交到临时分支。

**FR-CHANGE-005** 质量门禁失败时，任务状态为 `failed` 或 `awaiting_approval`（由策略决定），必须显示可重试入口和失败原因。

### 6.8 通知中心与手机推送

#### 6.8.1 渠道

MVP 提供两个渠道：

1. **站内通知**：始终写入 PostgreSQL，用户打开工作台即可查看，不能因推送失败而丢失。
2. **Web Push**：使用 VAPID 的标准 Web Push，支持 Android Chrome PWA 和支持 Web Push 的 iOS 已安装 PWA。未来原生客户端通过同一 `PushProvider` 接口接入 FCM/APNs。

#### 6.8.2 通知事件

| 事件 | 默认级别 | 默认推送 | 说明 |
| --- | --- | --- | --- |
| `task.assigned` | info | 否 | 任务被分配给用户 |
| `run.started` | info | 否 | Agent 开始运行 |
| `run.succeeded` | success | 是 | 任务完成并通过配置门禁 |
| `run.failed` | error | 是 | Agent、环境或质量门禁失败 |
| `run.interrupted` | warning | 是 | 运行被中断或无法恢复 |
| `approval.requested` | action_required | 是 | 需要人工批准才能继续 |
| `approval.expired` | warning | 是 | 审批超时，运行暂停或失败 |
| `review.requested` | action_required | 是 | 请求审查变更 |
| `review.completed` | info | 可选 | 审查结论可用 |
| `security.alert` | critical | 是 | 检测到凭据、危险命令或策略违规 |
| `mention.created` | info | 可选 | 用户被评论或任务提及 |

通知类型、接收人、优先级、静默时段、项目覆盖和是否发送手机推送都必须可配置。

#### 6.8.3 设备注册

**FR-PUSH-001** 浏览器仅在用户明确点击“开启手机通知”后请求推送权限；禁止页面加载即弹系统权限框。

**FR-PUSH-002** 注册请求包含 `endpoint`、`p256dh`、`auth`、浏览器/设备名称、平台、时区和客户端版本；服务端为当前用户和组织建立设备记录。

**FR-PUSH-003** 同一设备重复注册必须幂等；用户退出、撤销权限或收到供应商的永久失败后，设备状态变为 `revoked` 或 `invalid`。

**FR-PUSH-004** 用户可查看最近设备、最后活跃时间、最近送达状态，并逐台撤销设备；撤销后不能再发送推送。

**FR-PUSH-005** iOS PWA 不满足系统推送前置条件时，界面必须解释原因，并提供站内通知作为降级路径，不显示“已开启”假状态。

#### 6.8.4 投递可靠性

通知写入业务事务时同时写入 transactional outbox，禁止在 HTTP 请求中直接调用推送供应商。

```text
业务事件
  -> outbox_events（同一事务）
  -> notification_dispatcher
  -> notifications（站内通知）
  -> notification_deliveries（每个设备/渠道）
  -> PushProvider
  -> delivered | retrying | failed | invalid
```

要求：

- 使用 `event_id + recipient_id + channel` 生成幂等键，避免重复推送。
- Worker 使用 PostgreSQL `FOR UPDATE SKIP LOCKED` 领取任务；MVP 不引入 Redis 作为可靠性前置条件。
- 临时错误使用指数退避和抖动，重试次数、下次重试时间和最终错误可查询。
- 永久错误（410/404、无效 token、权限撤销）立即标记设备失效，不继续重试。
- 高优先级通知可绕过普通批量窗口，但仍受用户静默时段和组织策略约束；`critical` 是否突破静默时段必须显式配置。
- 同一任务在短时间内产生的重复进度通知必须合并；推送只发送状态变化和行动入口，不发送每条 Agent item。
- 推送 payload 只包含 `notification_id`、事件类型、简短标题、脱敏摘要和深链接；代码、命令参数、token、Cookie、密钥和完整日志禁止进入 payload。
- 深链接打开后必须重新认证并经过权限检查；过期或无权限时显示统一错误页。

#### 6.8.5 通知偏好

用户可按事件类型配置：站内开关、手机推送开关、重要级别、静默时段、时区和项目覆盖。组织管理员可关闭某些高风险事件的外发推送，但不能删除站内审计记录。

### 6.9 审计与可观测性

必须审计：

- 项目、仓库、环境、策略和成员变更；
- Agent run 创建、启动、中断、取消、重试、失败和完成；
- 工具审批请求、批准、拒绝和过期；
- 设备注册、撤销、推送偏好和供应商错误；
- 受保护分支、推送、提交和导出动作；
- 管理员修改 VAPID/FCM/APNs 配置或环境 Secret 引用。

指标至少包括：活动 run 数、run 成功/失败率、审批等待时长、工具执行时长、SSE 连接数、outbox backlog、推送成功率、推送 P95 延迟、永久失败设备数和数据库连接池健康度。

---

## 7. 数据模型与迁移

以下为逻辑模型，实际字段类型和命名应与 arc-admin 现有表及 SQLx 迁移风格对齐。所有新增 migration 只允许追加，不修改已应用 migration。

### 7.1 项目域

#### `devrail_projects`

| 字段 | 要求 |
| --- | --- |
| `id` | 主键 |
| `organization_id` | 非空，组织外键 |
| `department_id` | 可空，同组织部门外键 |
| `owner_user_id` | 非空，用户外键 |
| `slug` | 组织内唯一，小写安全字符 |
| `name` / `description` | 项目名称和说明 |
| `status` | `active` / `archived` |
| `default_repository_id` / `default_environment_id` | 可空引用 |
| `notification_policy` | JSONB，版本化策略快照 |
| `created_at` / `updated_at` | UTC 时间 |

索引：组织+状态、组织+slug、负责人、更新时间。

#### `devrail_repositories`

保存远端 URL、协议、默认分支、凭据引用、最近同步状态和最近 HEAD。远端认证信息不得保存明文。

#### `devrail_project_members`

保存项目、用户、项目角色、加入者、加入时间和撤销时间；项目成员唯一约束为项目+用户。

#### `devrail_environments`

保存环境名称、工作区策略、网络模式、工具策略、资源上限、Secret 引用和启用状态。Secret 值不入库。

### 7.2 任务与 Agent 域

#### `devrail_tasks`

保存项目、组织、部门、所有者、标题、目标、背景、验收标准、约束、负责人、优先级、状态、标签、截止时间和归档信息。

#### `devrail_task_snapshots`

在 run 开始时保存任务正文、仓库 HEAD、环境、权限和策略的不可变 JSONB 快照，便于复现和审计。

#### `devrail_runs`

保存任务、snapshot、thread id、turn id、harness 版本、模型标识、状态、启动/结束时间、退出码、token 使用摘要和失败分类。

#### `devrail_run_events`

保存 run、游标、幂等键、事件类型、公开 payload、原始 payload 的安全摘要、发生时间和可见性。原始 payload 必须过滤凭据和敏感参数；大日志存对象存储或受控文件引用，不直接无限写入 PostgreSQL。

#### `devrail_approvals`

保存 run、事件、请求工具、脱敏参数、风险级别、请求者、审批者、决策、决策原因、过期时间和策略版本。决策更新采用追加审计，不覆盖原始请求。

#### `devrail_changesets`

保存 run、分支、基准 SHA、当前 SHA、变更文件统计、敏感文件告警和补丁引用。

#### `devrail_quality_gates` / `devrail_quality_gate_runs`

前者保存项目门禁定义，后者保存每次 run 的实际执行结果、命令摘要、退出码、耗时、日志引用和状态。

### 7.3 通知与推送域

#### `devrail_notification_preferences`

唯一键为用户+项目（项目可空表示全局），保存事件类别开关、渠道开关、静默时段、时区和版本。

#### `devrail_push_devices`

保存用户、组织、设备名、平台、浏览器、endpoint 加密值或受保护引用、endpoint 指纹、`p256dh`、`auth`、状态、最后活跃时间、最后错误和撤销时间。endpoint 指纹用于幂等和查询，原始 endpoint 不用于日志。

#### `devrail_notifications`

保存接收人、组织、项目、事件类型、级别、标题、脱敏正文、资源类型/ID、深链接、已读时间、过期时间和来源事件幂等键。通知是站内事实记录。

#### `devrail_notification_deliveries`

每个通知与设备/渠道一行，保存状态、尝试次数、供应商消息 ID、最后错误、下次重试时间、送达时间和失效时间。

#### `devrail_outbox_events`

保存待处理的业务事件、聚合类型/ID、事件版本、payload、幂等键、处理次数、锁定时间、下次处理时间和最终错误。成功处理后保留一段时间用于排障和重复检测。

### 7.4 保留策略

- 通知：默认保留 180 天；用户可删除已读视图，但不能删除组织审计事实。
- 推送投递记录：默认保留 90 天；聚合指标长期保留。
- Agent 事件和日志：默认保留 30 天，可按组织配置为 7 至 365 天。
- 审计记录按 arc-admin 的审计保留与归档规则执行。
- 删除项目采用软删除/归档；真正物理删除必须是明确的管理员操作并有二次确认和审计。

---

## 8. API 需求

API 根路径为 `/api/v1`，所有非公开路由都使用 arc-admin 的会话、CSRF 和权限提取器。

### 8.1 项目与环境

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| `GET` | `/projects` | 分页查询项目 |
| `POST` | `/projects` | 创建项目 |
| `GET/PATCH` | `/projects/{projectId}` | 查看/更新项目 |
| `POST` | `/projects/{projectId}/archive` | 归档项目 |
| `GET/POST` | `/projects/{projectId}/members` | 查询/添加成员 |
| `DELETE` | `/projects/{projectId}/members/{userId}` | 移除成员 |
| `GET/POST` | `/projects/{projectId}/repositories` | 查询/导入仓库 |
| `GET/PATCH` | `/projects/{projectId}/environments/{environmentId}` | 查看/更新环境 |
| `GET/PATCH` | `/projects/{projectId}/policy` | 查看/更新项目策略 |

### 8.2 任务、运行与事件

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| `GET/POST` | `/projects/{projectId}/tasks` | 查询/创建任务 |
| `GET/PATCH` | `/tasks/{taskId}` | 查看/更新任务 |
| `POST` | `/tasks/{taskId}/runs` | 创建并启动 run |
| `GET` | `/tasks/{taskId}/runs` | 查询历史 run |
| `GET` | `/runs/{runId}` | 查看 run 状态和摘要 |
| `POST` | `/runs/{runId}/interrupt` | 中断 run |
| `POST` | `/runs/{runId}/retry` | 新建重试 run |
| `GET` | `/runs/{runId}/events` | 分页补拉事件 |
| `GET` | `/runs/{runId}/events/stream` | SSE 实时事件流 |
| `GET` | `/runs/{runId}/changeset` | 查看变更集 |
| `GET` | `/runs/{runId}/quality-gates` | 查看质量门禁结果 |

SSE 要求支持 `Last-Event-ID`，事件必须带服务端游标；服务器重启后同一游标不可复用到另一 run。

### 8.3 审批

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| `GET` | `/approvals` | 查询当前用户可处理的审批 |
| `GET` | `/approvals/{approvalId}` | 查看审批详情 |
| `POST` | `/approvals/{approvalId}/approve` | 批准 |
| `POST` | `/approvals/{approvalId}/reject` | 拒绝 |

### 8.4 通知与推送设备

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| `GET` | `/notifications` | 分页查询站内通知 |
| `POST` | `/notifications/{id}/read` | 标记已读 |
| `POST` | `/notifications/read-all` | 全部标记已读 |
| `GET/PATCH` | `/notification-preferences` | 查看/更新偏好 |
| `GET` | `/push/config` | 获取公开 VAPID key 和能力信息 |
| `GET/POST` | `/push/devices` | 查询/注册当前用户设备 |
| `DELETE` | `/push/devices/{deviceId}` | 撤销设备 |
| `POST` | `/push/devices/{deviceId}/test` | 发送测试推送（需明确确认） |

### 8.5 错误与幂等

统一错误响应包含 `code`、`message`、`traceId` 和可选 `details`。不得把数据库、供应商或 shell 的原始敏感错误直接返回给浏览器。

写请求支持 `Idempotency-Key`；服务端按用户、路由和 key 保存短期结果，重复请求返回原结果。

---

## 9. 后端实现约束

### 9.1 推荐目录

```text
backend/src/
├── handlers/
│   ├── devrail_projects.rs
│   ├── devrail_tasks.rs
│   ├── devrail_runs.rs
│   ├── devrail_approvals.rs
│   └── devrail_notifications.rs
├── services/
│   ├── devrail_projects.rs
│   ├── devrail_tasks.rs
│   ├── devrail_runs.rs
│   ├── devrail_harness.rs
│   ├── devrail_approvals.rs
│   ├── devrail_notifications.rs
│   └── devrail_push.rs
├── repositories/
│   ├── devrail_projects.rs
│   ├── devrail_tasks.rs
│   ├── devrail_runs.rs
│   ├── devrail_events.rs
│   ├── devrail_approvals.rs
│   └── devrail_notifications.rs
├── models/devrail.rs
├── permissions/devrail.rs
└── workers/
    ├── devrail_harness_supervisor.rs
    └── devrail_notification_dispatcher.rs
```

### 9.2 Harness Supervisor

`HarnessSupervisor` 是唯一允许启动 Codex 进程的服务。它必须：

- 使用显式的 `cwd`、环境变量白名单和超时；不继承不必要的服务器环境变量。
- 通过 stdin/stdout 处理 JSONL，stderr 进入脱敏结构化日志。
- 将 thread/turn/item 映射到 DevRail run/event；解析失败时保留安全摘要并停止继续执行。
- 对每个项目/环境限制并发数、CPU/内存/磁盘和最大运行时长。
- 将审批请求发送给 `ApprovalService`，不在 worker 内自行批准。
- 收到取消信号时先优雅中断，再按超时强制终止子进程。
- 重启恢复只能从数据库中状态为可恢复的 run 进行，不能依据内存状态猜测。

### 9.3 Notification Dispatcher

`NotificationDispatcher` 负责 outbox、站内通知和渠道投递，不属于 Handler 请求生命周期。它必须：

- 使用 `sqlx` 事务创建 outbox 和站内通知事实；
- 使用租约/锁超时避免多副本重复处理；
- 为每个设备生成独立 delivery 记录；
- 将 Web Push、FCM、APNs 封装为同一异步 provider trait；
- 区分临时失败和永久失败；
- 暴露 backlog、失败和延迟指标；
- 支持优雅停机，已领取但未完成的任务在租约到期后可重新处理。

### 9.4 配置项

建议新增环境变量：

| 变量 | 说明 |
| --- | --- |
| `DEVRAIL_HARNESS_COMMAND` | app-server 可执行文件路径 |
| `DEVRAIL_HARNESS_MAX_CONCURRENCY` | 每个 API 副本最大 Agent 并发数 |
| `DEVRAIL_RUN_MAX_DURATION_SECS` | 单次 run 最大时长 |
| `DEVRAIL_RUN_WORKSPACE_ROOT` | 工作区根目录，必须是受控绝对路径 |
| `DEVRAIL_OUTBOX_BATCH_SIZE` | outbox worker 每批领取数量 |
| `DEVRAIL_OUTBOX_POLL_INTERVAL_MS` | outbox 轮询间隔 |
| `WEB_PUSH_VAPID_PUBLIC_KEY` | 对前端公开的 VAPID public key |
| `WEB_PUSH_VAPID_PRIVATE_KEY` | 仅后端使用的 VAPID private key |
| `WEB_PUSH_SUBJECT` | VAPID subject（mailto 或 HTTPS URL） |
| `WEB_PUSH_MAX_RETRIES` | 推送最大重试次数 |
| `DEVRAIL_NOTIFICATION_RETENTION_DAYS` | 通知保留天数 |
| `DEVRAIL_EVENT_RETENTION_DAYS` | Agent 事件保留天数 |

生产环境禁止通过前端 `config.js` 注入任何私钥；公开 VAPID key 只能通过受保护 API 返回。

---

## 10. 前端实现约束

### 10.1 推荐目录

```text
frontend/src/app/features/devrail/
├── data-access/
│   ├── devrail-project-api.service.ts
│   ├── devrail-task-api.service.ts
│   ├── devrail-run-api.service.ts
│   ├── devrail-notification-api.service.ts
│   └── devrail-event-stream.service.ts
├── models/devrail.models.ts
├── pages/
│   ├── project-list/
│   ├── project-detail/
│   ├── task-list/
│   ├── task-detail/
│   ├── run-detail/
│   ├── approval-inbox/
│   ├── notification-center/
│   └── notification-settings/
├── push/
│   ├── push-registration.service.ts
│   └── push-permission.service.ts
├── devrail.routes.ts
└── devrail.permissions.ts
```

### 10.2 Angular 22 约束

- 使用 standalone components、signals、OnPush/zoneless 兼容模式和 `inject()`。
- 异步资源使用 signal/resource 或现有项目约定；不要在页面中复制订阅和请求状态。
- 路由和导航引用同一个权限常量；页面采用懒加载。
- SSE 事件进入集中式 signal store；页面只订阅当前 run 的派生状态。
- 浏览器推送的 Service Worker 只处理通知显示和点击深链接，不执行代码或命令。
- 推送权限失败、浏览器不支持、iOS 未安装 PWA 和设备被撤销必须有明确可恢复提示。
- 所有可见文案使用简体中文；按钮、图标、菜单和推送设置提供可访问名称。
- 手机视口下：任务状态、审批操作、关键错误和“打开任务”入口必须在首屏可见；事件详情允许折叠。
- 遵循 arc-admin 的页面结构、Material M3 token、表格、加载/空/失败状态和无障碍规范。

### 10.3 主要页面

| 路由 | 页面 | 关键内容 |
| --- | --- | --- |
| `/devrail/projects` | 项目列表 | 搜索、筛选、状态、负责人、最近活动 |
| `/devrail/projects/:id` | 项目详情 | 仓库、环境、成员、策略、最近任务 |
| `/devrail/tasks` | 任务列表 | 状态、负责人、优先级、项目和最近 run |
| `/devrail/tasks/:id` | 任务详情 | 目标、验收标准、run 历史、变更和评论 |
| `/devrail/runs/:id` | 运行详情 | 实时事件、工具调用、审批、日志、变更、门禁 |
| `/devrail/approvals` | 审批中心 | 待处理请求、风险级别、移动端批准/拒绝 |
| `/devrail/notifications` | 通知中心 | 未读、全部、按项目/事件过滤 |
| `/devrail/settings/notifications` | 通知设置 | 设备、渠道、事件偏好、静默时间 |

---

## 11. 手机推送实现验收

### 11.1 功能验收

- 用户未明确授权时，系统不发送浏览器权限请求。
- 用户授权后，设备出现在“通知设置”，重复注册不会产生重复设备。
- 从任务完成、失败和审批请求到站内通知，均能在数据库中查询到完整链路。
- Android Chrome PWA 可收到推送，点击后打开对应任务或审批详情。
- 支持 Web Push 的 iOS 已安装 PWA 可收到推送；不满足条件时显示降级说明。
- 撤销设备后不会再收到测试推送；供应商永久失败会自动失效设备。
- 同一事件重复消费不能产生重复推送。
- 推送失败时站内通知仍存在，用户能看到失败状态和重试/重新授权建议。

### 11.2 安全验收

- Push payload 不包含代码、密钥、Cookie、完整命令参数、原始日志或敏感个人信息。
- 深链接打开后重新进行身份和权限检查，不能因拥有通知 ID 绕过 RBAC。
- VAPID private key 仅存在服务端密钥配置，不进入 Git、前端包、日志和错误响应。
- 设备 endpoint 不出现在普通业务日志；排障使用不可逆指纹和 trace id。
- 推送设置变更、设备注册/撤销、测试推送和供应商配置变更都有审计。

### 11.3 可靠性验收

- 模拟供应商 500/超时时，delivery 进入 `retrying` 并按退避重试。
- 模拟 404/410 时，delivery 进入 `failed`，设备进入 `invalid`，不再继续重试。
- worker 重启或网络断开后，租约到期的 outbox/delivery 可以重新领取。
- 两个 worker 并行处理同一 outbox 时，最终只生成一条逻辑通知和一组幂等 delivery。
- backlog 超过阈值会产生运维告警。

---

## 12. 测试要求

### 12.1 后端

- Repository：组织、部门、所有者数据范围、分页、幂等和状态转换。
- Service：任务快照、run 生命周期、审批决策、重试、中断和策略校验。
- Harness：JSONL 解析、事件游标、进程退出、超时、取消和脱敏。
- Push：Web Push provider mock、临时/永久失败、重试退避、设备失效和幂等。
- 数据库集成：真实隔离 PostgreSQL、migration、outbox 并发领取和事务回滚。
- API 契约：OpenAPI 路径/DTO 与实际路由一致，403/404/409/422/429/500 行为明确。

### 12.2 前端

- Vitest：signal store、SSE 重连、通知偏好、设备注册、权限显隐和错误状态。
- Playwright Desktop：登录、创建项目、创建任务、启动 run、审批、查看变更、通知中心。
- Playwright Mobile：移动导航、审批操作、推送设置、深链接、无横向页面溢出。
- 浏览器 API mock：Service Worker、Notification、PushManager 不可用和权限被拒绝场景。

### 12.3 交付前命令

从仓库根执行：

```bash
cargo flow scope
cargo flow verify --all
```

前端至少执行：

```bash
cd frontend
npm run lint
npm run format:check
npm run test:ci
npm run build
npm run e2e
```

---

## 13. 非功能需求

### 13.1 安全

- 默认 deny；任何工作区外路径、网络访问、生产环境和受保护 Git 写入都必须显式允许。
- 不允许 Agent 读取 `backend/.env`、浏览器会话、VAPID private key、数据库连接串和宿主机凭据。
- 所有命令、工具参数和日志输出执行敏感字段脱敏。
- 使用组织/部门/所有者数据范围过滤所有查询，禁止先全量读取再由前端过滤。
- 后端 API、worker 和 app-server 之间使用最小权限和明确的本地边界。
- 供应商密钥从部署密钥系统注入；文档、示例和测试只使用假值。

### 13.2 性能与容量

- 列表 API 默认分页，禁止无界返回任务、事件或通知。
- Agent 事件大字段分离存储或截断，单条事件设置大小上限。
- SSE 每个客户端有心跳、缓冲上限和断开清理；慢客户端不得阻塞 run worker。
- 推送 worker 和 Agent supervisor 都支持有界并发和背压。
- 初始容量目标：单实例 10 个并发 run、100 个 SSE 客户端、10,000 个设备；超过后通过配置水平扩展。

### 13.3 可用性与恢复

- API 无状态；活动 run、outbox 和审批状态持久化在 PostgreSQL。
- worker 重启可恢复未完成 outbox 和可恢复 run；不可恢复状态必须显式失败。
- PostgreSQL 备份、PITR、迁移和恢复演练沿用 arc-admin 生产指南。
- 关键故障通过 Prometheus/Grafana 或现有观测系统告警。

### 13.4 可访问性和兼容性

- 支持键盘导航、可见焦点、屏幕阅读器语义和 reduced motion。
- 桌面目标为 Chromium；移动目标为 Android Chrome 和支持 Web Push 的 iOS PWA。
- 断网或服务端不可用时，页面保留上下文并显示可重试状态，不显示空白页。

---

## 14. 迭代计划

### Phase 0：基线与骨架

- 从 arc-admin 模板初始化 DevRail 工程。
- 建立 `devrail` 权限、前端路由、OpenAPI 生成和基础 migration。
- 完成项目、仓库、环境和任务 CRUD。

### Phase 1：Agent 运行闭环

- 接入 app-server supervisor。
- 完成 run、事件时间线、SSE、取消、恢复和基础审批。
- 增加变更集和质量门禁。

### Phase 2：可靠通知

- 完成 outbox、站内通知、Web Push 设备注册、偏好、投递、重试和审计。
- 完成 Android PWA 和 iOS 已安装 PWA 验收。
- 建立推送失败、backlog 和设备失效告警。

### Phase 3：协作与发布

- 评论、提及、审查任务、临时分支和补丁导出。
- 可选接入 GitHub/GitLab 合并请求。
- 评估原生移动客户端和 FCM/APNs provider。

---

## 15. 风险与决策

| 风险 | 影响 | 应对 |
| --- | --- | --- |
| Codex app-server 协议变化 | Agent 接入回归 | 固定 harness 版本，生成并校验协议 schema，保留 SDK/CLI 兼容层 |
| Agent 运行命令具有破坏性 | 数据或凭据泄露 | 工作区隔离、默认拒绝、审批、超时、审计和资源限制 |
| iOS Web Push 前置条件复杂 | 用户收不到手机通知 | 首次授权引导、能力检测、站内通知降级、后续原生适配 |
| 推送供应商不稳定 | 重要通知延迟 | transactional outbox、重试、永久失败识别和运维告警 |
| 长事件日志造成数据库膨胀 | 成本和查询变慢 | 大字段限制、分离日志、保留策略和归档 |
| 多副本 worker 重复处理 | 重复执行或重复推送 | 租约、`SKIP LOCKED`、幂等键和 delivery 唯一约束 |
| 复制基线时绕过 arc-admin 公约 | 安全/审计缺失 | 先完成范围审计、模板渲染、OpenAPI 契约和全量验证 |

### 15.1 首次实现前必须确认的决策

1. MVP 是否只接受 Web Push，还是同步实现原生 FCM/APNs 客户端。
2. Agent 工作区运行在同一主机受控目录、Docker 容器，还是外部执行节点。
3. Git 凭据使用部署 Secret Manager、SSH agent，还是短期 OAuth token。
4. Agent 事件原始日志的存储介质和组织级保留期限。
5. 质量门禁失败后任务是直接 `failed`，还是进入人工审查状态。
6. 是否允许组织管理员配置静默时段例外，以及 `critical` 通知能否突破静默时段。

---

## 16. MVP 完成定义（Definition of Done）

以下复选框是最终验收条件，不表示当前已经完成。当前 MVP 状态为“未完成”；只有所有条件均经代码、自动化测试和运行验收证明后，才能更新为完成。

以下条件全部满足，DevRail MVP 才能标记为完成：

- [ ] 新建项目、仓库、环境、任务和成员的全流程可用。
- [ ] Agent run 能启动、流式展示、请求审批、中断、恢复、失败和重试。
- [ ] 所有受保护接口都有后端权限和数据范围校验。
- [ ] 变更集、质量门禁和审计记录能从任务详情追溯到具体 run。
- [ ] 站内通知可靠写入；推送失败不会丢失通知。
- [ ] Android PWA 和符合条件的 iOS PWA 可收到完成、失败和审批请求推送。
- [ ] 推送设备注册、撤销、永久失败失效、临时失败重试和幂等均有自动化测试。
- [ ] Push payload 未包含代码、命令、凭据和完整日志；深链接经过重新授权。
- [ ] 桌面与移动端 E2E、Rust 集成测试、OpenAPI 生成和 `cargo flow verify --all` 通过。
- [ ] 生产配置、密钥、备份、告警、保留策略和恢复演练文档齐全。
