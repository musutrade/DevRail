import { DEPARTMENT_ROUTE_ACCESS } from './features/departments/departments.permissions';
import { DEVRAIL_ROUTE_ACCESS } from './features/devrail/devrail.permissions';

export const ROUTE_ACCESS = {
  permissionDirectory: ['permission:directory:read'],
  users: ['user:directory:read'],
  departments: DEPARTMENT_ROUTE_ACCESS,
  roles: ['role:directory:read'],
  rolePermissions: ['role:permissions:write', 'role:directory:read', 'permission:directory:read'],
  auditLogs: ['audit:logs:read'],
  devrail: DEVRAIL_ROUTE_ACCESS,
} as const satisfies Record<string, readonly string[]>;

export interface NavigationLink {
  readonly kind: 'link';
  readonly id: string;
  readonly label: string;
  readonly icon?: string;
  readonly route: string;
  readonly permissions: readonly string[];
}

export interface NavigationGroup {
  readonly kind: 'group';
  readonly id: string;
  readonly label: string;
  readonly icon: string;
  readonly permissions: readonly string[];
  readonly children: readonly NavigationLink[];
}

export type NavigationItem = NavigationLink | NavigationGroup;

export const APP_NAVIGATION = [
  {
    kind: 'group',
    id: 'user-management',
    label: '用户管理',
    icon: 'group',
    permissions: ROUTE_ACCESS.users,
    children: [
      {
        kind: 'link',
        id: 'users',
        label: '用户列表',
        route: '/users',
        permissions: ROUTE_ACCESS.users,
      },
    ],
  },
  {
    kind: 'link',
    id: 'departments',
    label: '部门管理',
    icon: 'account_tree',
    route: '/departments',
    permissions: ROUTE_ACCESS.departments,
  },
  {
    kind: 'link',
    id: 'roles',
    label: '角色管理',
    icon: 'badge',
    route: '/roles',
    permissions: ROUTE_ACCESS.roles,
  },
  {
    kind: 'link',
    id: 'permissions',
    label: '权限目录',
    icon: 'lock_person',
    route: '/permissions',
    permissions: ROUTE_ACCESS.permissionDirectory,
  },
  {
    kind: 'link',
    id: 'role-permissions',
    label: '权限分配',
    icon: 'shield',
    route: '/role-permissions',
    permissions: ROUTE_ACCESS.rolePermissions,
  },
  {
    kind: 'link',
    id: 'audit-logs',
    label: '审计日志',
    icon: 'fact_check',
    route: '/audit-logs',
    permissions: ROUTE_ACCESS.auditLogs,
  },
  {
    kind: 'link',
    id: 'devrail-projects',
    label: 'DevRail 项目',
    icon: 'terminal',
    route: '/devrail/projects',
    permissions: ROUTE_ACCESS.devrail,
  },
  {
    kind: 'link',
    id: 'devrail-approvals',
    label: 'DevRail 审批中心',
    icon: 'approval',
    route: '/devrail/approvals',
    permissions: ROUTE_ACCESS.devrail,
  },
  {
    kind: 'link',
    id: 'devrail-notifications',
    label: 'DevRail 通知中心',
    icon: 'notifications',
    route: '/devrail/notifications',
    permissions: ROUTE_ACCESS.devrail,
  },
] as const satisfies readonly NavigationItem[];
