import { inject } from '@angular/core';
import { CanActivateFn, Router, Routes } from '@angular/router';
import { LayoutComponent } from './layout/layout';
import { LoginPage } from './pages/login/login';
import { AuthService } from './core/auth.service';
import { ROUTE_ACCESS } from './app.navigation';

/** 无需登录即可访问的错误页 */
const PUBLIC_PATHS = new Set(['403', '404', '500']);

const loadErrorPage = () => import('./pages/errors/error-page').then((m) => m.ErrorPage);

/** 未登录跳转到 /login;错误页始终放行 */
export const authGuard: CanActivateFn = (route) => {
  const router = inject(Router);
  const auth = inject(AuthService);
  const path = route.routeConfig?.path ?? '';
  if (PUBLIC_PATHS.has(path)) {
    return true;
  }
  return auth
    .ensureSession()
    .then((authenticated) => (authenticated ? true : router.createUrlTree(['/login'])));
};

export const permissionGuard: CanActivateFn = (route) => {
  const router = inject(Router);
  const auth = inject(AuthService);
  const permissions = route.data['permissions'] as readonly string[] | undefined;
  return auth.ensureSession().then((authenticated) => {
    if (!authenticated) {
      return router.createUrlTree(['/login']);
    }
    const allowed = Boolean(permissions?.length) && auth.hasAllPermissions(permissions ?? []);
    return allowed ? true : router.createUrlTree(['/403']);
  });
};

export const routes: Routes = [
  // 主落地页保持 eager,其余页面懒加载
  { path: 'login', component: LoginPage },
  {
    path: '',
    component: LayoutComponent,
    children: [
      { path: '', redirectTo: 'permissions', pathMatch: 'full' },
      {
        path: 'permissions',
        canActivate: [permissionGuard],
        data: { permissions: ROUTE_ACCESS.permissionDirectory },
        loadComponent: () =>
          import('./pages/permissions/permissions').then((m) => m.PermissionsPage),
      },
      {
        path: 'users',
        canActivate: [permissionGuard],
        data: { permissions: ROUTE_ACCESS.users },
        loadComponent: () => import('./pages/users/users').then((m) => m.UsersPage),
      },
      {
        path: 'departments',
        canActivate: [permissionGuard],
        data: { permissions: ROUTE_ACCESS.departments },
        loadComponent: () =>
          import('./pages/departments/departments').then((m) => m.DepartmentsPage),
      },
      {
        path: 'roles',
        canActivate: [permissionGuard],
        data: { permissions: ROUTE_ACCESS.roles },
        loadComponent: () => import('./pages/roles/roles').then((m) => m.RolesPage),
      },
      {
        path: 'role-permissions',
        canActivate: [permissionGuard],
        data: { permissions: ROUTE_ACCESS.rolePermissions },
        loadComponent: () =>
          import('./pages/role-permissions/role-permissions').then((m) => m.RolePermissionsPage),
      },
      {
        path: 'audit-logs',
        canActivate: [permissionGuard],
        data: { permissions: ROUTE_ACCESS.auditLogs },
        loadComponent: () => import('./pages/audit-logs/audit-logs').then((m) => m.AuditLogs),
      },
      {
        path: 'devrail/projects',
        canActivate: [permissionGuard],
        data: { permissions: ROUTE_ACCESS.devrail },
        loadComponent: () => import('./pages/devrail/devrail').then((m) => m.DevRailPage),
      },
      {
        path: 'security',
        canActivate: [authGuard],
        loadComponent: () => import('./pages/security/security').then((m) => m.SecurityPage),
      },
      {
        path: '403',
        title: '403 - 无权访问',
        data: { status: 403 },
        loadComponent: loadErrorPage,
      },
      {
        path: '404',
        title: '404 - 页面不存在',
        data: { status: 404 },
        loadComponent: loadErrorPage,
      },
      {
        path: '500',
        title: '500 - 服务器内部错误',
        data: { status: 500 },
        loadComponent: loadErrorPage,
      },
    ],
  },
  { path: '**', redirectTo: '404' },
];
