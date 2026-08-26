## 1. 数据模型与兼容迁移

- [x] 1.1 在修改 Repository/Service/Model 前检查 `codex-audit-pipeline/.codex/templates/` 的适用模板，并用 `cargo flow scope` 记录初始范围；以 scope 输出和模板使用记录验证。
- [x] 1.2 新增 task 修订号、不可变 dispatch snapshot/摘要、run workflow 身份/快照以及带组织、部门、所有者范围的 workflow version 持久化模型；以全新数据库迁移和 schema 断言测试验证字段、约束和索引。
- [x] 1.3 为历史 queued/active task 与 run 回填明确的 legacy/default workflow 身份，保持旧版 INSERT 兼容且不猜测仓库内容；以升级迁移演练和历史 fixture 查询验证。
- [x] 1.4 增加 workflow version 去重、last-known-good 查询和失败证据幂等所需的唯一约束或 source key；以重复写入与跨组织隔离数据库测试验证。

## 2. Workflow 契约与严格加载

- [x] 2.1 增加 YAML 与严格模板解析所需的最小 Rust 依赖和 workflow 模块骨架；以 `cargo check --manifest-path backend/Cargo.toml` 验证依赖及模块边界。
- [x] 2.2 实现带大小上限的 front matter/Markdown 分离、拒绝未知字段的强类型 schema、枚举和数值范围校验；以合法文件、缺失分隔符、未知字段、非法枚举和超限文件单元测试验证。
- [x] 2.3 实现封闭模板变量/过滤器注册表、strict undefined 校验与强类型任务上下文渲染；以未知变量、未知过滤器、缺失值、转义和合法渲染单元测试验证。
- [x] 2.4 实现受控根目录 canonicalize/符号链接越界校验、版本化安全默认 workflow、规范化序列化和 SHA-256 摘要；以缺失文件、越界链接、稳定摘要和内容变化单元测试验证。
- [x] 2.5 实现仓库策略与平台审批、网络、工具、路径和资源上限的安全交集，并对诊断执行脱敏；以权限扩大被拒绝、策略收紧生效和 secret 不出现在错误中的测试验证。
- [x] 2.6 在仓库根新增示例 `WORKFLOW.md`，覆盖当前 Harness 执行、工具、门禁、超时、重试、hooks 和通知默认契约；以 loader 严格解析该文件的测试验证。

## 3. TaskTracker 与 PostgreSQL adapter

- [x] 3.1 定义不暴露 SQLX/表结构的异步 TaskTracker 领域端口、作用域上下文、领域任务和分类错误；以内存测试实现编译及错误分类单元测试验证。
- [x] 3.2 通过模板优先方式实现 DevRail PostgreSQL tracker adapter，复用 Repository 的 claim、租约、attempt、状态历史和 System Actor 事务；以现有 scheduler PostgreSQL 集成测试继续通过验证。
- [x] 3.3 将任务进入 `queued` 与修订号、任务输入及已验证 workflow 快照原子持久化，并禁止原地修改已排队输入；以成功入队、回滚、过期修订和非法修改集成测试验证。
- [x] 3.4 让候选读取在 SQL 中强制组织/部门/所有者、环境健康、活动 run、退避和截止时间条件，同时保持优先级与 aging 排序；以跨组织、暂不可派发和并发 claim 集成测试验证。
- [x] 3.5 将 task scheduler 的任务访问切换到注入的 TaskTracker，移除其对具体 task SQL/Repository 细节的直接依赖；以 mock tracker 调度测试和真实 PostgreSQL 对账测试验证行为一致。

## 4. Workflow 快照与动态 reload

- [x] 4.1 实现 workflow version Repository 的按范围保存、摘要去重、last-known-good 读取和安全默认值恢复；以并发保存、跨组织查询和重启恢复数据库测试验证。
- [x] 4.2 让 run 创建在同一事务中复制或稳定引用 task 的 workflow 来源、版本、摘要和规范化快照，并在不一致时 fail closed；以 run 创建、重试同一 attempt 和身份不一致集成测试验证。
- [x] 4.3 实现有界、可取消、带抖动和摘要短路的 workflow reload worker，合法版本原子发布且只供之后入队的任务使用；以运行中改文件、既有 queued task 不变和新 task 使用新版本的受控文件系统测试验证。
- [x] 4.4 无效候选保留持久化 last-known-good，并按环境、候选摘要和错误类别去重告警、指标及 System Actor 审计；以重复坏版本、恢复合法版本、删除文件和 worker 重启测试验证。
- [x] 4.5 将 reload worker 接入应用优雅启动/停止、健康状态和低基数 Prometheus 指标；以后端 worker 生命周期测试及 metrics 文本断言验证。

## 5. API、界面与可观测性

- [x] 5.1 在 task/run 详情 API 中只读暴露修订号、workflow 来源、声明版本和摘要，不暴露完整提示正文或敏感配置；以后端权限、范围、序列化和 OpenAPI snapshot 测试验证。
- [x] 5.2 重新生成 Angular API 客户端，并在现有任务/run 详情诊断区域显示简体中文 workflow 身份与版本；以前端组件测试、无英文新增文案扫描和生产构建验证。
- [x] 5.3 为 tracker/reload 增加低基数成功、失败、回退和延迟指标及结构化 trace 字段；以 metrics 单元测试和日志脱敏测试验证不包含路径、正文或 secret。

## 6. 端到端验收与文档

- [x] 6.1 增加从有效 `WORKFLOW.md` 入队、task 快照、scheduler claim、run 快照到 Harness 输入的 PostgreSQL/受控假 app-server 端到端测试；以单次 run、相同摘要和无重复执行断言验证。
- [x] 6.2 增加非法 reload、last-known-good、进程重启和跨组织隔离的端到端回归测试；以候选不生效、审计去重、旧 run 不漂移和恢复后新任务生效断言验证。
- [x] 6.3 更新 Symphony 专项需求、实现状态、架构、调度运维手册和 WORKFLOW.md 使用说明，移除已删除审计输入文档的失效引用；以文档链接检查和需求—测试证据矩阵复核验证。
- [x] 6.4 实现与验收完成后将 ADR-0003 状态更新为 Accepted，并补充迁移、模块、测试和运维证据链接；以 ADR 链接可达和 OpenSpec requirement ID 可追溯验证。

## 7. 质量门禁与交付

- [x] 7.1 对后端、前端、迁移和 workflow 组件执行定向 format/lint/check/test/build，修复全部诊断且不增加 Clippy allow；以 `cargo flow verify` 的组件结果验证。
- [x] 7.2 执行 `cargo flow verify --all`，确保 secret scan、审计、Rust/Angular 测试、OpenAPI、迁移、供应链和构建全部通过，并保存最终证据。
- [ ] 7.3 使用明确文件清单提交、推送并创建或更新 PR，监控 CI、arc-flow、供应链与 CodeQL；以提交 SHA、PR URL 和全部必需检查成功验证。
