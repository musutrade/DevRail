# DevRail Symphony 编排与 Harness Engineering 专项需求

## 0. 文档信息

| 项目 | 内容 |
| --- | --- |
| 文档版本 | 1.0.0 |
| 文档状态 | 专项需求基线，待分阶段实现与验收 |
| 编写日期 | 2026-08-26 |
| 适用产品 | DevRail（基于 Codex harness 的开发工作台） |
| 上位需求 | [DevRail 产品与技术需求](requirements.md) |
| 当前状态 | [DevRail 实现状态](devrail-implementation-status.md) |
| 审计输入 | [Symphony 审计报告](symphony-audit-report.md) |

本文件把 Symphony 的调度、工作流和恢复思想，以及 Harness Engineering 的可复现、可观测、自动验证思想，转化为 DevRail 可实现、可测试、可审计的产品需求。本文件是专项补充，不替代 `docs/requirements.md`；两者冲突时，较新的经评审版本和项目公约优先。

---

## 1. 背景与目标

### 1.1 背景

DevRail 已经具备项目、仓库、环境、任务、Harness Supervisor、run/thread/turn/event、审批、通知、质量门禁和外部代码审查等基础能力。当前的 `queued` 任务调度已经能按优先级轮询，并使用 PostgreSQL 租约避免多实例重复领取，但仍缺少完整的任务跟踪器抽象、工作流配置、状态 reconciliation、依赖编排和失败恢复闭环。

审计报告中的部分结论来自较早的代码快照：调度器、外部评论同步和 GitHub/GitLab resolved 状态基础能力已经存在。因此，本文件以当前实现状态为事实基线，只把仍有代码、测试或运行证据缺口的内容列为待实现需求。

### 1.2 目标

完成后，DevRail 应成为一个由任务驱动、始终在线、可恢复且有安全边界的 Agent 控制平面：

1. 任务状态是唯一调度事实来源，Agent 是执行者而不是浏览器插件。
2. 调度器可在多 worker、进程重启、网络断流和部分失败情况下保持幂等和最终一致。
3. 每个任务都在确定性的隔离工作区中运行，并可重现、审查和清理。
4. 工作流、质量门禁、审批和通知可以配置，但安全红线不可被配置绕过。
5. 每项能力都有代码、自动化测试、运行验收和文档证据，不能仅以 `cargo flow verify --all` 通过代替产品验收。

### 1.3 成功标准

- 同一任务不会因多实例或重启而产生重复活动 run。
- 调度器重启后能恢复、重试或明确终止所有未完成任务，不留下无限期僵尸状态。
- 任务状态改变后，已不满足条件的 run 能被取消或停止继续派发。
- 任意一次运行都可追溯到任务快照、工作流版本、工作区、审批、变更集、质量门禁和通知。
- 不向 Agent 事件、日志、通知或推送载荷泄露凭据、源代码和完整命令上下文。

---

## 2. 权威来源与术语

### 2.1 权威来源

实现和评审至少参考以下资料：

- 本地官方页面快照：[Codex 编排的开源规范：Symphony](Codex%20编排的开源规范：Symphony%20_%20OpenAI.mhtml)。
- 本地官方页面快照：[Harness 工程：在智慧體優先的世界中善用 Codex](Harness%20工程：在智慧體優先的世界中善用%20Codex%20_%20OpenAI.mhtml)。
- OpenAI 文章：[Harness 工程：在智慧體優先的世界中善用 Codex](https://openai.com/zh-Hant/index/harness-engineering/)。
- OpenAI 文章：[Codex 编排的开源规范：Symphony](https://openai.com/zh-Hans-CN/index/open-source-codex-orchestration-symphony/)。
- Symphony 参考实现：[README](https://github.com/openai/symphony/blob/main/README.md) 和 [SPEC.md](https://github.com/openai/symphony/blob/main/SPEC.md)。
- DevRail 项目公约、架构和 [审计与门禁](devrail-audit-and-gates.md)。

若外部页面不可访问，以随仓库评审的 MHTML 和 `SPEC.md` 版本为依据，并在版本变更时记录差异。

### 2.2 术语

| 术语 | 定义 |
| --- | --- |
| TaskTracker | 向调度器提供任务列表、状态和更新能力的抽象；第一实现使用 DevRail PostgreSQL。 |
| Workflow | 任务执行模板，包含提示词、环境、门禁、超时、重试和 hooks。 |
| Orchestrator | 负责 reconciliation、领取任务、创建 run、启动 Agent 和处理终态的控制循环。 |
| Workspace | 任务专属的受控目录或 Git worktree。 |
| Attempt | 某任务的一次执行尝试；首次运行、重试和 continuation 都有明确编号。 |
| Continuation turn | 在同一 run/thread 中，根据后续事件继续执行的回合。 |
| Reconciliation | 将 tracker、run、workspace 和 worker 实际状态对账并修正不一致。 |
| Harness | 受控的 Codex app-server 进程及其工作区、工具、审批、日志和资源限制。 |
| Harness Engineering | 通过环境、工具、观测和自动验证，使 Agent 输出可复现、可诊断、可修复。 |

---

## 3. 产品定位与边界

### 3.1 产品定位

DevRail 是现有控制平面中的 Symphony 编排能力，不部署一套独立 Symphony 服务。现有 Axum API、PostgreSQL、Angular、Harness Supervisor、outbox 和 `arc-flow` 继续作为产品基础设施。

### 3.2 执行模式

任务创建时必须声明执行模式；模式影响默认提示、权限和门禁，但不能扩大用户权限：

| 模式 | 用途 | 默认行为 |
| --- | --- | --- |
| `interactive` | 用户实时协作 | 用户启动，Agent 可在审批点暂停，允许 continuation。 |
| `autonomous` | 队列自动处理 | 调度器自动领取，使用项目预设策略和最大重试次数。 |
| `research` | 调研、分析和只读验证 | 默认禁止写入仓库和远端，输出报告和引用。 |
| `review` | 代码审查和质量门禁 | 以变更集和测试结果为输入，不修改受保护分支。 |

### 3.3 不可突破的边界

- Codex harness 只能由后端 Harness Supervisor 启动，浏览器只能使用 DevRail API 和 SSE。
- 工作区必须位于受控根目录；网络默认关闭；危险命令、生产环境命令和远端写入必须审批。
- 事件、日志、差异、通知和推送载荷必须脱敏。
- 通知先写 PostgreSQL transactional outbox，供应商调用只能由 dispatcher worker 执行。
- 所有 Repository 查询必须在 SQL 中完成组织、部门和所有者数据范围过滤。

### 3.4 需求到交付的标准闭环

每个需要 Agent 执行的工程变更都必须沿着同一条可追溯链路推进：

```text
需求文档 / ADR
        ↓
DevRail Task
        ↓
不可变任务快照
        ↓
Harness Run（thread / turn / event）
        ↓
审批、断流恢复与重试
        ↓
ChangeSet
        ↓
arc-flow 质量门禁
        ↓
需求、实现状态、测试与运行证据同步
        ↓
PR / MR
        ↓
CI、供应链、CodeQL 与外部审查
        ↺（失败则创建 continuation 或修复 run，回到质量门禁）
```

闭环必须满足以下规则：

- Task 必须引用需求或 ADR，保存验收标准；进入 `queued` 后使用不可变快照。
- Run、审批、恢复、ChangeSet、质量门禁和 PR/MR 必须通过稳定 ID 互相关联，任何一步都能反向追溯到 Task。
- 质量门禁失败、CI 失败或外部审查要求修改时，系统应保留原始失败证据，并按策略创建 continuation/修复 run；不得覆盖原 run 结论。
- 文档和实现状态只能在代码、测试和运行证据更新后同步为“已完成”，不能先改状态再补证据。
- PR 创建前必须通过交付前门禁；PR 创建后继续监控 CI、供应链、CodeQL 和审查状态，结果回写 Task/Run。
- 合并不是 DevRail 自动宣称完成的条件；只有目标分支合并、生产策略允许且最终验收证据齐全时，Task 才能归档。

**SY-LIFE-001**：系统必须为每个 Task 维护从需求/ADR 到 PR/MR 的关联链，并提供 API 和详情页查询。

**SY-LIFE-002**：质量门禁、CI 或外部审查失败必须触发可配置的 continuation/修复流程，并保留失败与修复的父子关系。

**SY-LIFE-003**：需求、实现状态和测试证据的同步必须是交付检查项；缺少证据时不得将 Task 标记为完成。

---

## 4. 当前基线与差距

### 4.1 已有能力（不重复开发）

- `queued` 任务按优先级、截止时间和创建顺序轮询；PostgreSQL `SKIP LOCKED`、claim 租约、过期恢复和启用环境过滤已实现。
- Harness Supervisor 已实现受控 app-server 启动、并发与时限、事件脱敏、SSE 游标、优雅中断、活动 run 重启恢复和传输断流有限次恢复。
- 审批持久化、批准/拒绝/撤回/过期、策略版本校验、终态重试和指定 turn 恢复已实现。
- transactional outbox、站内通知、Web Push 设备管理、dispatcher、投递重试、永久失效和告警已实现基础闭环。
- 任务评论、代码审查、补丁导出、GitHub/GitLab PR/MR 状态和外部评论归一化基础能力已实现，resolved 状态不再固定为 `false`。
- `arc-flow` scope、审计、lint、编译、测试、构建和供应链检查已接入 CI。

### 4.2 本专项尚待补齐

以下项目是需求，不代表现状已完成：

- 稳定的 `task_id + attempt` 幂等语义和 Scheduler/System Actor。
- `TaskTracker` 抽象、仓库级 `WORKFLOW.md`、严格模板渲染和动态 reload。
- 任务依赖/DAG、任务完成后创建后续任务、完整 reconciliation 和终态清理。
- 退避、stall 检测、任务取消传播、并发恢复和运行验收证据。
- Workspace 生命周期 hooks、跨运行可复现环境和终端清理保证。
- 结构化日志/指标/Trace、测试产物、Playwright 截图/视频和失败诊断上下文。
- 质量门禁失败后的受控修复 run，以及通知和外部供应商回调的完整端到端演练。

审计报告中“调度器不存在”“resolved 状态硬编码”“16 项验收条件”等表述不得直接作为当前状态引用；验收数量以 [DevRail 总需求第 16 节](requirements.md#16-mvp-完成定义definition-of-done) 当前版本为准。

---

## 5. 功能需求

### 5.1 TaskTracker 与任务生命周期（P1）

**SY-TRACK-001**：系统必须通过 `TaskTracker` 接口读取可调度任务、获取单任务状态、追加状态历史并更新调度元数据；Orchestrator 不得直接依赖具体 SQL 表。

**SY-TRACK-002**：第一实现必须提供 DevRail PostgreSQL tracker，保留组织、部门、所有者和权限过滤；GitHub Issues 等外部 tracker 作为后续 adapter，不得绕过 DevRail 权限。

**SY-TRACK-003**：任务进入 `queued` 后，标题、目标、仓库、环境、工作流版本和验收标准形成不可变快照；修改必须创建新版本或显式取消后重建。

**SY-TRACK-004**：任务状态转换必须由后端事务完成并写入历史，允许的转换、操作者、原因和时间必须可查询；非法转换返回明确错误。

**SY-TRACK-005**：只有满足以下条件的任务才能派发：状态为 `queued`、依赖全部成功、环境启用且健康、无活动 run、未超过截止时间或策略允许继续。

### 5.2 Workflow Loader 与 `WORKFLOW.md`（P1）

**SY-WORKFLOW-001**：仓库根目录可提供 `WORKFLOW.md`，文件由 YAML front matter 和 Markdown/提示正文组成。系统必须支持版本、执行模式、工具策略、质量门禁、超时、重试、hooks 和通知策略字段。

**SY-WORKFLOW-002**：配置加载必须使用严格模板：未知变量、未知过滤器、缺失必填字段和非法枚举均报错，不得静默替换为空值。

**SY-WORKFLOW-003**：每个 run 持久化实际使用的 workflow 内容摘要、版本和来源；运行中不得因文件变化而改变已解析配置。

**SY-WORKFLOW-004**：worker 支持动态 reload。新配置合法时只影响新 run；非法配置保留上一次有效版本并产生告警和审计事件。

**SY-WORKFLOW-005**：工作流中不得声明绕过组织权限、审批、脱敏、受控根目录和网络策略的选项。

### 5.3 Orchestrator 调度循环（P0）

**SY-ORCH-001**：Orchestrator 必须在有界循环中按“reconciliation → dispatch → reap/metrics”顺序运行，并支持优雅停止。

**SY-ORCH-002**：领取任务使用数据库原子 claim、`SKIP LOCKED` 和可过期租约；claim 过期后任务可被安全重新领取。

**SY-ORCH-003**：幂等键必须稳定地使用 `task_id + attempt`（或等价的持久化确定性键），禁止以每次循环生成的随机 claim UUID 作为业务幂等依据。

**SY-ORCH-004**：调度系统必须使用明确的 Scheduler/System Actor，不能以 `session_id = 0` 模拟普通用户；审计中应记录触发来源和策略版本。

**SY-ORCH-005**：同一任务最多一个活动 run。容量不足、环境不健康或审批未完成时任务保留在队列，不得伪造成功或永久丢弃。

**SY-ORCH-006**：调度排序至少支持优先级、截止时间和创建时间；排序规则和饥饿防护必须可配置并有测试证据。

**SY-ORCH-007**：任务状态、依赖、环境或策略发生变化后，Orchestrator 必须在下一轮 reconciliation 中终止不再符合条件的未启动 run，或将任务转为明确的等待/失败状态。

### 5.4 重试、stall 与恢复（P0）

**SY-RETRY-001**：可重试错误必须使用指数退避并带抖动、最大尝试次数和最大延迟；不可重试错误直接进入 `failed` 并保存脱敏根因。

**SY-RETRY-002**：run 必须记录 `attempt`、开始/结束时间、最后心跳、重试原因、父 run/turn 和恢复建议；首次运行、重试和 continuation 的语义不可混淆。

**SY-RETRY-003**：worker 必须检测无事件、无心跳、进程退出和租约失效等 stall，按策略中断、恢复或重新排队，并保证子进程清理。

**SY-RETRY-004**：网关或 app-server 传输断流应先按上限执行幂等恢复/重连，达到上限后才进入明确失败；浏览器 SSE 断开只补拉事件，不重复执行 Agent，且不能导致任务事实丢失。

**SY-RETRY-005**：进程重启后，系统必须扫描活动 run，能够恢复的继续运行，不能恢复的标记为 `failed` 或 `interrupted` 并发出通知；不得留下无限期 `active`。

### 5.5 任务依赖与后续任务（P1）

**SY-DAG-001**：任务可声明前置任务，依赖关系必须在同一组织范围内，禁止形成环；创建和修改时进行事务内环检测。

**SY-DAG-002**：依赖未成功时任务保持 `blocked`/`queued` 等明确状态，不得派发；依赖失败、取消或超时时按工作流策略决定阻塞、跳过或失败。

**SY-DAG-003**：Agent 可通过受控 API 提议后续任务；后续任务必须经过 schema 校验、权限/范围校验和幂等去重，不能直接写数据库或提升权限。

**SY-DAG-004**：依赖变化、后续任务创建和自动跳过均写入审计和事件，并在任务详情展示可追溯关系。

### 5.6 Workspace Manager（P1）

**SY-SPACE-001**：每个任务拥有确定性的 workspace 标识和路径，路径必须位于受控根目录；不同任务之间默认使用独立 worktree 或等价隔离目录。

**SY-SPACE-002**：创建 workspace 前校验仓库、分支、凭据引用和环境策略；凭据只在运行时注入，不写入磁盘、事件或推送。

**SY-SPACE-003**：支持 `before_run`、`after_run`、`on_failure` 和 `cleanup` hooks。hooks 继承同一命令白名单、超时、网络和审批策略。

**SY-SPACE-004**：终态 run 必须执行清理；清理失败要保留可诊断状态并触发告警，不得静默删除审计或变更集。

**SY-SPACE-005**：workspace 元数据必须包含 workflow 版本、基础提交、环境版本和工具版本，使同一任务可重新创建等价环境。

### 5.7 Agent Runner 与 continuation（P1）

**SY-RUNNER-001**：只有 Harness Supervisor 可创建 app-server 进程、thread 和 turn；Orchestrator 通过服务接口请求执行。

**SY-RUNNER-002**：启动参数、环境变量、工具清单、网络策略和工作目录必须从不可变的 run 快照解析，并在审计中保留摘要。

**SY-RUNNER-003**：Agent 请求审批时，run 进入 `awaiting_approval`；批准/拒绝/撤回/过期必须可恢复，审批决定不可修改，只能追加更正事件。

**SY-RUNNER-004**：支持 continuation turn：在同一 thread 中依据测试结果、审查意见或用户追加上下文继续执行；每个 continuation 都有父 turn、原因和幂等键。

**SY-RUNNER-005**：终态只能由后端根据 Agent 终态和质量门禁结果写入；客户端事件不能改变 run/task 状态。

### 5.8 Reconciliation 与终态处理（P0）

**SY-RECON-001**：每轮调度开始前对账 tracker 状态、run 状态、claim 租约、Supervisor 子进程和 workspace 状态。

**SY-RECON-002**：发现“数据库 active 但进程不存在”“进程已退出但数据库未更新”“任务已取消但 run 仍运行”等不一致时，按确定性策略修正并写入审计。

**SY-RECON-003**：成功终态必须验证配置的质量门禁；失败终态必须包含用户可读原因、trace/log 引用和重试建议。

**SY-RECON-004**：终态处理必须幂等：重复收到退出、超时或 webhook 事件不会重复通知、重复清理或创建多个后续任务。

---

## 6. Harness Engineering 需求

### 6.1 可复现执行环境（P1）

**HE-ENV-001**：任务运行所需的运行时、依赖、工具版本和环境变量来源必须可声明、可审计、可重建。

**HE-ENV-002**：默认网络关闭；开放域名、端口和外部服务必须由环境策略明确声明并在执行前审批或自动放行（仅低风险白名单）。

**HE-ENV-003**：环境健康检查失败时不得派发新 run；运行中环境失效应进入可诊断失败路径。

### 6.2 观测与产物（P1）

**HE-OBS-001**：每个 run 输出结构化日志、trace id、事件游标、耗时、退出码、资源用量和门禁摘要；敏感字段统一脱敏。

**HE-OBS-002**：大事件和长日志必须分离存储或截断，单条事件、SSE 缓冲和客户端队列均有上限；慢客户端不能阻塞 worker。

**HE-OBS-003**：测试报告、补丁、截图、视频和诊断包作为受控产物保存，访问必须重新进行身份、权限和组织范围校验。

**HE-OBS-004**：产物保留期可按组织和项目配置，过期清理必须可审计；推送载荷只携带产物深链接和脱敏摘要。

### 6.3 浏览器验证与故障录制（P2）

**HE-BROWSER-001**：Playwright 失败时可配置录制截图、视频、trace 和浏览器控制台日志，并关联到 run/quality gate。

**HE-BROWSER-002**：需要浏览器调试时，Chrome DevTools MCP 只能在受控 workspace、工具白名单和审批策略下启用。

**HE-BROWSER-003**：故障录制不得成为 MVP 成功的强制前置条件；录制失败不能覆盖原始测试失败原因。

### 6.4 失败诊断与修复 run（P1）

**HE-FIX-001**：质量门禁失败时，系统自动汇总失败 trace、结构化错误、相关日志、变更集、环境摘要和最近一次命令，生成脱敏诊断上下文。

**HE-FIX-002**：系统可按策略创建关联修复 run；修复 run 必须使用独立 attempt、同一权限边界和新的审计链。

**HE-FIX-003**：低风险格式化或未使用导入修复可配置为自动建议；逻辑修改、依赖升级、远端写入和安全策略变更不得未经审批自动应用。

**HE-FIX-004**：修复 run 必须重新执行受影响的门禁，并将原始失败与修复结果关联；达到最大修复次数后转人工处理。

---

## 7. 安全、权限与数据治理

**SY-SEC-001**：Scheduler/System Actor 必须拥有显式、最小化权限；自动调度不得继承某个已退出用户的会话权限。

**SY-SEC-002**：所有新增表和 Repository 查询遵循组织、部门、所有者边界；依赖、产物、事件、workspace 和 run 均不能跨组织关联。

**SY-SEC-003**：命令、路径、环境变量、标准输出、stderr、差异、trace、通知和推送载荷必须使用统一脱敏器；禁止密码、Cookie、token、私钥、连接串、完整请求头和源代码片段进入推送。

**SY-SEC-004**：生产命令、受保护分支写入、创建提交、远端推送、合并请求合并和密钥访问均为独立高风险动作，必须单独权限和可配置审批。

**SY-SEC-005**：Webhook 必须验签、去重并进行 payload 脱敏；外部评论、线程状态和 PR/MR 状态只在所属仓库/组织范围内同步。

**SY-SEC-006**：深链接打开通知或产物时重新执行会话、权限和数据范围校验；拥有通知 ID 或 URL 不构成授权。

**SY-SEC-007**：禁止在 Service、Handler 或路由层执行 SQL 写操作；禁止以 Clippy `allow` 属性绕过质量门禁。

---

## 8. 数据模型与不变量

实现可复用现有表，但必须满足以下逻辑字段和约束（具体命名以迁移评审为准）：

| 领域 | 必要数据 |
| --- | --- |
| Task | `organization_id`、`project_id`、状态、优先级、截止时间、快照版本、workflow 引用、依赖关系和归档信息。 |
| Run | `task_id`、`attempt`、模式、状态、actor 类型、workflow 摘要、workspace、父 run/turn、心跳、重试原因和终态摘要。 |
| Claim | `task_id`、attempt、worker 标识、租约开始/过期时间、续租时间和释放原因。 |
| Dependency | 前置/后置 task、组织边界、创建者、状态和环检测依据。 |
| Workspace | 任务/运行关联、受控路径、基础提交、环境版本、生命周期状态和清理错误。 |
| Artifact | run/quality gate 关联、类型、对象引用、脱敏摘要、大小、哈希、访问范围和过期时间。 |
| Workflow | 来源、版本、内容摘要、解析状态、错误信息和生效时间。 |

必须建立以下唯一性或一致性约束：

- 一个任务同一时间最多一个活动 run。
- `task_id + attempt` 在 run、通知和修复链中幂等。
- 同一任务不能重复建立相同依赖，依赖图不能成环。
- workspace 路径不能被两个活动任务占用。
- 终态通知、outbox 和 delivery 遵循既有来源事件幂等约束。

---

## 9. 配置与运行参数

新增配置必须有默认值、范围、脱敏规则、启动时校验和运维文档。至少包括：

| 配置类别 | 示例配置项 |
| --- | --- |
| 调度 | 轮询间隔、最大并发 run、claim 租约、续租间隔、饥饿防护。 |
| 重试 | 最大 attempt、基础退避、最大退避、抖动、stall 阈值、修复 run 上限。 |
| workspace | 受控根目录、清理超时、磁盘配额、hooks 开关。 |
| Harness | app-server 版本、单 run 时限、事件大小上限、网络策略。 |
| 产物 | 对象存储引用、单文件大小、保留天数、下载速率限制。 |
| 可观测性 | 指标开关、trace 采样、日志级别和敏感字段规则。 |

配置热加载不得绕过权限、审批和安全红线；非法配置保留上一个有效版本。

---

## 10. API、前端和协作体验

### 10.1 API/OpenAPI

- 新增接口必须由 Rust DTO 和 `utoipa` 生成 OpenAPI，再生成 Angular client；禁止手改生成产物。
- 调度、依赖、工作流、workspace、诊断和产物 API 均要求分页、幂等键、权限码和组织范围。
- API 必须返回可区分的“未找到”“无权限”“状态冲突”“依赖阻塞”“配置无效”和“暂时不可用”。
- SSE 支持心跳、`Last-Event-ID`/cursor 补拉和断开清理；重连不得重复执行任务。

### 10.2 Angular

- 在 `features/<domain>` 增加任务依赖图、工作流状态、run attempt、诊断产物和调度状态视图；共享能力放入 `core`。
- 用户可看到排队原因、依赖阻塞原因、当前 attempt、下一次重试时间、审批等待和清理状态。
- 所有可见文案、Tooltip、错误消息和 ARIA 标签使用简体中文。
- 断线、权限变化、任务被其他 worker 领取和状态冲突都必须有明确的可恢复提示。

### 10.3 PR 与审查

- Agent 生成的变更默认提交到临时分支或导出补丁；受保护分支和合并必须显式审批。
- PR/MR、外部评论、resolved 状态和本地 changeset 关联必须保持幂等同步。
- 自动合并不属于本专项 MVP；即使未来启用，也必须满足白名单、质量门禁、审查批准、无冲突和分支保护条件。

---

## 11. 可观测性、容量与成本

至少提供以下 Prometheus 指标，并按组织/项目维度避免高基数标签：

- `devrail_scheduler_queue_depth`：各优先级队列深度。
- `devrail_scheduler_dispatch_total`：领取、跳过、冲突和失败计数。
- `devrail_scheduler_dispatch_latency_seconds`：入队到启动延迟。
- `devrail_scheduler_retry_total`、`devrail_scheduler_stall_total`：重试和 stall。
- `devrail_run_active`、`devrail_run_duration_seconds`：活动数和耗时分布。
- `devrail_run_reconciliation_total`：对账修正结果。
- `devrail_run_tokens_total` 或等价用量指标（供应商提供时记录），以及预算拒绝次数。
- `devrail_artifact_bytes`、`devrail_sse_clients` 和 `devrail_sse_dropped_total`。

初始容量目标沿用总需求：单实例 10 个并发 run、100 个 SSE 客户端、10,000 个设备。worker 必须有界并发和背压；超出容量时任务留在队列并给出可观察原因。模型供应商和具体模型路由不在本专项内，成本控制先通过预算、用量和告警完成。

---

## 12. 测试与验收证据

每个需求 ID 必须建立以下追踪记录：

```text
需求 ID → 代码文件/迁移 → 自动化测试 → 运行验收 → 文档或 CI 证据
```

### 12.1 Rust/数据库测试

- 调度排序、稳定幂等键、并发 `SKIP LOCKED`、claim 过期、续租和多 worker 竞争。
- 依赖环检测、依赖阻塞、后续任务幂等和状态传播。
- worker 重启、活动 run 恢复、app-server 断流、进程退出、超时、取消和子进程清理。
- workflow front matter、严格模板、非法 reload 回退和版本快照。
- workspace 路径隔离、hooks 权限继承、清理失败和产物访问范围。
- 重试退避、stall、最大 attempt、修复 run 关联和终态幂等通知。
- 组织/部门/所有者越权、Scheduler Actor、Webhook 验签去重和敏感字段扫描。

### 12.2 Angular/Vitest/Playwright

- signal/store 显示 queue、依赖、attempt、重试和诊断状态。
- SSE 断线重连、cursor 补拉、慢客户端和状态冲突提示。
- Desktop：创建任务、自动排队、审批、失败重试、修复 run、查看产物和 PR 关联。
- Mobile：推送深链接重新授权、Android PWA、符合条件的 iOS PWA，以及不支持 Web Push 时的站内降级引导。

### 12.3 交付门禁

编码前执行 `cargo flow scope`；按范围执行对应 reviewer 和测试；交付前必须执行 `cargo flow verify --all`。完整门禁通过后仍须完成真实 PostgreSQL、隔离 workspace、Playwright 和供应商回调演练，并将结果附在 PR 或验收记录中。

### 12.4 P0 调度可靠性证据矩阵（2026-08-26）

本矩阵只声明 `symphony-orchestrator-reconciliation` change 覆盖的 DevRail DB tracker 与 Harness 调度能力；外部 tracker、DAG 和 per-task workspace 不因此视为完成。

| 需求 ID | 状态 | 代码/迁移 | 自动化与运行证据 |
| --- | --- | --- | --- |
| SY-ORCH-001 | 已实现 | `backend/src/workers/task_scheduler.rs` 的三阶段循环、`CancellationToken` 优雅停止 | `tick_phase_order_is_reconcile_dispatch_reap`、`cancelled_scheduler_stops_without_polling_the_database` |
| SY-ORCH-002 | 已实现 | `backend/src/repositories/devrail.rs` 原子 claim、续租、过期判定 | `concurrent_claim_lease_expiry_cancel_and_retry_limit_are_deterministic`（真实 PostgreSQL） |
| SY-ORCH-003 | 已实现 | `scheduler:{task_id}:{attempt}`、`uq_devrail_run_task_attempt` | 重复 `create_run` 返回 `None`，同一 attempt 不产生第二个 run |
| SY-ORCH-004 | 已实现 | `ActorType::System`、`audit_logs::record_actor`、reconciliation 审计 | PostgreSQL 断言 `actor_user_id IS NULL`、UUID trace、原因和策略版本 |
| SY-ORCH-005 | 已实现（当前 tracker） | 活动 run 唯一索引、Supervisor reservation、环境 enabled 过滤 | 容量冲突保持 queued；并发 claim/重复 run 集成测试 |
| SY-ORCH-006 | 已实现 | 优先级/截止时间/创建时间排序与 `DEVRAIL_SCHEDULER_PRIORITY_AGING_SECS` | 真实 PostgreSQL 验证等待四个 aging 周期的低优先级任务获得执行权 |
| SY-ORCH-007 | 部分实现 | task 取消和启动阶段环境失效在 reconciliation 传播；run policy 快照防止运行中漂移 | 取消竞态与 interruption 审计测试；DAG 依赖传播属于 SY-DAG P1 |
| SY-RETRY-001 | 已实现 | 可重试分类、OS 随机抖动、指数退避、最大延迟/attempt | 退避边界单元测试、最大 retry PostgreSQL 测试 |
| SY-RETRY-002 | 已实现（continuation 除外） | run attempt、心跳、重试原因、父 run/turn、恢复建议 | `create_run` 字段映射/父子 run PostgreSQL 断言；continuation 属于 SY-RUNNER-004 |
| SY-RETRY-003 | 已实现 | Supervisor 心跳、stall timer、进程清理、重排队 | `stalled_and_disconnected_processes_recover_without_duplicate_runs` |
| SY-RETRY-004 | 已实现 | 传输恢复上限、持久化 thread/turn 后同 attempt `thread/resume`；SSE cursor 补拉独立 | 受控假 app-server 关闭 stdout，断言恢复命令、同 run 和恢复后控制通道 |
| SY-RETRY-005 | 已实现 | 启动扫描、可恢复 run 重启、不可恢复失败/通知/outbox | `mark_unrecoverable_runs` 重复执行只产生一次终态、审计和通知 |
| SY-RECON-001 | 已实现（当前控制面） | 单事务对账 task/run/claim/Supervisor，workspace 以受控 cwd/cleanup 状态核对 | stale run、运行中 run ID 和 claim 修正 PostgreSQL 测试；独立 workspace manager 属于 P1 |
| SY-RECON-002 | 已实现 | stale/cancel/environment/restart 的确定性修正和 System Actor 审计 | cancellation、process missing、restart 三类审计与 UUID trace 断言 |
| SY-RECON-003 | 已实现 | `finish_run` 校验质量门禁并保存 exit reason、trace、恢复建议 | Harness 单元测试、质量门禁既有测试、OpenAPI contract |
| SY-RECON-004 | 已实现 | 条件终态更新、事件/通知 source key、outbox 唯一约束 | 重复 reconciliation/终态只产生一次通知、outbox 与 cleanup 结果 |

可复查命令：

```bash
TEST_DATABASE_URL=postgres://... cargo test --manifest-path backend/Cargo.toml repositories::devrail::scheduler_integration_tests -- --test-threads=1
TEST_DATABASE_URL=postgres://... cargo test --manifest-path backend/Cargo.toml stalled_and_disconnected_processes_recover_without_duplicate_runs -- --test-threads=1
cd frontend && npm test -- --watch=false --runner=vitest
cargo flow verify --all
```

---

## 13. 分阶段实施计划

### P0：调度可靠性与验收基础

1. 稳定 `task_id + attempt` 幂等语义和 Scheduler/System Actor。
2. reconciliation、stall、退避、终态清理和 PostgreSQL 并发/重启集成测试。
3. 补齐总需求第 16 节十项 MVP DoD 的代码—测试—运行证据矩阵。
4. 完成通知投递、410/404 设备失效和供应商回调的端到端演练。

### P1：Symphony 核心兼容

1. TaskTracker 抽象及 DevRail DB tracker。
2. `WORKFLOW.md`、严格模板、动态 reload 和 workflow 快照。
3. DAG 依赖、后续任务、确定性 workspace、hooks 和 continuation turns。
4. 完善指标、trace、成本预算和前端调度/诊断视图。
5. 质量门禁失败生成受控修复 run，并在达到上限后转人工。

### P2：Harness 体验增强

1. Playwright/Chrome DevTools MCP 产物、截图、视频和 trace。
2. 更丰富的故障诊断包、跨运行复现和产物保留策略。
3. 在安全评审后评估低风险自动合并和外部 issue tracker adapter。

每一阶段完成后必须同步 `requirements.md`、本文件和 `devrail-implementation-status.md`，并以可复查证据更新状态；不能以估算百分比替代验收。

---

## 14. 明确不纳入近期范围

- 不部署独立 Symphony 服务，不引入 Redis/Kubernetes 作为 MVP 前置条件。
- 暂不建设 Agent 能力注册中心、心跳池、复杂负载均衡或跨实例弹性集群。
- 暂不绑定 Claude、Haiku、Sonnet、Opus 或其他具体供应商模型路由。
- 默认不自动合并受保护分支，不直接自动应用 Clippy 或未经审批的逻辑修复。
- 不把故障视频、原生 FCM/APNs 客户端和原生移动应用作为 MVP 阻塞条件。
- 不允许 Agent 直接连接生产数据库、绕过 DevRail API 或自行调用外部推送供应商。

---

## 15. 与现有文档的关系

| 文档 | 关系 |
| --- | --- |
| `docs/requirements.md` | 产品总需求、通用权限、数据模型、MVP DoD 和总体路线；本文件补充 Symphony/Harness 专项。 |
| `docs/devrail-implementation-status.md` | 唯一实现状态口径；每次合并后更新已实现、部分实现和未实现。 |
| `docs/symphony-audit-report.md` | 审计输入和历史问题清单；若与代码现状冲突，以代码、测试和实现状态文档复核。 |
| `docs/architecture.md` | 分层、数据范围、配置和契约约束；本文件不得放宽其安全边界。 |
| `docs/devrail-governance.md`、`docs/devrail-audit-and-gates.md` | 工程治理、审计规则和 CI 门禁；所有实现必须遵守。 |

需求、实现和验收状态必须保持一致。若某项需求被取消、拆分或改变优先级，应在本文件记录决策日期、理由、影响范围和替代验收条件。

---

## 16. 专项完成定义

本专项只有在以下条件全部满足后，才可标记为完成：

- [ ] P0 调度可靠性需求均有代码、集成测试和重启/并发运行证据。
- [ ] TaskTracker、workflow、reconciliation、依赖、workspace 和 continuation 的 P1 骨干能力可在 DevRail 内运行。
- [ ] Harness 运行、审批、日志、产物、失败诊断和修复 run 全部遵守权限、脱敏和隔离边界。
- [ ] 总需求第 16 节的十项 MVP DoD 均有可追溯证据。
- [ ] `cargo flow verify --all`、Rust/Angular/Playwright/供应商回调测试和安全审计均通过。
- [ ] 需求、OpenAPI、数据库迁移、前端页面、运维配置和实现状态文档保持一致。
