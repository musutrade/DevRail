# 仓库工作流契约

DevRail 从每个受控仓库根目录读取 `WORKFLOW.md`。它是版本控制内的执行契约，不是扩大平台权限的配置入口；根目录示例见 [`WORKFLOW.md`](../WORKFLOW.md)。

## 文件结构

文件必须是 UTF-8，最大 256 KiB，由 YAML front matter 和非空 Markdown 模板正文组成。front matter 只允许以下字段，未知字段会让整份候选版本失败：

| 字段 | 约束 |
| --- | --- |
| `version` | 小写字母、数字和连字符，最长 64 字符 |
| `execution_mode` | `implementation`、`review` 或 `maintenance` |
| `tools` | 工具白名单、网络开关和危险命令审批；只能收紧平台策略 |
| `quality_gates` | 小写标识列表，必须命中平台支持的门禁 |
| `timeout_seconds`、`stall_timeout_seconds` | 正整数且不超过环境上限 |
| `retry` | 最大尝试次数、基础延迟和最大延迟，均受平台上限约束 |
| `hooks` | `before_run`/`after_run`，当前只允许 `cargo-flow-scope`、`cargo-flow-verify` |
| `notifications.events` | `awaiting_approval`、`succeeded`、`failed` 的子集 |

## 模板能力

模板使用 strict undefined。允许变量为 `task.id/title/goal/background/acceptance_criteria/constraints`、`repository.name/default_branch` 和 `environment.name/workspace_root`；允许过滤器为 `trim`、`lower`、`upper`、`default`。未知变量、过滤器或缺失必填值不会被替换为空字符串。

模板不能读取文件、环境变量、网络或执行函数。任务入队时以强类型上下文渲染，渲染正文与规范化配置一起进入不可变派发快照。

## 生命周期与安全

1. `draft → queued` 时 loader 校验受控路径、schema、模板和平台安全交集，并在同一事务保存 task 修订号及 workflow 来源、版本、SHA-256 摘要和快照。
2. scheduler claim 后，run 创建再次在 SQL 中核对 task 修订号与 workflow 三元身份；Harness 使用快照里的 `renderedPrompt`。
3. reloader 只发布完整合法的新版本。既有 queued task 和活动 run 永不漂移；非法候选保留 PostgreSQL last-known-good，并按环境、候选摘要和错误类别去重。
4. 文件缺失使用版本化安全默认值；符号链接越过受控根目录会被拒绝。网络默认关闭，危险命令审批、脱敏、数据范围和资源上限不能由仓库覆盖。

## 修改与排障

- 修改前先运行 `cargo flow scope`，修改后运行 `cargo flow verify --all`；推荐原子替换文件，避免半写入候选。
- 新任务未采用新版本时，检查环境工作区是否位于 `DEVRAIL_RUN_WORKSPACE_ROOT`、reload 健康指标和 `devrail.workflow.reject` 审计。
- API 与日志只显示来源、版本、摘要和脱敏类别，不返回完整提示正文或平台敏感配置。
- 需要改变已排队任务时，应取消并重建；禁止直接修改 task/run 快照。
