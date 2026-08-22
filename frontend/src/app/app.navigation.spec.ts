import { APP_NAVIGATION, NavigationItem, NavigationLink, ROUTE_ACCESS } from './app.navigation';
import { routes } from './app.routes';

function navigationLinks(): readonly NavigationLink[] {
  const links: NavigationLink[] = [];
  for (const item of APP_NAVIGATION) {
    links.push(...(item.kind === 'group' ? item.children : [item]));
  }
  return links;
}

describe('application navigation', () => {
  it('uses unique navigation ids and routes', () => {
    const items: NavigationItem[] = [];
    for (const item of APP_NAVIGATION) {
      items.push(item, ...(item.kind === 'group' ? item.children : []));
    }
    const links = navigationLinks();

    expect(new Set(items.map((item) => item.id)).size).toBe(items.length);
    expect(new Set(links.map((item) => item.route)).size).toBe(links.length);
  });

  it('shares route access requirements with every navigation entry', () => {
    const accessByRoute = new Map<string, readonly string[]>([
      ['/permissions', ROUTE_ACCESS.permissionDirectory],
      ['/users', ROUTE_ACCESS.users],
      ['/departments', ROUTE_ACCESS.departments],
      ['/roles', ROUTE_ACCESS.roles],
      ['/role-permissions', ROUTE_ACCESS.rolePermissions],
      ['/audit-logs', ROUTE_ACCESS.auditLogs],
      ['/devrail/projects', ROUTE_ACCESS.devrail],
    ]);

    for (const item of navigationLinks()) {
      expect(item.permissions).toBe(accessByRoute.get(item.route));
      expect(item.permissions.length).toBeGreaterThan(0);
    }
  });

  it('uses the navigation access requirements on protected routes', () => {
    const protectedRoutes = routes.find((route) => route.path === '')?.children ?? [];

    for (const item of navigationLinks()) {
      const route = protectedRoutes.find((candidate) => candidate.path === item.route.slice(1));
      expect(route?.data?.['permissions']).toBe(item.permissions);
    }
  });
});
