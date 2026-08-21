import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { RoleResponse as ApiRole } from '../../generated/api/models/role-response';
import { API_BASE_URL } from '../runtime-config';
import { RoleApiService } from './role-api.service';

const apiRole: ApiRole = {
  id: 3,
  code: 'viewer',
  name: '查看者',
  category: '只读',
  icon: null,
  color: 'success',
  description: null,
  dataScope: 'self',
  isActive: true,
  members: 4,
  permissionGroupIds: [1],
};

describe('RoleApiService', () => {
  let service: RoleApiService;
  let http: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: API_BASE_URL, useValue: '/api/v1' },
      ],
    });
    service = TestBed.inject(RoleApiService);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => http.verify());

  it('maps roles and role permission rows', async () => {
    const roles = service.getRoles();
    http.expectOne('/api/v1/roles').flush([apiRole]);
    await expect(roles).resolves.toEqual([
      expect.objectContaining({ id: '3', code: 'viewer', dataScope: 'self', isActive: true }),
    ]);

    const rows = service.getRolePermissionRows();
    http.expectOne('/api/v1/roles').flush([apiRole]);
    await expect(rows).resolves.toEqual([
      expect.objectContaining({ roleId: '3', roleName: '查看者', usersAssigned: 4 }),
    ]);
  });

  it('updates a role through the existing role endpoint', async () => {
    const result = service.updateRole('3', { isActive: false });
    const request = http.expectOne('/api/v1/roles/3');
    expect(request.request.method).toBe('PUT');
    expect(request.request.body).toEqual({ isActive: false });
    request.flush({ ...apiRole, isActive: false });

    await expect(result).resolves.toEqual(expect.objectContaining({ id: '3', isActive: false }));
  });

  it('sends numeric permission ids when assigning a role', async () => {
    const result = service.assignRolePermissions('3', ['7', '9'], 'step-up-token');
    const request = http.expectOne('/api/v1/roles/3/permissions');
    expect(request.request.method).toBe('PUT');
    expect(request.request.body).toEqual({ permissionIds: [7, 9] });
    expect(request.request.headers.get('X-Step-Up-Token')).toBe('step-up-token');
    request.flush(null);
    await result;
  });
});
