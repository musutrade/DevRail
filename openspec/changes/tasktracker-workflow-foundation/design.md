## Context

现有调度循环位于后端 worker，直接组合 DevRail Repository 的任务 claim、run 创建、reconciliation 和审计操作；P0 已经确立 `task_id + attempt`、数据库租约与 System Actor 语义。任务表尚无完整派发输入快照，run 也没有可复现的 workflow 内容。详见 `proposal.md`、`specs/task-tracker/spec.md` 和 `specs/workflow-loader/spec.md`。

实现必须继续满足 SQL 写入只位于 Repository/migration/test、所有查询在 SQL 层强制数据范围、Harness 只由后端启动、默认网络关闭及事件/诊断脱敏等约束。

## Goals / Non-Goals

**Goals:**

- 用稳定的领域接口隔离 Orchestrator 与 DevRail PostgreSQL 细节，同时保留现有事务和 claim 行为。
- 在任务进入队列时生成完整、不可变、可复现的派发快照，并让 run 明确关联该快照。
- 以严格 schema、封闭模板上下文和平台安全上限加载 workflow。
- 让动态加载故障可诊断、可恢复且不改变既有执行。
- 为未来外部 tracker、DAG、workspace 和 continuation 提供清晰扩展点，但不提前实现它们。

**Non-Goals:**

- 不实现 GitHub/GitLab issue tracker adapter 或跨系统状态双写。
- 不增加任务依赖表、workspace/worktree 管理、continuation turns 或自动修复 run。
- 不允许 workflow 定义任意脚本型 loader、插件或平台级权限。
- 不在本批增加前端 workflow 编辑器；workflow 由受版本控制的仓库文件管理。

## Decisions

### 1. TaskTracker 是领域端口，PostgreSQL 实现复用 Repository

在独立的 orchestration/domain 模块定义异步、可注入的 TaskTracker 端口，以强类型输入输出表达候选读取、状态查询、claim、历史追加和调度元数据更新。DevRail PostgreSQL adapter 组合现有 Repository；所有 SQL 和事务仍留在 Repository 层。worker 持有共享端口，通过测试内存实现或 mock 验证调度决策。

选择该结构是为了避免把 SQLX 类型泄漏进调度器，也不复制已经验证的 claim 事务。替代方案是把现有 Repository trait 直接当 tracker；这会把持久化模型和大量非调度操作暴露给控制循环，因此拒绝。

### 2. 数据范围属于 tracker 调用上下文和 SQL 条件

普通 API 发起的查询继续携带组织、部门和所有者范围；调度 worker 使用明确的 System Actor 与目标组织范围逐批处理，不使用全局无范围读取后在内存过滤。tracker 的领域错误区分 not-found/scope-denied、版本冲突、非法转换、暂时存储错误和永久数据错误，但对外响应不泄露跨组织存在性。

替代方案是 scheduler 一次性读取所有组织任务再过滤；它会扩大泄漏半径并违反 SQL 层数据范围约束，因此拒绝。

### 3. queued 转换是派发快照的原子边界

新增版本化 task dispatch snapshot，至少包含任务修订号、标题、目标、仓库、环境、验收标准以及规范化 workflow。任务进入 `queued` 与快照写入在同一事务完成。已 queued 任务的派发输入不提供原地更新路径；修改通过新任务修订或取消后重建表达。run 保存 task snapshot 的稳定标识，并复制 workflow 来源、版本和 SHA-256 摘要，便于独立查询和保留。

选择 queued 而非 run 创建作为主快照边界，是因为排队内容也必须稳定，且动态 reload 不能让等待时间改变任务语义。仅在 run 创建时读文件的方案无法满足这一点，因此拒绝。

迁移为历史 queued/active 任务生成明确的 legacy/default 快照；迁移不能猜测已不存在的仓库内容。既有终态任务可按查询需要惰性展示 legacy 标识，不重新执行。

### 4. WORKFLOW.md 使用封闭 schema 与规范化摘要

文件只允许位于经 canonicalize 验证的仓库根目录，拒绝越界符号链接，并设置字节上限。解析器先分离 YAML front matter 与 Markdown 正文，再用拒绝未知字段的强类型结构校验。配置包含 schema/version、执行模式、允许工具、质量门禁、超时、重试、生命周期 hooks 和通知策略；每个枚举与数值范围显式验证。

模板引擎使用 strict undefined 模式和封闭的变量/过滤器注册表。加载候选版本时分析全部变量和过滤器，确保都在允许集合；任务排队时用强类型上下文渲染并再次严格校验。模板不具备文件、环境变量、网络或任意函数访问能力。

规范化摘要对解析后的有序配置和原始提示正文计算 SHA-256；数据库同时保存原始声明版本和规范化快照。摘要用于关联与去重，不替代 task/run 主键。

### 5. 平台策略与仓库策略采用安全交集

workflow loader 不直接产生最终 Harness 权限，而是把仓库请求与环境/平台策略求安全交集：布尔能力默认取更严格值，资源上限取较小值，工具必须同时位于平台和仓库允许集合。无法安全合并或显式请求硬边界之外能力时拒绝整个候选版本。最终生效值进入规范化快照，审计只保存脱敏摘要。

替代方案是允许仓库覆盖平台默认值再依赖审批补救；该方案会在审批前扩大攻击面，因此拒绝。

### 6. 动态 reload 采用内容摘要轮询和原子发布

worker 在有界、可配置且带抖动的间隔内读取元数据与内容摘要；只在摘要变化时完整解析。这样复用现有 Tokio worker 生命周期，无需依赖操作系统文件通知的丢事件语义。候选版本通过完整校验后，使用按仓库隔离的并发安全缓存原子替换当前有效版本；删除文件等价于加载版本化安全默认值。

每个环境最近接受的有效 workflow 身份和规范化快照持久化在带组织、部门、所有者范围的版本记录中，内存缓存只是加速层。无效候选不覆盖 last-known-good。失败证据按仓库、候选摘要和错误类别去重，产生低基数指标、System Actor 审计和脱敏告警；相同坏版本不在每轮制造事件风暴，但仍按退避重试。进程重启会从磁盘重新校验候选版本；候选无效时从持久化记录恢复 last-known-good，数据库中的任务/run 快照保证既有执行不依赖内存缓存。

选择轮询而不是仅依赖文件 watcher，是为了跨 Linux、容器挂载和原子 rename 保持一致恢复语义。未来可用 watcher 作为降低延迟的提示，但轮询仍是对账来源。

### 7. 失败边界和发布顺序

workflow 无效属于任务进入队列前的可修复配置错误，不创建半成品快照或 run。tracker 暂时失败由现有 scheduler 退避；版本冲突触发重新读取，不盲目覆盖。任务快照存在但 run workflow 身份不一致时，reconciliation 阻止启动并记录数据完整性失败。

## Risks / Trade-offs

- [task snapshot 增加 JSON 存储和迁移成本] → 设置字段/正文大小上限、使用规范化结构，并为历史记录采用明确 legacy 版本。
- [trait 抽象可能只是 Repository 的薄包装] → 只暴露调度所需领域操作，并用 mock 验证 worker 不再依赖 SQLX/表结构。
- [轮询导致短暂 reload 延迟] → 使用较短有界间隔和内容摘要跳过无变化解析；需求只保证最终用于新快照，不承诺瞬时生效。
- [半写入文件产生短暂错误] → 推荐原子 rename，loader 保留 last-known-good 并防抖重读。
- [模板能力演进造成兼容问题] → schema 和 workflow 声明版本显式化，新增变量/过滤器需测试与文档，未知能力继续 fail closed。
- [诊断可能泄露提示或 secret] → 只记录位置、类别、摘要和脱敏消息，不记录完整正文或模板上下文。

## Migration Plan

1. 新增可空/带兼容默认值的 task dispatch snapshot、任务修订号和 run workflow 身份字段，回填历史 queued/active 数据为版本化 legacy/default 快照。
2. 部署 Repository 与 PostgreSQL TaskTracker adapter；先通过兼容路径验证读取和 claim 结果与现状一致。
3. 部署 workflow parser、严格校验、默认 workflow 和缓存，但保持调度器使用显式兼容配置。
4. 将 queued 转换切换为原子快照写入，再让新 run 强制使用 task snapshot；reconciliation 检查身份一致性。
5. 启用 reload 轮询、指标和告警，更新运维文档与实现状态。
6. 确认没有未快照的新活动任务后收紧非空约束和旧写入路径。

回滚时停止 scheduler/reload worker，回退到仍能读取新增字段的上一兼容版本；不删除迁移字段或快照。已创建的 run 继续按持久化快照完成或由 reconciliation 明确终止，禁止重新读取当前文件替代其配置。
