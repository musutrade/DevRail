# 业务模块扩展指南

## 边界目标

模板内置认证、RBAC、审计、运行时产品配置和开发工作流。股票分析、OA、Token 流转等业务应作为独立业务域接入，不修改认证协议，也不把业务状态塞进 RBAC 平台模块。

前端依赖方向：

```text
app.routes.ts / app.navigation.ts
              |
              v
      features/<domain>
              |
              v
             core
```

后端调用方向：

```text
Router -> Handler -> Service -> Repository -> PostgreSQL
```

`core` 不依赖 `features`，Handler 不直连 Repository，Repository 不依赖 Handler 或 Service；这些方向由 `audit.toml` 自动检查。

## 新建前端业务域

以股票分析为例，从仓库根执行：

```bash
cd frontend
npx ng generate component features/stock/pages/stock-overview
npx ng generate service features/stock/data-access/stock-api \
  --injectable --type service --add-type-to-class-name
```

推荐结构：

```text
frontend/src/app/features/stock/
├── data-access/stock-api.service.ts
├── models/stock.models.ts
├── pages/stock-overview/
├── stock.routes.ts
└── *.spec.ts
```

业务域内部负责 DTO 映射、页面状态和业务交互。通用认证、`API_BASE_URL`、错误转换等从 `core` 使用；API DTO 由 Rust 契约生成到 `frontend/src/app/generated/api`，不要在前端重复声明 `StockQuote`、审批单或 Token 流转状态字段。

在 `app.navigation.ts` 增加权限要求和中文导航项，再由 `app.routes.ts` 懒加载业务路由。两处必须引用同一个 `ROUTE_ACCESS` 成员，受保护路由未声明权限时守卫会拒绝访问。

## 新建后端业务域

沿用现有扁平分层，为同一业务名分别增加：

```text
backend/src/handlers/stock.rs
backend/src/services/stock.rs
backend/src/repositories/stock.rs
```

同时完成以下接入：

1. 在各层 `mod.rs` 注册模块，并在 `backend/src/lib.rs` 组合路由。
2. Handler 只解析请求、声明权限并映射 HTTP 结果。
3. Service 校验业务规则并管理事务。
4. Repository 承担全部 SQL；schema 变化只新增 migration，不修改历史 migration。
5. 更新 Rust DTO 与 `backend/src/openapi.rs`，运行 `npm run generate:api:all`，再补契约测试和业务测试。

每张业务主表必须包含以下归属列，并使用外键保证部门与组织一致：

```sql
organization_id BIGINT NOT NULL REFERENCES organizations (id),
department_id   BIGINT,
owner_user_id   BIGINT NOT NULL REFERENCES users (id)
```

CRUD 模板要求 `ActorContext` 从 Handler 传到 Repository，并在查询、更新和删除 SQL 中同时应用角色数据范围。`all` 才能跨组织；其他范围都先限定 `organization_id`，再分别限定组织、部门树、当前部门或 `owner_user_id`。禁止在 Service 或前端对未过滤结果做二次筛选。

权限码使用项目初始化时的业务前缀，例如 `stock:quote:read`、`oa:approval:write`、`token:transfer:approve`。数据库、Rust 和 Angular 应从同一套[业务权限模板](business-permissions.md)开始，菜单权限、按钮权限和 API 权限按实际动作拆分，后端每个受保护接口都必须校验对应权限。

## 跨业务域协作

- 前端业务域之间不直接引用页面、组件内部状态或私有数据服务。
- 后端跨域流程由 Service 编排；不要通过另一个域的 Handler 调用业务。
- 仅当类型或工具稳定、无业务归属且至少被两个域复用时，才提取为共享基础能力。
- 暂不拆微服务或 Rust workspace crate。先保持单体内清晰分层，只有独立部署、扩缩容或团队所有权成为真实需求时再拆分。

## 完成清单

1. 新增带组织、部门和所有者列的 migration、Repository、Service、Handler 和 OpenAPI 契约。
2. 从业务权限模板创建权限 migration、Rust 标记和 Angular 常量，并用后端提取器保护接口。
3. 新增 `features/<domain>` 页面、资源服务、模型与测试。
4. 在共享权限表、导航和路由中注册入口。
5. 执行 `cargo flow scope`、`cargo flow verify` 和交付前的 `cargo flow verify --all`。
