# arc-flow schema v2 配置参考

本文档说明 `.arc-flow/flow.toml` 和审计规则文件的完整配置模型。首次接入请先阅读上一级的 [操作手册](../README.md)，再按需查阅本参考。

## 1. 文件与加载顺序

默认项目配置是仓库内的 `.arc-flow/flow.toml`。`arc-flow` 从当前目录向父目录查找该文件，也可以显式指定：

```bash
arc-flow --project-root /path/to/project config check
arc-flow --project-root /path/to/project --config config/ci-flow.toml config check
ARC_FLOW_CONFIG=config/ci-flow.toml arc-flow --project-root /path/to/project config check
```

配置路径必须位于项目根内部。相对路径按项目根解析；绝对路径、`..` 越界路径和指向仓库外的符号链接会被拒绝。

生效顺序如下，后者覆盖前者：

1. `.arc-flow/flow.toml`；
2. 配置字段声明的环境变量；
3. 通用环境变量，例如 `ARC_FLOW_CONFIG`、`ARC_FLOW_REPORTS`；
4. CLI 的 `--project-root` 和 `--config`。

使用以下命令查看配置是否有效以及环境覆盖后的结果：

```bash
arc-flow config check
arc-flow config print --resolved
```

## 2. 命名和路径约束

所有 project、component、profile、step、parser、service、doctor check 和 path alias ID 必须：

- 非空；
- 只包含小写 ASCII 字母、数字、`.`、`-`、`_`；
- 例如 `api.tests`、`frontend`、`pre_commit` 合法，`API Tests` 不合法。

环境变量名只能包含大写 ASCII 字母、数字和下划线。`program` 必须是 PATH 中的可执行文件名，不能包含 `/` 或 `\\`。

仓库路径必须是非空相对路径，不能包含父目录跳转。以下占位符可用于参数、Doctor 路径和部分配置值：

| 占位符           | 含义                                                       |
| ---------------- | ---------------------------------------------------------- |
| `{root}`         | 项目根的绝对路径                                           |
| `{reports}`      | 报告目录的绝对路径                                         |
| `{audit_config}` | 审计规则文件的绝对路径                                     |
| `{<alias>}`      | `[paths.aliases.<alias>]` 解析后的绝对路径                 |
| `{host_port}`    | 仅 Docker service 的 `connection` 使用，由随机宿主端口替换 |

步骤的 `cwd` 必须是单个 `{root}` 或 `{<alias>}`，不能写成 `{root}/backend`。需要子目录时先声明 path alias。

## 3. 顶层结构

```toml
version = 2

[project]
# ...

[paths]
# ...

[policy]
# ...

[[doctor.checks]]
# ...

[services.example]
# ...

[parsers.example]
# ...

[[scope.rules]]
# ...

[[steps]]
# ...
```

| 区域                | 必需 | 用途                        |
| ------------------- | ---- | --------------------------- |
| `version`           | 是   | 当前固定为 `2`              |
| `[project]`         | 是   | 项目标识和默认 profile      |
| `[paths]`           | 是   | 报告、审计规则和路径别名    |
| `[policy]`          | 否   | 声明不可缺失的步骤          |
| `[scope]`           | 否   | 未匹配变更路径的处理策略    |
| `[[doctor.checks]]` | 否   | 本机环境体检                |
| `[services.*]`      | 否   | 外部环境或 Docker 临时服务  |
| `[parsers.*]`       | 否   | 从测试日志计算结果数        |
| `[[scope.rules]]`   | 是   | 变更路径到 component 的映射 |
| `[[steps]]`         | 是   | 实际执行的命令步骤          |

未知字段会直接导致解析失败，避免拼写错误被静默忽略。

## 4. `[project]`

```toml
[project]
name = "orders-api"
default_profile = "full"
hook_profile = "hook"
```

| 字段              | 说明                                      |
| ----------------- | ----------------------------------------- |
| `name`            | 项目 ID，也用于临时容器名称               |
| `default_profile` | `arc-flow verify` 未传 `--profile` 时使用 |
| `hook_profile`    | `arc-flow hook` 使用的快速 profile        |

两个 profile 都必须至少被一个步骤引用。profile 不需要单独声明，它由步骤的 `profiles` 集合产生。

## 5. `[paths]` 和 aliases

```toml
[paths]
reports = ".arc-flow/reports"
audit_config = ".arc-flow/audit.toml"
secrets_config = ".arc-flow/secrets.toml"

[paths.aliases.api]
path = "services/api"
env = "ARC_FLOW_API_DIR"

[paths.aliases.web]
path = "apps/web"
```

| 字段                | 说明                               |
| ------------------- | ---------------------------------- |
| `reports`           | JSON、Markdown 和步骤日志目录      |
| `audit_config`      | auditor 规则文件                   |
| `secrets_config`    | Secret Scan 规则及占位符策略文件   |
| `aliases.<id>.path` | 仓库内目录或文件路径               |
| `aliases.<id>.env`  | 可选；覆盖 alias 路径的环境变量    |

`root`、`reports`、`audit_config`、`host_port` 是保留 alias 名称。

通用覆盖变量：

| 环境变量                                    | 覆盖字段             |
| ------------------------------------------- | -------------------- |
| `REPORT_DIR` 或 `ARC_FLOW_REPORTS`          | `paths.reports`        |
| `AUDITOR_CONFIG` 或 `ARC_FLOW_AUDIT_CONFIG` | `paths.audit_config`   |
| `ARC_FLOW_SECRETS_CONFIG`                   | `paths.secrets_config` |
| `ARC_FLOW_CONFIG`                           | 配置文件路径         |

## 6. `[policy]`

```toml
[policy]
required_steps = ["api.format", "api.clippy", "api.tests"]
```

`required_steps` 中的每个 ID 都必须出现在 `[[steps]]` 中，而且不能重复。这用于防止项目基础门禁被误删。它不规定步骤属于哪个 profile；profile 仍由步骤自身配置。

## 7. `[[doctor.checks]]`

每项检查都有公共字段：

```toml
[[doctor.checks]]
id = "tool.git"
label = "git"
required = true
timeout_secs = 15
help = "install Git and ensure it is on PATH"
kind = "command"
program = "git"
args = ["--version"]
```

| 字段           | 默认值 | 说明                                    |
| -------------- | ------ | --------------------------------------- |
| `id`           | 无     | 唯一检查 ID                             |
| `label`        | 无     | 终端和 JSON 报告中的显示名称            |
| `required`     | `true` | `true` 失败计为 FAIL，`false` 计为 WARN |
| `timeout_secs` | `15`   | 单项检查硬超时，范围 1 到 300 秒        |
| `help`         | 无     | 失败时追加的修复提示                    |
| `kind`         | 无     | 检查类型                                |

支持的 kind：

| kind          | 字段                                     | 行为                                           |
| ------------- | ---------------------------------------- | ---------------------------------------------- |
| `command`     | `program`, `args`                        | 执行命令并要求退出码为 0                       |
| `path`        | `path`, `path_type`                      | 检查任意路径、文件或目录                       |
| `glob`        | `pattern`                                | 要求 glob 至少命中一个路径                     |
| `env`         | `name`                                   | 要求环境变量存在                               |
| `env-or-file` | `env`, `path`, `contains`                | 环境变量存在，或文件中有以 `contains` 开头的行 |
| `git-config`  | `key`, `expected`                        | 要求 Git 配置等于预期值                        |
| `git-remotes` | 无                                       | 检查 Git remote 配置                           |
| `version`     | `program`, `args`, `path`, `trim_prefix` | 比较命令输出与版本文件                         |
| `service`     | `service`                                | 检查 service 的外部变量或 Docker 可用性        |

示例：

```toml
[[doctor.checks]]
id = "node.version"
label = "Node version"
kind = "version"
program = "node"
args = ["--version"]
path = "{root}/.node-version"
trim_prefix = "v"

[[doctor.checks]]
id = "frontend.dependencies"
label = "frontend dependencies"
kind = "path"
path = "{web}/node_modules"
path_type = "directory"
help = "run `cd apps/web && npm ci`"

[[doctor.checks]]
id = "test.database"
label = "test database"
required = false
kind = "service"
service = "test-postgres"
```

`path_type` 可取 `any`、`file`、`directory`，默认 `any`。命令、Git 配置、remote 和 service 探测都受 `timeout_secs` 约束，超时时会终止整个子进程组。CI 中通常使用 `arc-flow doctor --strict`，把 WARN 也视为失败。

## 8. `[services.*]`

### 8.1 Environment service

适合由 CI、开发机或密钥管理系统提供现成连接值：

```toml
[services.test-redis]
kind = "environment"
source_env = "CI_REDIS_URL"
inject_env = "TEST_REDIS_URL"
```

步骤启动前读取 `source_env`，并以 `inject_env` 注入子进程。变量不存在时步骤失败。

### 8.2 Docker service

```toml
[services.test-postgres]
kind = "docker"
image = "postgres:16-alpine"
image_env = "ARC_FLOW_POSTGRES_IMAGE"
external_env = "TEST_DATABASE_URL"
inject_env = "TEST_DATABASE_URL"
external_value_policy = "isolated-postgres"
startup_timeout_secs = 30
timeout_env = "ARC_FLOW_DATABASE_TIMEOUT_SECS"
container_port = 5432
environment = { POSTGRES_USER = "test", POSTGRES_PASSWORD = "test", POSTGRES_DB = "app_test" }
healthcheck = ["pg_isready", "-U", "test", "-d", "app_test"]
connection = "postgres://test:test@127.0.0.1:{host_port}/app_test"
```

| 字段                   | 必需 | 说明                                           |
| ---------------------- | ---- | ---------------------------------------------- |
| `image`                | 是   | 本机已有的 OCI 镜像；运行时使用 `--pull=never` |
| `image_env`            | 否   | 覆盖镜像名                                     |
| `external_env`         | 否   | 若该变量已设置，直接使用其值并跳过 Docker      |
| `inject_env`           | 是   | 注入测试步骤的变量名                           |
| `external_value_policy` | 否  | 外部值安全策略；测试 PostgreSQL 使用 `isolated-postgres` |
| `startup_timeout_secs` | 是   | 整个 Docker 启动过程的秒数，范围 1 到 300      |
| `timeout_env`          | 否   | 覆盖启动超时                                   |
| `container_port`       | 是   | 容器监听端口；宿主端口随机绑定到 `127.0.0.1`   |
| `environment`          | 否   | 传给容器的环境变量                             |
| `healthcheck`          | 是   | `docker exec` 后的参数列表，不能为空           |
| `connection`           | 是   | 注入值，必须包含 `{host_port}`                 |

服务按需启动，同一轮验证内复用。Docker daemon 探测、容器创建、端口查询和健康检查共享一个启动截止时间；验证成功、失败、超时或收到中断信号时都会在独立清理超时内尝试 `docker rm --force`。镜像不会自动拉取，先用 `docker pull <image>` 准备。

`isolated-postgres` 会要求 URL 使用 `postgres`/`postgresql` 协议，数据库名以 `_test` 或
`-test` 结尾，并拒绝与当前 `DATABASE_URL` 指向同一数据库。默认只允许本机回环地址；确需
使用已确认隔离的远程测试库时，额外设置 `ARC_FLOW_ALLOW_REMOTE_TEST_DATABASE=1`。

一个步骤可以依赖多个服务：

```toml
services = ["test-postgres", "test-redis"]
remove_env = ["DATABASE_URL", "REDIS_URL"]
```

每个 service 必须注入不同变量；`remove_env` 也不能删除 service 正在注入的变量。

## 9. `[parsers.*]`

解析器用于防止命令退出码为 0、实际却没有执行任何测试：

```toml
[parsers.rust]
kind = "regex"
patterns = ['(?m)^running ([0-9]+) tests?$']
capture = 1
minimum = 1
```

| 字段       | 默认值 | 说明                          |
| ---------- | ------ | ----------------------------- |
| `kind`     | 无     | 当前支持 `regex`              |
| `patterns` | 无     | 一个或多个 Rust regex         |
| `capture`  | `1`    | 包含数值的 capture group 索引 |
| `minimum`  | `1`    | 所有匹配计数之和的最低值      |

每个正则都必须包含对应的 capture group。步骤成功后才解析日志；计数低于 `minimum` 时，该步骤改判为失败。

## 10. `[scope]` 和 `[[scope.rules]]`

```toml
[scope]
unmatched = "fail"
```

| `unmatched` 值 | 行为                                                       |
| -------------- | ---------------------------------------------------------- |
| `fail`         | 默认值；列出未命中的变更路径并失败                         |
| `all`          | 任一路径未命中时选择全部 component，适合通用或未知结构项目 |
| `ignore`       | 忽略未命中路径，仅适合已明确评估漏测风险的项目             |

`--all` 是显式全量模式，不读取工作区路径，因此不应用 `unmatched` 策略。

```toml
[[scope.rules]]
patterns = ["services/api/**", "shared/contracts/**"]
components = ["api"]

[[scope.rules]]
patterns = [".arc-flow/**", ".github/workflows/**"]
components = ["api", "web", "workflow"]
```

每条规则使用 glob 匹配仓库相对路径，命中后把所有 `components` 加入集合。规则可以重叠，最终 component 去重。每个 component 必须至少有一个步骤。`scope` 的文本和 JSON 输出都包含未匹配文件，便于排查规则覆盖缺口。

建议把共享契约、工作流配置和 CI 文件映射到所有受影响组件，避免只验证单端。

## 11. `[[steps]]`

```toml
[[steps]]
id = "api.tests"
label = "API tests"
component = "api"
profiles = ["full"]
program = "cargo"
args = ["test", "--manifest-path", "{api}/Cargo.toml", "--", "--nocapture"]
cwd = "{root}"
log = "api_tests.log"
timeout_secs = 300
timeout_env = "API_TEST_TIMEOUT"
parser = "rust"
services = ["test-postgres"]
remove_env = ["DATABASE_URL"]
```

| 字段           | 必需 | 说明                               |
| -------------- | ---- | ---------------------------------- |
| `id`           | 是   | 全局唯一步骤 ID                    |
| `label`        | 是   | 终端和报告显示名称                 |
| `component`    | 是   | 变更范围选择单位                   |
| `profiles`     | 是   | 该步骤参与的 profile，至少一个     |
| `program`      | 是   | PATH 中的裸命令名                  |
| `args`         | 是   | 独立参数数组，可使用路径占位符     |
| `cwd`          | 是   | 单个 `{root}` 或 path alias 占位符 |
| `log`          | 是   | 报告目录下的单个 `.log` 文件名     |
| `timeout_secs` | 是   | 运行超时，范围 1 到 3600 秒        |
| `timeout_env`  | 否   | 覆盖运行超时的环境变量             |
| `parser`       | 否   | 成功后使用的 parser ID             |
| `services`     | 否   | 运行前需要准备的 service ID 列表   |
| `remove_env`   | 否   | 创建子进程前删除的继承环境变量     |

命令直接通过 `program + args[]` 启动，不执行 shell 拼接。`sh -c`、`bash -lc` 等命令字符串会被配置校验拒绝；管道、重定向和条件逻辑应拆成多个步骤，或封装成项目内受版本控制的可执行程序。

步骤选择条件是：component 已被 scope 选中，并且步骤包含当前 profile。配置顺序就是执行顺序；任一步失败后，报告判为失败，但仍继续执行不依赖该故障 service 的后续步骤。同一 service 启动失败会被缓存，依赖它的步骤快速失败，不会反复等待启动超时。

## 12. Secret Scan 规则文件

`[paths].secrets_config` 指向独立、受版本控制的 TOML 文件。预设会生成一套通用高置信规则，项目可在不重新编译 `arc-flow` 的情况下增加供应商或业务密钥规则。配置版本、规则 ID、正则、捕获组和最小长度都会在扫描前校验；配置缺失、空规则或无效捕获组会直接让门禁失败。

```toml
version = 2

[placeholders]
minimum_unique_characters = 4
maximum_nonalphanumeric_characters = 2
prefixes = ["${", "{{", "<"]
markers = ["change-me", "replace-me", "placeholder"]
exact = ["password", "secret"]

[[rules]]
id = "named-signing-secret"
kind = "value"
pattern = '''(?i)signing_secret\s*=\s*([A-Za-z0-9_-]{12,})'''
capture = 1
minimum_length = 12
```

规则类型：

- `direct`：正则命中即报告，适合有固定前缀或结构的 Token、JWT、私钥头。
- `value`：只对指定捕获组执行长度、占位符和字符多样性判断，适合命名密钥及厂商 Webhook Token。
- `postgres-url`：分别捕获用户名、密码、主机和数据库；`local_test_policy` 可显式配置临时数据库允许的主机、库名后缀及用户名密码约束。
- `webhook-url`：解析捕获到的 URL，检查配置的敏感查询参数或高信息路径末段。

扫描报告只包含命中文件名，不会复制密钥内容。占位符策略只作用于捕获值；`direct` 应仅配置误报概率足够低的模式。

## 13. 审计规则文件

`[paths].audit_config` 指向独立 TOML 文件。空规则文件可写为：

```toml
version = 2

[engine]
ignore_filename = ".auditignore"
json_report_filename = "review_context.json"
markdown_report_filename = "review_context.md"
markdown_max_bytes = 4096
markdown_occurrences_per_rule = 3

[engine.comment_syntax.rs]
line = ["//"]
block = [{ start = "/*", end = "*/", nested = true }]
strings = [
  { start = 'r###"', end = '"###' },
  { start = 'r##"', end = '"##' },
  { start = 'r#"', end = '"#' },
  { start = 'r"', end = '"' },
  { start = '"', end = '"', escape = '\' },
]

[paths]
exclude = ["target", "node_modules", "dist", ".git"]
```

内置空 preset 还预置 `sql`、`ts`、`tsx`、`js`、`jsx`、`toml`、`yaml` 和 `yml` 的注释与字符串定界符。每条规则使用的扩展名都必须存在对应的 `[engine.comment_syntax.<扩展名>]`；缺少时 auditor 会拒绝运行，避免把注释示例当成真实代码。

<a id="audit-v2-migration"></a>

### Audit v2 迁移

audit v2 是 `arc-flow` 3.0.0 的破坏性配置升级。旧配置不会被静默套用新语义：缺少 `version`、缺少 `[engine]`、未知版本、未知字段或字符串 allowlist 都会 fail closed，并在错误中指向本节。

旧配置可能依赖隐式 engine 默认值，并让字符串内容同时承担路径和正则语义：

```toml
[[hard_rules]]
name = "SQL writes stay in repositories"
severity = "blocker"
paths = ["api"]
extensions = ["rs"]
patterns = ['(?i)INSERT\s+INTO']
allowlist = ["services/api/src/repositories", "^services/api/generated/.*\.rs$"]
```

迁移时先加入 `version = 2`，从当前 `empty.audit.toml` 复制完整 `[engine]` 和所需扩展名的 `comment_syntax`，再逐项明确 allowlist 类型：

```toml
version = 2

[engine]
ignore_filename = ".auditignore"
json_report_filename = "review_context.json"
markdown_report_filename = "review_context.md"
markdown_max_bytes = 4096
markdown_occurrences_per_rule = 3

[engine.comment_syntax.rs]
line = ["//"]
block = [{ start = "/*", end = "*/", nested = true }]
strings = [
  { start = 'r###"', end = '"###' },
  { start = 'r##"', end = '"##' },
  { start = 'r#"', end = '"#' },
  { start = 'r"', end = '"' },
  { start = '"', end = '"', escape = '\' },
]

[[hard_rules]]
name = "SQL writes stay in repositories"
severity = "blocker"
paths = ["api"]
extensions = ["rs"]
patterns = ['(?i)INSERT\s+INTO']
allowlist = [
  { kind = "path-prefix", path = "services/api/src/repositories" },
  { kind = "regex", pattern = '^services/api/generated/.*\.rs$' },
]
```

字符串 allowlist 无法可靠推断原意，因此不自动迁移。完成转换后运行 `arc-flow config check` 和 `arc-flow audit`，确认路径引用、正则和报告配置有效。

### 13.1 Hard rule

```toml
[[hard_rules]]
name = "SQL writes stay in repositories"
severity = "blocker"
paths = ["api"]
extensions = ["rs", "sql"]
patterns = ['(?i)INSERT\s+INTO', '\.execute\s*\(']
allowlist = [
  { kind = "path-prefix", path = "services/api/src/repositories" },
  { kind = "path-prefix", path = "services/api/migrations" },
  { kind = "path-prefix", path = "services/api/tests" },
]
exclude_patterns = []
```

`paths` 可以引用审计文件 `[paths]` 中的 alias。`allowlist` 必须显式使用 `path-prefix` 或 `regex` 类型，避免根据字符串内容猜测语义。`exclude_patterns` 用正则排除文件路径。

### 13.2 Architecture rule

```toml
[[arch_rules]]
name = "handlers do not query SQL"
layer = "handler"
paths = ["services/api/src/handlers"]
extensions = ["rs"]
forbidden_patterns = ['sqlx::(query|query_as|query_scalar)!?\\s*\\(']
allowed_patterns = []
suggestion = "move SQL into a repository"
allowlist = []
exclude_patterns = []
```

`allowed_patterns` 匹配违规起始行，适合明确的 trait impl 或框架样板；不要用过宽正则隐藏真实违规。`exclude_patterns` 与 hard rule 一样匹配文件路径。任意审计违规都会阻止后续外部步骤。

每个规则的 `paths` 必须解析到项目根内已经存在的目录。路径中的 `..`、逃出项目的绝对路径或符号链接都会被拒绝；目录遍历或文件读取失败也会让审计失败，避免扫描缺失时误报通过。审计报告统一记录仓库相对路径。

auditor 是确定性的整文件正则扫描器，不是语言 parser。正则默认启用 multi-line 模式，因此 `^`/`$` 仍按代码行匹配，`\s` 可以跨行；需要让 `.` 跨行时应在规则中显式使用 `(?s)`。报告定位到匹配起始行。`[engine.comment_syntax.<扩展名>]` 可配置行注释、块注释和字符串定界符；扫描器会跟踪这些词法状态，避免把字符串中的注释标记当成真实注释。需要抽象语法树级判断时，应使用项目语言自己的 lint 工具，并把该工具配置成一个 step。

## 14. 最小完整示例

```toml
version = 2

[project]
name = "example-api"
default_profile = "full"
hook_profile = "hook"

[paths]
reports = ".arc-flow/reports"
audit_config = ".arc-flow/audit.toml"
secrets_config = ".arc-flow/secrets.toml"

[paths.aliases.app]
path = "."

[policy]
required_steps = ["app.format", "app.tests"]

[scope]
unmatched = "all"

[[doctor.checks]]
id = "tool.cargo"
label = "cargo"
kind = "command"
program = "cargo"
args = ["--version"]

[parsers.rust]
kind = "regex"
patterns = ['(?m)^running ([0-9]+) tests?$']
capture = 1
minimum = 1

[[scope.rules]]
patterns = ["**"]
components = ["app"]

[[steps]]
id = "app.format"
label = "Rust format"
component = "app"
profiles = ["full", "hook"]
program = "cargo"
args = ["fmt", "--", "--check"]
cwd = "{app}"
log = "rust_fmt.log"
timeout_secs = 120

[[steps]]
id = "app.tests"
label = "Rust tests"
component = "app"
profiles = ["full"]
program = "cargo"
args = ["test", "--", "--nocapture"]
cwd = "{app}"
log = "rust_tests.log"
timeout_secs = 300
parser = "rust"
```

完成配置后依次执行：

```bash
arc-flow config check
arc-flow doctor
arc-flow scope --all
arc-flow verify --all
```
