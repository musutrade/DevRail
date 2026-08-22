# 代码审计模板生产化待办

更新日期：2026-08-21

## 当前基线

- [x] 审计规则、密钥规则、本地测试数据库放行条件、占位符策略和报告参数已配置化。
- [x] allowlist 已改为显式 `path-prefix` / `regex` 类型。
- [x] 注释扫描支持按扩展名配置行注释、块注释、嵌套块注释和字符串定界符。
- [x] `cargo flow verify --all` 已通过，包括后端、前端、E2E、真实全栈 smoke 和生产构建。
- [x] 当前审计报告为 0 blocker、0 error、0 warning。
- [x] `cargo flow doctor` 的测试数据库告警已消除：CI 使用隔离的 `TEST_DATABASE_URL` 执行 `doctor --strict --json` 并保留 artifact；干净环境的 Docker 路径已复验为 0 failure、0 warning。

生产级定位：当前可用于 arc-admin 的受控技术栈，但 auditor 仍是 Clippy、ESLint、SAST 等工具之前的补充门禁，不能作为唯一安全扫描器。

## P0：发布前必须完成

### 审计配置版本与迁移

- [x] 为 `audit.toml` 增加明确的 schema `version`。
- [x] 为旧版字符串 allowlist 和缺少 `[engine]` 的配置提供迁移路径，或输出包含升级步骤的明确错误。
- [x] 增加旧配置、当前配置、未知版本和未知字段的兼容性测试。
- [x] 验收：已有项目升级 `arc-flow` 后不会静默改变规则语义；无法迁移时必须 fail closed。

### 默认 preset 完整性

- [x] 在 `empty.audit.toml` 中提供 Rust、TypeScript、SQL 等常用扩展名的默认注释语法。
- [x] 当规则使用的扩展名没有对应 `comment_syntax` 时，配置校验应告警或拒绝运行。
- [x] 增加初始化新项目后添加第一条规则的端到端测试。
- [x] 验收：新项目不会因为遗漏注释配置而扫描注释中的示例代码。

### 版本发布治理

- [x] 根据兼容性影响调整 `arc-flow` crate 版本。
- [x] 更新根目录 `CHANGELOG.md`，记录 secret config v2、显式 allowlist 和 audit engine 配置变更。
- [x] 在配置文档中提供旧配置到新配置的完整迁移示例。
- [x] 验收：发布版本、preset、文档和配置解析器声明的 schema 保持一致。

## P1：通用生产模板必须完成

### 词法扫描准确性

- [x] 覆盖任意 `#` 数量的 Rust raw string，而不是只覆盖常见层级。
- [x] 覆盖 TypeScript 正则字面量和模板字符串中的嵌套表达式。
- [x] 覆盖 PostgreSQL dollar-quoted string 和转义字符串。
- [x] 评估使用 tree-sitter 等成熟 parser；当前继续使用受配置约束的自研词法器，并在文档中明确支持边界；需要 AST 级判断时仍交给语言 lint/SAST。
- [x] 验收：每种语言都有字符串、注释、嵌套、未闭合输入及跨行规则的正反例测试。

### 稳健性测试

- [x] 为注释区间扫描、配置反序列化和路径匹配增加确定性 property-style 随机测试；持续接入 fuzz runner 时复用同一入口。
- [x] 增加畸形 UTF-8、超长单行、超大文件、空文件和二进制文件测试，并分别断言可处理输入与 fail-closed 输入。
- [x] 增加 Windows 路径分隔符及非 ASCII 路径测试。
- [x] 验收：随机输入不得 panic、越界或产生非确定性结果。

### 大仓库性能

- [x] 预编译 allowlist 正则，不得对每个文件重复编译。
- [x] 同一文件被多条规则扫描时复用文件内容、行索引和注释区间。
- [x] 建立至少 1 万文件规模的基准，记录文件数、耗时、峰值 RSS、单线程/并行度和结果一致性；CI artifact 保留运行结果。
- [x] 验收：基准脚本在目标 CI 机器上满足 30 秒与 512 MiB 峰值 RSS 预算，且并行结果与串行扫描一致。

## P2：运营与交付完善

- [x] 增加 Linux 和 Windows CI matrix；项目不声明 macOS 支持，因此不伪造 macOS 通过证据。
- [x] 发布构建生成二进制 SHA-256 校验和、SPDX 2.3 SBOM，并记录 Rust 工具链与最低支持版本。
- [x] 在操作手册中补充 secret/audit 生产验证、边界和常见失败定位章节。
- [x] 在临时新仓库执行初始化、配置迁移、首条规则、故意失败和恢复演练；真实远端 PR 仍需 GitHub 执行。

## 最终验收命令

```bash
cargo flow doctor
cargo fmt --manifest-path codex-audit-pipeline/tools/arc-flow/Cargo.toml -- --check
cargo clippy --manifest-path codex-audit-pipeline/tools/arc-flow/Cargo.toml --locked --all-targets --all-features -- -D warnings
cargo test --manifest-path codex-audit-pipeline/tools/arc-flow/Cargo.toml --locked
cargo flow verify --components workflow
cargo flow verify --all
```

交付条件：以上命令全部通过，审计报告无违规，升级路径有自动化测试，发布说明明确 auditor 不是语言级 SAST 的替代品。
