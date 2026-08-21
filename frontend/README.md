# arc-admin 前端

Angular 22.1 + Angular Material 22.1 管理端。应用使用 standalone components、signals、zoneless 变更检测和 `OnPush`，通过真实 Axum API 完成认证、RBAC 管理与安全设置。

## 启动与验证

```bash
npm ci
npm start              # http://localhost:4200
npm run lint
npm run format:check
npm run test:ci        # Vitest + jsdom
npm run e2e            # Desktop Chrome + Pixel 7
npm run build          # dist/arc-admin
```

开发服务器通过 `proxy.conf.json` 把 `/api/v1` 转发到 `http://127.0.0.1:8080`。完整全栈流程使用仓库根的 `cargo flow verify --all`；它会准备隔离的 PostgreSQL，并额外运行真实 Angular/Axum 跨端冒烟测试。

需要保存视觉审计截图时执行：

```bash
VISUAL_REVIEW=1 npm run e2e -- --project=chromium --project=mobile-chromium
```

截图写入 `test-results/visual-review/`，只用于本地复核，不提交到仓库。

## 认证与权限

- `AuthService` 使用服务端 HttpOnly Cookie 会话；浏览器脚本不读取或保存认证凭据。
- 登录请求中的“记住我”只决定服务端会话期限，不会把 token 写入 Local Storage 或 Session Storage。
- 写请求由 `authInterceptor` 从非 HttpOnly CSRF Cookie 读取随机值，并附加 `X-CSRF-Token`；401 响应会清理前端会话状态并返回登录页。
- 路由守卫通过 `/auth/me` 复核会话，并从 `/auth/me/permissions` 获取权限码。
- 路由守卫、侧栏和按钮共用 `app.navigation.ts` 与对应权限常量；后端授权始终是最终安全边界。
- 高风险操作先取得短时、限定用途的 step-up token；该 token 只随当前请求发送，不作为登录会话保存。
- `core/api` 按平台资源映射生成的 OpenAPI DTO；新业务的数据访问放在自己的 `features/<domain>/data-access`。

## 页面

| 路由                   | 功能                                               |
| ---------------------- | -------------------------------------------------- |
| `/login`               | 密码登录、首次 TOTP 注册、TOTP/通行密钥/恢复码验证 |
| `/permissions`         | 权限分组、搜索、类型筛选                           |
| `/users`               | 用户增删改、密码重置、角色分配、筛选与分页         |
| `/departments`         | 层级部门、状态筛选、统计摘要                       |
| `/roles`               | 角色增删改、数据范围与成员统计                     |
| `/role-permissions`    | 角色权限矩阵与授权对话框                           |
| `/audit-logs`          | 操作审计、追踪号搜索与复制，移动端卡片视图         |
| `/security`            | MFA 状态、通行密钥与恢复码管理                     |
| `/403`、`/404`、`/500` | 统一错误状态与恢复入口                             |

除公开登录和错误页外，页面都位于主布局内。平台页面使用懒加载 `loadComponent`；应用默认落到权限目录。

## 目录边界

```text
src/app/
├── app.navigation.ts         # 路由和导航共用的权限要求
├── app.routes.ts             # 应用组合根
├── core/                     # 认证、运行时配置、拦截器和平台 API
├── features/<domain>/        # 新业务域：页面、模型、数据访问、测试
├── generated/api/            # OpenAPI 生成产物，禁止手改
├── layout/                   # 侧栏、顶栏、移动导航和主题切换
└── pages/                    # 内置 RBAC、安全和错误页面
```

依赖方向固定为 `app 组合根 -> features -> core`。新业务页面不得继续加入 `pages/`；详细规则见 [业务模块扩展指南](../docs/business-extension.md)。

## 主题与样式

- `src/styles/_tokens.scss` 定义字体、4px 间距阶梯、圆角、控制高度、动效、亮暗颜色和 Material M3 映射。
- `src/styles.scss` 装配主题并维护跨页面共享模式，例如页面头、按钮、过滤栏、表格、状态标签、空状态、统计卡与分页。
- 页面 SCSS 只维护该页面独有的布局或内容，不复制共享组件，也不直接声明业务色值。
- 主题选择保存在运行时配置的 `themeStorageKey` 下；默认是 `arc-admin-theme`。仅主题偏好使用 Local Storage。
- 所有可交互元素保留清晰的 `:focus-visible`，动效遵守 `prefers-reduced-motion`，强制颜色模式保留结构边界。

页面实现与审查以 [UI 与 CSS 规范](../docs/ui-design-system.md) 为准。

## API 生成

`../backend/src/openapi.rs` 与 Rust DTO 是契约源。修改接口后在本目录执行：

```bash
npm run generate:api:all
```

该命令重新生成 `../docs/openapi.json` 与 `src/app/generated/api/`。生成文件漂移会被 `cargo flow verify` 阻断。

## 运行时配置

`public/config.js` 提供公开的产品名称、简称、slug、API 根路径和主题存储键。它由浏览器直接读取，不得包含任何密钥或认证凭据。
