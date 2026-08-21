import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import {
  ActivatedRouteSnapshot,
  Router,
  RouterStateSnapshot,
  UrlTree,
  provideRouter,
} from '@angular/router';
import { authGuard, permissionGuard, routes } from './app.routes';
import { ROUTE_ACCESS } from './app.navigation';
import { AuthService } from './core/auth.service';

function makeRoute(path: string, permissions?: readonly string[]): ActivatedRouteSnapshot {
  return {
    routeConfig: { path },
    data: permissions ? { permissions } : {},
  } as unknown as ActivatedRouteSnapshot;
}

describe('route guards', () => {
  const auth = {
    ensureSession: vi.fn(() => Promise.resolve(false)),
    hasAllPermissions: vi.fn(() => false),
  };

  beforeEach(() => {
    auth.ensureSession.mockReset().mockResolvedValue(false);
    auth.hasAllPermissions.mockReset().mockReturnValue(false);
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        provideRouter([]),
        { provide: AuthService, useValue: auth },
      ],
    });
  });

  function runAuth(path: string): unknown {
    return TestBed.runInInjectionContext(() =>
      authGuard(makeRoute(path), {} as RouterStateSnapshot),
    );
  }

  function runPermissions(permissions?: readonly string[]): unknown {
    return TestBed.runInInjectionContext(() =>
      permissionGuard(makeRoute('protected', permissions), {} as RouterStateSnapshot),
    );
  }

  it('redirects to login when the server session is invalid', async () => {
    const result = await runAuth('users');
    const router = TestBed.inject(Router);
    expect(result instanceof UrlTree).toBe(true);
    expect(router.serializeUrl(result as UrlTree)).toBe('/login');
  });

  it('allows an authenticated session', async () => {
    auth.ensureSession.mockResolvedValue(true);
    await expect(runAuth('users')).resolves.toBe(true);
  });

  it('always allows public error pages', () => {
    expect(runAuth('403')).toBe(true);
    expect(runAuth('404')).toBe(true);
    expect(runAuth('500')).toBe(true);
  });

  it('redirects authenticated users without the required permission', async () => {
    auth.ensureSession.mockResolvedValue(true);
    const result = await runPermissions(ROUTE_ACCESS.users);
    const router = TestBed.inject(Router);
    expect(router.serializeUrl(result as UrlTree)).toBe('/403');
  });

  it('allows authenticated users with the required permission', async () => {
    auth.ensureSession.mockResolvedValue(true);
    auth.hasAllPermissions.mockReturnValue(true);
    await expect(runPermissions(ROUTE_ACCESS.users)).resolves.toBe(true);
  });

  it('requires every dependency for the role permission page', async () => {
    auth.ensureSession.mockResolvedValue(true);
    const required = ROUTE_ACCESS.rolePermissions;
    const router = TestBed.inject(Router);
    const denied = await runPermissions(required);
    expect(router.serializeUrl(denied as UrlTree)).toBe('/403');

    auth.hasAllPermissions.mockReturnValue(true);
    await expect(runPermissions(required)).resolves.toBe(true);
  });

  it('fails closed when a protected route has no permission declaration', async () => {
    auth.ensureSession.mockResolvedValue(true);
    const router = TestBed.inject(Router);
    const denied = await runPermissions();
    expect(router.serializeUrl(denied as UrlTree)).toBe('/403');
  });

  it('does not run the authentication guard twice on permission-protected routes', () => {
    const protectedRoutes = routes
      .flatMap((route) => route.children ?? [])
      .filter((route) => Array.isArray(route.data?.['permissions']));

    expect(protectedRoutes.length).toBeGreaterThan(0);
    expect(protectedRoutes.every((route) => route.canActivate?.length === 1)).toBe(true);
    expect(protectedRoutes.every((route) => route.canActivate?.[0] === permissionGuard)).toBe(true);
  });
});
