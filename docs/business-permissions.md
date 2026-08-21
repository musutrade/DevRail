# 业务权限模板

业务权限必须同时落到数据库目录、后端 API 授权和前端交互控制。仓库提供三份同步模板：

| 模板                            | 目标文件                                                                     | 作用                                      |
| ------------------------------- | ---------------------------------------------------------------------------- | ----------------------------------------- |
| `business_permissions.sql.tmpl` | `backend/migrations/<UTC_TIMESTAMP>_add_<domain>_<resource>_permissions.sql` | 创建权限组、权限目录和默认角色授权        |
| `rust_permissions.rs.tmpl`      | `backend/src/permissions/<resource>.rs`                                      | 声明 `RequirePermission` 使用的类型化权限 |
| `angular_permissions.ts.tmpl`   | `frontend/src/app/features/<domain>/<resource>.permissions.ts`               | 声明路由、导航和按钮共用的权限常量        |

模板位于 `codex-audit-pipeline/.codex/templates/`。权限码统一采用：

```text
<permission-prefix>:<resource>:<action>
```

三个片段只能使用小写字母、数字和下划线，并以小写字母开头。`permission-prefix` 使用 `.arc-project.json` 中项目初始化时保存的 `project.permissionPrefix`，禁止 `*` 等通配符权限。

## 替换字段

以股票行情为例：

| 占位符                      | 示例值                     |
| --------------------------- | -------------------------- |
| `{{PERMISSION_PREFIX}}`     | `stock`                    |
| `{{GROUP_NAME}}`            | `股票分析`                 |
| `{{GROUP_ICON}}`            | `monitoring`               |
| `{{RESOURCE_NAME}}`         | `quote`                    |
| `{{RESOURCE_LABEL}}`        | `股票行情`                 |
| `{{PERMISSION_MODULE}}`     | `stock_quote`              |
| `{{READ_PERMISSION_TYPE}}`  | `StockQuoteRead`           |
| `{{WRITE_PERMISSION_TYPE}}` | `StockQuoteWrite`          |
| `{{PERMISSION_CONST}}`      | `STOCK_QUOTE_PERMISSIONS`  |
| `{{ROUTE_ACCESS_CONST}}`    | `STOCK_QUOTE_ROUTE_ACCESS` |

替换后会得到 `stock:quote:read` 和 `stock:quote:write`。模板中的 `read` 是页面及查询权限，`write` 是创建、修改和删除权限；风险差异明显时必须继续拆分动作，不能让一个宽泛的 `write` 覆盖审批或资金操作。

## 接入顺序

1. 复制 SQL 模板并以 UTC 时间戳命名 migration，例如 `20260808000100_add_stock_quote_permissions.sql`；只能新增 migration，已应用文件不可修改。
2. 复制 Rust 模板，并在 `backend/src/permissions.rs` 增加 `pub mod <permission_module>;`。
3. Handler 的读接口使用 `RequirePermission<...Read>`，写接口使用 `RequirePermission<...Write>`。CRUD Handler 模板已默认采用此方式。
4. 复制 Angular 模板，路由 `data.permissions` 和导航项引用同一个 `ROUTE_ACCESS` 常量。
5. 按钮使用 `AuthService.hasPermission(PERMISSIONS.write)` 控制显示，但后端仍必须重复校验。
6. 更新 Rust DTO 与 `backend/src/openapi.rs`，运行 `npm run generate:api:all`，再补业务测试并执行 `cargo flow verify --all`。

SQL 模板只自动把新权限授予 `super_admin`。其他角色应在权限分配页面显式授权；若产品必须提供内置业务角色，应在同一个 migration 中按明确角色代码列出权限，禁止按前缀批量授权。

## 模板质量门禁

`codex-audit-pipeline/.codex/templates/manifest.json` 是业务模板的唯一登记清单。新增、删除或重命名 `.tmpl` 文件时必须同步更新清单，并为每个占位符提供示例值。

门禁会检查模板是否完整登记、占位符与示例值是否一致、是否残留冲突标记或禁用模式，并分别使用 TypeScript 编译器、`rustfmt` 和 SQL 结构扫描器验证渲染结果。

本地可以单独执行：

```bash
node scripts/check-templates.mjs
node --test scripts/check-templates.test.mjs
cargo flow verify --components workflow
```

该门禁已加入 `hook` 和 `full` 验证流程，提交钩子与交付前完整验证都会执行。

## 自定义动作

审批、导出、转账等动作应使用独立权限，例如：

```text
oa:approval:approve
stock:report:export
token:transfer:approve
```

增加自定义动作时，必须同时：

1. 在 SQL migration 增加权限目录记录。页面入口用 `menu`，纯按钮动作用 `button`，仅 API 能力用 `api`。
2. 在 Rust 权限模块增加 `PermissionRequirement` 标记，并让对应 Handler 使用。
3. 在 Angular 权限对象增加同码常量并控制交互。
4. 增加“无权限返回 403”和“有权限成功”的后端测试。

前端守卫和按钮隐藏不是安全边界。即使业务页面没有前端入口，所有受保护 API 仍必须使用 `RequirePermission` 或在已验证的权限上下文中调用 `require`。
