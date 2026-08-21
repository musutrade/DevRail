import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { API_BASE_URL } from './runtime-config';
import { LoginResponse } from '../generated/api/models/login-response';
import { UserResponse as ApiUser } from '../generated/api/models/user-response';
import { AuthService } from './auth.service';

const user: ApiUser = {
  id: 1,
  username: 'admin',
  displayName: 'Administrator',
  email: 'admin@example.test',
  departmentId: 1,
  status: 'active',
  roles: ['Super Admin'],
  lastLoginAt: null,
  createdAt: '2026-08-01T00:00:00Z',
};

const loginResponse: LoginResponse = {
  status: 'authenticated',
  expiresAt: '2026-08-01T08:00:00Z',
  user,
  methods: [],
  recoveryCodes: [],
};

describe('AuthService', () => {
  let service: AuthService;
  let http: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: API_BASE_URL, useValue: '/api/v1' },
      ],
    });
    service = TestBed.inject(AuthService);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => http.verify());

  it('creates a server session and loads permissions after login', async () => {
    const result = service.login('admin', 'secure-password', true);
    const loginRequest = http.expectOne('/api/v1/auth/login');
    expect(loginRequest.request.body).toEqual({
      username: 'admin',
      password: 'secure-password',
      remember: true,
    });
    loginRequest.flush(loginResponse);
    let permissionRequest: ReturnType<HttpTestingController['expectOne']> | undefined;
    await vi.waitFor(() => {
      const requests = http.match('/api/v1/auth/me/permissions');
      expect(requests).toHaveLength(1);
      permissionRequest = requests[0];
    });
    permissionRequest?.flush({ codes: ['user:directory:read'] });
    await result;

    expect(service.currentUser()).toEqual(user);
    expect(service.hasPermission('user:directory:read')).toBe(true);
  });

  it('does not create in-memory session state before MFA succeeds', async () => {
    const result = service.login('admin', 'secure-password', false);
    http.expectOne('/api/v1/auth/login').flush({
      status: 'mfaRequired',
      challengeToken: 'challenge-token',
      methods: ['totp', 'recoveryCode'],
      recoveryCodes: [],
    } satisfies LoginResponse);

    await expect(result).resolves.toMatchObject({ status: 'mfaRequired' });
    http.expectNone('/api/v1/auth/me/permissions');
    expect(service.currentUser()).toBeNull();
  });

  it('revokes a partial server session when permission loading fails', async () => {
    const result = service.login('admin', 'secure-password', false);
    http.expectOne('/api/v1/auth/login').flush(loginResponse);
    let permissionRequest: ReturnType<HttpTestingController['expectOne']> | undefined;
    await vi.waitFor(() => {
      const requests = http.match('/api/v1/auth/me/permissions');
      expect(requests).toHaveLength(1);
      permissionRequest = requests[0];
    });
    permissionRequest?.flush({ message: 'failed' }, { status: 500, statusText: 'Server Error' });
    let logoutRequest: ReturnType<HttpTestingController['expectOne']> | undefined;
    await vi.waitFor(() => {
      const requests = http.match('/api/v1/auth/logout');
      expect(requests).toHaveLength(1);
      logoutRequest = requests[0];
    });
    logoutRequest?.flush(null, { status: 204, statusText: 'No Content' });

    await expect(result).rejects.toBeTruthy();
    expect(service.currentUser()).toBeNull();
    expect(service.permissions().size).toBe(0);
  });

  it('checks an existing HttpOnly Cookie session on application startup', async () => {
    const result = service.ensureSession();
    http.expectOne('/api/v1/auth/me').flush(user);
    http.expectOne('/api/v1/auth/me/permissions').flush({ codes: ['dashboard:analytics:read'] });

    await expect(result).resolves.toBe(true);
    expect(service.currentUser()).toEqual(user);
  });

  it('coalesces concurrent permission refreshes', async () => {
    const first = service.refreshPermissions();
    const second = service.refreshPermissions();
    const requests = http.match('/api/v1/auth/me/permissions');

    expect(requests).toHaveLength(1);
    requests[0].flush({ codes: ['user:directory:read'] });
    await Promise.all([first, second]);
    expect(service.hasPermission('user:directory:read')).toBe(true);
  });

  it('changes the password and clears in-memory session state', async () => {
    const request = {
      currentPassword: 'current-password',
      newPassword: 'updated-password',
    };
    const result = service.changePassword(request, 'step-up-token');
    const apiRequest = http.expectOne('/api/v1/auth/me/password');

    expect(apiRequest.request.method).toBe('PUT');
    expect(apiRequest.request.body).toEqual(request);
    expect(apiRequest.request.headers.get('X-Step-Up-Token')).toBe('step-up-token');
    apiRequest.flush(null, { status: 204, statusText: 'No Content' });
    await result;
    expect(service.currentUser()).toBeNull();
  });

  it('issues a scoped step-up token with the current credentials', async () => {
    const result = service.issueStepUp('users.delete', 'current-password', '123456');
    const request = http.expectOne('/api/v1/auth/me/step-up');

    expect(request.request.method).toBe('POST');
    expect(request.request.body).toEqual({
      scope: 'users.delete',
      currentPassword: 'current-password',
      totpCode: '123456',
    });
    request.flush({ token: 'step-up-token', expiresAt: '2026-08-01T00:05:00Z' });

    await expect(result).resolves.toEqual(expect.objectContaining({ token: 'step-up-token' }));
  });
});
