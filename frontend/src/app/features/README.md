# 业务功能目录

新业务必须放在 `features/<domain>/`，不得继续堆入 `core/` 或平台级 `pages/`。

每个业务域自行拥有页面、资源服务、领域模型和测试：

```text
features/<domain>/
├── pages/
├── data-access/
├── models/
└── <domain>.routes.ts
```

依赖方向固定为 `app 组合根 -> features -> core`：

- `core/` 只提供认证、运行时配置、拦截器、通用对话框等基础能力，禁止导入 `features/`。
- 业务 API Service 放在本业务域的 `data-access/`；`core/api/` 只保存模板自带的 RBAC 平台客户端。
- 一个业务域不得直接导入另一个业务域的页面或内部服务。确需复用时，把稳定的无业务基础能力提取到 `core/`，跨域流程放在应用组合层。
- 页面权限同时登记到 `app.navigation.ts` 和后端权限校验；前端菜单与守卫只改善体验，后端授权才是安全边界。
- 新权限从 SQL、Rust、Angular 三端[业务权限模板](../../../../docs/business-permissions.md)同步创建。

完整接入步骤见 [`docs/business-extension.md`](../../../../docs/business-extension.md)。
