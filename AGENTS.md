# 项目全局公约（Angular + Rust 双端工作流）

## 📁 目录结构

```
仓库根/
├── frontend/                  # Angular 22 + Material M3 前端
├── backend/                   # Rust + Axum + SQLX 后端（handlers/services/repositories/models）
├── .arc-flow/                 # arc-flow schema v2 项目配置
└── codex-audit-pipeline/      # 工作流工具（自包含）
    ├── hooks/                 # pre-commit 极薄启动器
    ├── tools/arc-flow/        # Rust CLI（审计 + 全流程编排）
    └── .codex/                # 审计规则 / 模板 / 报告产物
```

所有命令从仓库根执行，`arc-flow` 通过 `.arc-flow/flow.toml` 自动定位仓库根。

## ⛔ 数据库安全红线
- SQL 写操作（INSERT/UPDATE/DELETE/DROP/ALTER/TRUNCATE、`.execute()`/`.exec()`）**只允许出现在 Repository / db / migrations / tests / seed 层**。
- Service、Handler、路由层出现写操作 SQL 会被 auditor 硬性扫描判定为 blocker，直接打回。
- AI 或任何子代理禁止直接连接生产数据库；测试一律走 `cargo flow verify` 管理的 `TEST_DATABASE_URL` 或一次性 PostgreSQL 容器。

## 🚫 读取策略（Token 节约 + 准确性平衡）
- **默认不读源码**。编码/修复阶段先读 `codex-audit-pipeline/.codex/reports/review_context.json`（完整明细，含文件:行号），按行号精准读取，单次 ≤ 200 行。
- **reviewer 子代理例外**：允许读取 `codex-audit-pipeline/.codex/reports/changed_files.txt` 中列出的变更文件（评审必须基于真实代码）。
- 严禁读取 `node_modules/`、`target/` 目录。
- `codex-audit-pipeline/.codex/reports/review_context.md` 是截断版（≤4KB），只用于给 LLM 展示；**判定以 review_context.json 完整数据为准**。

## 🔧 代码修复输出规范
1. 禁止输出整个文件。
2. 必须使用 `git diff` 补丁格式，仅输出改动部分：
   ```diff
   --- a/backend/src/services/user.rs
   +++ b/backend/src/services/user.rs
   @@ -40,7 +40,7 @@
   -    let data = cache.get(key).unwrap();
   +    let data = cache.get(key)?;
   ```
3. 多文件修改时，每个文件单独一个 diff 块。

## 🤖 状态码协议
### Reviewer
- `PASS:0` → 无违规，进入下一环节。
- `FAIL:文件:行号,...` → 按补丁格式修复指定行号，重新提交审核。
### Tester
- `PASS:0` → 测试通过，执行本地提交。
- `FAIL:错误简述` → 定位问题，按补丁格式修复，重新测试。
### 熔断
- 连续 2 次收到相同文件+行号的 `FAIL` → 输出 `⛔ 熔断: 连续违规未修复，请求人工介入`，终止循环。

## 📁 模板优先策略
- 涉及 CRUD / Repository / Service / Angular Service / Model 时：
  1. 先检查 `codex-audit-pipeline/.codex/templates/` 是否存在对应 `.tmpl`。
  2. 存在则禁止从零编写，先读模板，仅替换 `{{PLACEHOLDER}}`。

## 🧭 变更范围路由（替代 git diff HEAD~1）
- 编码前先执行：`cargo flow scope`。
- `scope` 支持默认工作区、`--staged`（pre-commit）、`--base <ref>`（PR 增量）和 `--all`（完整验证），并写入 `scope.json`。
- 以输出的 backend / frontend / workflow components 决定调用哪些 reviewer 与测试范围：
  - 仅 backend → `reviewer-rust`，执行 `cargo flow verify --components backend`
  - 仅 frontend → `reviewer-angular`，执行 `cargo flow verify --components frontend`
  - 双端 → 串行调用两端
- 判定顺序由 `cargo flow verify` 固定为：secret scan → 审计门禁 → lint/compile/test/build → LLM reviewer（可选）。

## 🔒 提交安全
- 只允许 `git add <变更文件清单>` 和 `git commit -m "..."`。
- **严禁** `git push` / `git push --force` / `git reset --hard`。
- 提交完成后输出：`✅ 代码已本地提交，请人工执行 git push 或创建 PR。`
- 提交前快速门禁由 `codex-audit-pipeline/hooks/pre-commit` 执行（lint + 审计），完整测试不在提交钩子里跑。
- 提交到远端前必须执行 `cargo flow verify --all`；该命令不会修改源码，后端测试只使用 `TEST_DATABASE_URL` 或一次性 PostgreSQL 容器。

## 🧠 错误诊断规范
- `tester` 返回失败时，按顺序排查：
  1. 若日志为 JSON Lines：优先取 `level == "ERROR"` 的日志所在 `trace_id`（`cargo flow parse-logs` 已按此实现），再回溯同 trace_id 的 info 日志。
  2. 检查错误上下文中 `error` 字段，那是根因。
  3. 若无非结构化日志，直接看测试报告失败详情。

## 🌐 界面语言
- 用户可见文案统一使用简体中文，包括标题、按钮、表单标签、提示、错误消息、Tooltip 和 ARIA 标签。
- `RBAC`、`API`、角色代码、权限代码、接口字段及用户自行录入的内容属于技术或业务数据，不做自动翻译。
- 新增功能不得引入未翻译的英文界面文案；当前系统不提供语言切换，也不维护第二套翻译资源。

## 🔄 完整工作流
```
用户发起需求
  → 1. cargo flow scope 确定范围
  → 2. 模板优先：命中 CRUD/Repository/Service 则读模板替换占位符
  → 3. 编码（仅输出 diff 补丁）
  → 4. cargo flow verify（secret → audit → lint/check/test/build）
  → 5. 可选 LLM reviewer：只读变更文件做语义/架构审查
  → 6. cargo flow verify --all 做交付前完整验证
  → 7. 本地提交（禁止 push），pre-commit 调用 cargo flow hook
```
