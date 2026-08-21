# arc-flow

`arc-flow` 是可复用的 Rust 开发工作流与架构门禁 CLI。源码位于 `codex-audit-pipeline/tools/arc-flow/`；独立二进制是标准入口，本仓库额外提供 `cargo flow` 别名。

它统一负责 changed paths、secret scan、architecture audit、环境体检、外部命令编排、测试结果计数、超时与中断处理，以及临时服务生命周期。Git hook 只保留启动器，流程判断不依赖 Shell 脚本。

## 阅读导航

- 在 arc-admin 中开发：看“本仓库快速开始”和“典型工作流”；
- 接入新项目：看“安装与新项目接入”和“内置预设”；
- 增加命令、组件或 CI profile：看“选择模型”和 [schema v2 配置参考](docs/configuration.md)；
- 处理失败：看“报告与日志”和“故障排查”；
- 扩展 Rust 引擎：看“无需改代码的范围”和“需要改 Rust 的边界”。

## 工作模型

`arc-flow` 把项目流程拆成四类数据：

1. **scope rule**：把 Git 变更路径映射成 component；
2. **profile**：从同一 component 中选择不同强度的步骤，例如 `hook`、`full`、`ci`；
3. **step**：声明一个 `program + args[]` 外部命令、超时、日志、parser 和 service 依赖；
4. **gate**：固定先运行 secret scan 和 audit，成功后才允许外部步骤执行。

运行 `verify` 时的数据流：

```text
Git 变更文件
  -> scope.rules
  -> components
  -> component + profile 匹配的 steps
  -> secret scan
  -> architecture audit
  -> 按配置顺序执行 steps
  -> JSON / Markdown / log 报告
```

component、profile、命令、路径、parser 和 service 都来自 TOML。常规项目迁移不需要在 Rust 中增加枚举或修改匹配分支。

## 本仓库快速开始

arc-admin 已在 `.cargo/config.toml` 中配置：

```toml
[alias]
flow = "run --quiet --locked --manifest-path codex-audit-pipeline/tools/arc-flow/Cargo.toml --"
```

因此无需全局安装，直接从仓库根运行：

```bash
# 1. 校验工具、依赖、Git 和测试服务条件
cargo flow doctor

# 2. 查看当前变更影响范围
cargo flow scope

# 3. 验证受影响组件
cargo flow verify

# 4. 提交或创建 PR 前运行全部组件
cargo flow verify --all
```

首次克隆还应准备依赖、测试镜像和 Git hook：

```bash
cd frontend && npm ci && cd ..
cp backend/.env.example backend/.env
docker pull postgres:16-alpine
git config core.hooksPath codex-audit-pipeline/hooks
cargo flow doctor
```

`doctor` 不会连接生产数据库。后端完整测试使用已有 `TEST_DATABASE_URL`，或者启动绑定到 `127.0.0.1` 随机端口的一次性 PostgreSQL。启用 `isolated-postgres` 策略后，测试库名称必须以 `_test` 或 `-test` 结尾，并拒绝复用 `DATABASE_URL`；远程测试库还需显式设置 `ARC_FLOW_ALLOW_REMOTE_TEST_DATABASE=1`。

## 安装与新项目接入

从源码安装独立二进制：

```bash
cargo install --locked --path codex-audit-pipeline/tools/arc-flow
arc-flow --version
arc-flow presets
```

在目标项目中初始化：

```bash
arc-flow --project-root /path/to/new-project init --preset rust-api
arc-flow --project-root /path/to/new-project config check
arc-flow --project-root /path/to/new-project doctor
arc-flow --project-root /path/to/new-project verify --all
```

推荐接入顺序：

1. 选择最接近技术栈的预设；
2. 修改 `.arc-flow/flow.toml` 中的路径、component、scope 和步骤；
3. 在 `.arc-flow/audit.toml` 增加项目自己的架构规则；
4. 在 `.arc-flow/secrets.toml` 增加业务或供应商特有的凭据规则；
5. 执行 `config check`，先解决引用和 schema 错误；
6. 执行 `doctor`，补齐本机工具、依赖、镜像或环境变量；
7. 在干净仓库执行 `verify --all`，确认所有 component 都能运行；
8. 在 CI 中使用相同命令，并按需安装只负责调用 `arc-flow hook` 的薄 hook。

`init` 会创建目标目录，但 `scope` 和 `verify` 需要目标目录是 Git worktree。项目已有配置时默认拒绝覆盖；只有确认目标内容可替换时才使用 `--force`。

## 内置预设

| 预设                    | 用途                        | 初始步骤                                |
| ----------------------- | --------------------------- | --------------------------------------- |
| `generic`               | 任意 Git 项目               | working tree 和 staged whitespace check |
| `rust-api`              | 单 Rust crate               | fmt、Clippy、check、test                |
| `angular-only`          | Angular/npm                 | lint、format check、test、build         |
| `angular-rust-postgres` | Angular + Rust + PostgreSQL | 双端检查、测试、构建和临时数据库        |

`init` 以同目录临时文件和原子重命名写入 `.arc-flow/flow.toml`、`.arc-flow/audit.toml`、`.arc-flow/secrets.toml` 和忽略报告目录的 `.arc-flow/.gitignore`，不会留下半写文件，也不会覆盖已有配置，除非显式传入 `--force`。`config migrate` 对目标配置使用相同的原子写入策略，并在缺失时生成 Secret Scan v2 默认规则。新建 audit v2 文件预置 Rust、TypeScript、JavaScript、SQL、TOML 和 YAML 的词法配置，可直接追加第一条规则。

预设是起点，不是运行时分支。初始化完成后，所有行为都由项目内 TOML 决定：可以重命名 component、增加 `ci` profile、换成 MySQL/Redis、调整目录或替换任意步骤，无需保留预设原有名称。

## 命令总览

| 命令                                                    | 用途                                  |
| ------------------------------------------------------- | ------------------------------------- |
| `arc-flow presets`                                      | 列出内置项目预设                      |
| `arc-flow init --preset <name>`                         | 生成 schema v2 配置                   |
| `arc-flow doctor [--strict] [--json]`                   | 执行配置声明的环境检查                |
| `arc-flow scope [--staged\|--base REF\|--all] [--json]` | 列出变更和选择的 components           |
| `arc-flow secrets [--staged] [--json]`                  | 扫描高置信凭据模式，只输出命中文件名  |
| `arc-flow audit [--json]`                               | 执行正则架构规则并生成审计报告        |
| `arc-flow verify`                                       | 按工作区变更执行默认 profile          |
| `arc-flow verify --profile ci --all`                    | 对全部 components 执行指定 profile    |
| `arc-flow hook`                                         | 对暂存快照执行 hook profile           |
| `arc-flow step <id>`                                    | 通过 secrets/audit 后单独运行一个步骤 |
| `arc-flow config check`                                 | 校验 schema、引用、路径和环境覆盖     |
| `arc-flow config print --resolved`                      | 输出最终生效配置                      |
| `arc-flow config migrate`                               | 将 schema v1 转换成 v2                |
| `arc-flow parse-logs`                                   | 提取 JSON Lines ERROR trace 上下文    |

所有命令都支持全局 `--project-root <PATH>` 和 `--config <PATH>`。命令成功返回 0；配置错误、门禁失败、步骤失败、超时或中断返回非 0，适合直接用于 CI。

本仓库使用等价的 Cargo 别名：

```bash
cargo flow scope
cargo flow verify --components backend,frontend
cargo flow verify --profile full --all
```

## 典型工作流

### 日常开发

```bash
# 编码前确认范围
cargo flow scope

# 编码后只验证命中的组件
cargo flow verify

# 定向重复运行一个步骤；仍会先过 secrets 和 audit
cargo flow step frontend.tests
```

工作区没有变更时，默认 scope 不选择 component，`verify` 只运行固定门禁。需要确认整个仓库时必须显式传 `--all`。

### 提交前

```bash
git add <明确的文件清单>
cargo flow scope --staged
cargo flow hook
git commit -m "..."
```

仓库的 pre-commit 会自动执行 `cargo flow hook`。hook profile 只保留快速确定性检查，不代替完整测试。

### PR 或发布前

```bash
cargo flow verify --all
```

该命令忽略变更路径，选择配置中的所有 component，并执行默认 `full` profile。

### 比较某个基线

```bash
cargo flow scope --base origin/main
cargo flow verify --base origin/main
```

`--base REF` 使用 `REF...HEAD` 的已提交变更，不包含未提交工作区内容，适合 CI 或 PR 分支验证。

### 手动指定范围

```bash
cargo flow verify --components backend
cargo flow verify --components backend,frontend --profile full
```

显式 components 会覆盖自动 scope，并且不能与 `--staged`、`--base`、`--all` 同时使用。未知 component 或 profile 会立即失败。

## Schema v2 概览

项目根由 `.arc-flow/flow.toml` 标识，不要求固定的 `backend/`、`frontend/` 或工具目录。component 和 profile 都是配置中的小写字符串 ID。

```toml
version = 2

[project]
name = "example"
default_profile = "full"
hook_profile = "hook"

[paths]
reports = ".arc-flow/reports"
audit_config = ".arc-flow/audit.toml"
secrets_config = ".arc-flow/secrets.toml"

[paths.aliases.api]
path = "services/api"
env = "API_DIR"

[[scope.rules]]
patterns = ["services/api/**"]
components = ["api"]

[[steps]]
id = "api.clippy"
label = "API Clippy"
component = "api"
profiles = ["full", "hook", "ci"]
program = "cargo"
args = ["clippy", "--manifest-path", "{api}/Cargo.toml"]
cwd = "{root}"
log = "api_clippy.log"
timeout_secs = 180
```

可用占位符包括 `{root}`、`{reports}`、`{audit_config}` 和任意 `[paths.aliases.*]`。命令使用 `program + args[]`，不经过 Shell 字符串解析。

`REPORT_DIR` / `ARC_FLOW_REPORTS`、`AUDITOR_CONFIG` / `ARC_FLOW_AUDIT_CONFIG`、`ARC_FLOW_SECRETS_CONFIG` 和步骤或服务声明的 `*_env` 字段可覆盖配置值。CLI 的 `--project-root` 决定项目根，`--config` 决定读取哪个仓库内配置文件。

完整字段、默认值、限制和示例见 [schema v2 配置参考](docs/configuration.md)。修改配置后先运行：

```bash
arc-flow config check
arc-flow config print --resolved
```

## 主要扩展点

### Doctor

`[[doctor.checks]]` 支持 `command`、`path`、`glob`、`env`、`env-or-file`、`git-config`、`git-remotes`、`version` 和 `service`。`required = false` 的失败表现为 warning，其余为 failure。

```bash
arc-flow doctor            # warning 不影响退出码
arc-flow doctor --strict   # warning 也导致非 0
arc-flow doctor --json     # 给 CI 或其他工具消费
```

### 测试解析器

`[parsers.<id>]` 定义一个或多个正则、计数 capture group 和最低结果数。步骤通过 `parser = "<id>"` 引用，因此 Jest、Pytest、Go test 等文本输出无需修改 Rust 代码即可增加零测试保护。

### 临时服务

`[services.<id>]` 支持两种 provider：

- `kind = "environment"`：从已有环境变量读取连接值并注入步骤；
- `kind = "docker"`：声明镜像、端口、环境、健康检查、连接串和目标环境变量。

Docker provider 可用于 PostgreSQL、MySQL、Redis 等服务。容器使用随机宿主端口，验证结束或异常退出时自动删除。

步骤可用 `services = ["test-postgres", "test-redis"]` 组合多个服务，并以 `remove_env = ["DATABASE_URL"]` 删除继承的运行时变量。每个 service 必须注入不同变量，避免静默覆盖。

### 项目策略

`[policy].required_steps` 声明项目不可缺失的基础步骤。策略属于项目配置，不再编译进通用引擎；新增 component、profile、普通命令、路径别名、regex parser 或 Docker service 都不需要修改 Rust 源码。

## 选择模型

### Working tree

默认 `scope` 合并以下文件集合：

- 未暂存修改：`git diff --name-only`；
- 已暂存修改：`git diff --cached --name-only`；
- 未忽略的未跟踪文件：`git ls-files --others --exclude-standard`。

这些路径按 `[[scope.rules]]` 匹配，命中的 component 去重后用于选择步骤。`[scope].unmatched` 控制未命中路径：`fail`（默认）立即失败并列出文件，`all` 选择全部 component，`ignore` 仅在明确接受漏测风险时忽略。

### Staged

`scope --staged` 和 `hook` 只读取暂存快照。secret scan 也通过 Git index 读取文件内容，而不是读取可能不同的工作区版本。

### All

`--all` 不依赖 Git diff，直接选择配置步骤中出现的全部 component。适合交付门禁和干净 checkout。

### Profile

profile 由步骤的 `profiles = [...]` 隐式声明。`verify` 使用 `[project].default_profile`，`hook` 使用 `[project].hook_profile`，`verify --profile <id>` 可选择其他 profile。

## 固定安全门禁

以下行为不由项目配置关闭：

1. 外部步骤前必须依次通过 secret scan 和 audit；
2. 配置、报告和路径别名不得逃出项目根；
3. 禁止 `sh -c`、`bash -lc` 等 Shell 命令字符串；
4. 未知引用、重复 ID、非法 glob/regex/占位符和越界超时直接失败；
5. service 容器必须声明健康检查并在结束时清理；
6. 多个 service 不得向同一步骤注入同名环境变量。
7. 审计扫描根必须存在且位于项目内，`..` 和逃出项目的符号链接会被拒绝；
8. Doctor、Git 探测和 Docker 生命周期命令都有硬超时，超时时终止整个子进程组。

secret scan 检查 Git 已追踪文件和未忽略的未跟踪文件。具体规则由 `[paths].secrets_config` 指向的 TOML 文件提供，默认覆盖 GitHub/GitLab/npm Token、AWS access key、JWT、命名签名密钥、PostgreSQL 凭据 URL、Webhook、企业微信/钉钉密钥、HTTP Basic Auth 和 PEM 私钥头。捕获值会经过占位符与低信息值过滤；报告只记录文件名，不把凭据内容复制到终端或 JSON。

## v1 迁移

```bash
arc-flow --project-root /path/to/project config migrate \
  --input codex-audit-pipeline/.codex/flow.toml \
  --output .arc-flow/flow.toml
```

迁移器保留源文件，把原有 backend/frontend、doctor、PostgreSQL、parser、scope 和 steps 转成 v2。目标文件已存在时需要显式 `--force`。

迁移后必须执行：

```bash
arc-flow config check
arc-flow doctor
arc-flow verify --all
```

确认新配置通过后，再由人工决定何时删除 v1 文件。迁移命令本身不会删除源文件。

## 验证与报告

`verify` 固定按以下顺序执行：

1. working tree 或 staged snapshot secret scan；
2. auditor 全量架构扫描；
3. 按配置顺序执行选中 component/profile 的步骤。

报告目录由 `[paths].reports` 决定。当前项目使用 `codex-audit-pipeline/.codex/reports/`。

| 文件                  | 内容                            | 常见消费者         |
| --------------------- | ------------------------------- | ------------------ |
| `changed_files.txt`   | 当前 scope 的变更文件，每行一个 | Reviewer           |
| `scope.json`          | scope mode、文件和 components   | CI、自动化工具     |
| `secret_scan.json`    | 扫描模式和命中文件名            | 安全门禁           |
| `review_context.json` | 完整审计结果、规则、文件和行号  | 修复代理、Reviewer |
| `review_context.md`   | 截断的人类可读审计摘要          | 终端或 LLM 上下文  |
| `test_result.json`    | profile、scope、步骤耗时和状态  | CI、统计           |
| `test_result.md`      | 简洁验证摘要和 `TEST_SUMMARY`   | 人工查看           |
| `logs/<step>.log`     | 外部命令完整 stdout/stderr      | 失败诊断           |

终端只展示摘要。步骤失败时先看 `test_result.md` 中的日志路径，再打开对应日志；不要只根据最后一行猜测根因。

JSON Lines 应用日志可提取同一 trace 的上下文：

```bash
arc-flow parse-logs \
  --input path/to/application.jsonl \
  --output /tmp/error-context.txt
```

解析器优先选择第一条 `level = ERROR` 所在的 `trace_id`，支持从事件字段、`data`、当前 `span` 和 `spans` 中提取，再收集相同 trace 的结构化记录；没有 trace 时退化为原始日志最后 30 行。

## Git Hook

当前仓库使用：

```bash
git config core.hooksPath codex-audit-pipeline/hooks
```

`pre-commit` 执行 `cargo flow hook`。hook profile 不运行数据库集成测试或 production build；交付前使用 `cargo flow verify --all`。

业务代码模板由 `.codex/templates/manifest.json` 统一登记。`hook` 和 `full` 流程会执行模板质量门禁，检查清单覆盖、占位符一致性和示例渲染结果；TypeScript 模板使用编译器诊断，Rust 模板使用 `rustfmt --check`，SQL 模板检查引号、注释、括号和语句终止符。对应入口为 `scripts/check-templates.mjs`，负向测试位于 `scripts/check-templates.test.mjs`。

独立安装方式的新项目可以创建同样的薄 hook：

```sh
#!/bin/sh
set -eu
root="$(git rev-parse --show-toplevel)"
cd "$root"
exec arc-flow hook
```

hook 只负责定位根目录和启动二进制，所有选择、门禁和步骤仍在 Rust 与 TOML 中。

## CI 集成

CI 推荐执行全量 profile，而不是依赖 runner 上的工作区 diff：

```bash
arc-flow config check
arc-flow doctor --strict
arc-flow verify --all
```

如需复用外部测试服务，向 job 注入 service 配置的 `external_env`；否则预拉取配置镜像并允许 runner 访问 Docker daemon。缓存 Cargo、npm 和构建目录只影响性能，不应跳过 `verify --all`。

无论成功或失败，都建议上传 `[paths].reports` 目录作为 artifact。这样可以保留审计行号、步骤耗时和完整日志。

## 故障排查

### 没有 component 被选中

工作区没有变更时不会选择 component。变更路径没有命中 scope rule 时，默认 `unmatched = "fail"` 会列出遗漏文件并失败；修正 `[[scope.rules]]`，或为希望触发全量验证的项目设置 `unmatched = "all"`。只有明确接受未匹配文件不触发验证时才使用 `ignore`。

### `unknown component` / `unknown profile`

component 来自 `[[steps]].component`，profile 来自 `[[steps]].profiles`。运行 `arc-flow config check` 查看引用错误，或 `config print --resolved` 确认最终配置。

### Docker daemon 不可用

可以启动 Docker，或者设置 service 的 `external_env`，例如 `TEST_DATABASE_URL`。`doctor` 中 `required = false` 的 service 检查只产生 warning，但真正依赖该 service 的步骤仍会失败。

### Docker image 不存在

Docker provider 使用 `--pull=never`，不会在验证中隐式访问网络。按 Doctor 提示执行 `docker pull <image>`，或通过 `image_env` 指向本机已有镜像。

### 测试命令成功但 parser 失败

查看步骤日志，确认测试框架实际输出与 `patterns` 一致，并且 capture group 是整数。不要把 `minimum` 改为 0；应修正规则或测试命令，确保零测试不会被误判为成功。

### 步骤超时

先看日志判断是死锁、网络等待还是超时过短。需要项目或 CI 差异化时设置步骤或 service 的 `timeout_env`，不要复制两套配置。步骤允许 1 到 3600 秒，service 整个启动过程允许 1 到 300 秒；Doctor 单项检查用 `timeout_secs` 控制，默认 15 秒、范围 1 到 300 秒。

### 配置路径被拒绝

配置、报告、audit 文件和 path alias 必须位于项目根内。不要用 `..`、仓库外绝对路径或逃出仓库的符号链接；把需要的输入放到仓库内，或通过受控环境变量向步骤传值。

## 无需改代码的范围

以下变化只修改 `.arc-flow/flow.toml` 或 audit TOML：

- 新增或重命名 component、profile、step；
- 接入 Go、Java、Python、Node 或其他 CLI 工具；
- 调整 monorepo 路径和 scope；
- 增加 Doctor 检查；
- 增加 regex 测试结果解析器；
- 增加 Docker 或环境变量 service；
- 增加硬性规则、分层规则和逐行允许规则；
- 为 CI 增加独立超时、镜像或目录覆盖变量。

## 需要改 Rust 的边界

只有引擎出现新的行为类别时才需要修改源码，例如：

- 新增非 Docker 的服务生命周期 provider；
- 新增无法用 regex 表达的测试报告格式；
- 新增现有 Doctor kind 无法表达的检查协议；
- 修改凭据识别算法、进程取消机制或固定安全策略；
- 增加远程执行、并行调度或新的报告格式。

这类改动应同时增加单元测试、内置预设验证和配置兼容性说明，并提升版本号。

## 当前审计规则

`codex-audit-pipeline/.codex/audit.toml` 约束 arc-admin 的 SQL 写入层、Handler/Service、Angular Component/Service 和代码模板。audit 配置当前 schema 为 v2，必须显式声明 `version = 2` 和 `[engine]`；规则扩展名没有对应 `comment_syntax` 时会 fail closed。旧版字符串 allowlist、缺失 engine 和版本升级方法见[配置迁移参考](docs/configuration.md#audit-v2-migration)。`arch_rules.allowed_patterns` 可声明逐行例外，不存在写死的 model trait 放行逻辑。

auditor 以整文件为单位执行正则检查并把命中映射回起始代码行：支持跨行规则、扩展名过滤、路径排除、显式类型的路径 allowlist 和起始行 allowed pattern。行注释、块注释及字符串定界符按扩展名配置，扫描时跟踪词法状态；正则默认启用 multi-line 模式。需要抽象语法树级判断时，应把 Clippy、ESLint 或其他语言 lint 工具配置为 step。

## 开发 arc-flow 本身

```bash
cargo fmt --manifest-path codex-audit-pipeline/tools/arc-flow/Cargo.toml -- --check
cargo clippy --manifest-path codex-audit-pipeline/tools/arc-flow/Cargo.toml \
  --locked --all-targets --all-features -- -D warnings
cargo test --manifest-path codex-audit-pipeline/tools/arc-flow/Cargo.toml --locked
cargo flow verify --components workflow
```

修改配置模型时还应验证全部内置预设和 v1 migration 测试；修改 service provider 时必须执行 `cargo flow verify --all`，确认一次性容器会在成功和失败路径上清理。
