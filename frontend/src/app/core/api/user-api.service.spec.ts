import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { UserResponse as ApiUser } from '../../generated/api/models/user-response';
import { API_BASE_URL } from '../runtime-config';
import { UserApiService } from './user-api.service';

const apiUser: ApiUser = {
  id: 7,
  username: 'sarah',
  displayName: 'Sarah Chen',
  email: 'sarah@example.com',
  departmentId: 3,
  status: 'active',
  roles: ['查看者'],
  lastLoginAt: null,
  createdAt: '2026-08-01T00:00:00Z',
};

describe('UserApiService', () => {
  let service: UserApiService;
  let http: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: API_BASE_URL, useValue: '/api/v1' },
      ],
    });
    service = TestBed.inject(UserApiService);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => http.verify());

  it('maps paged API users into view users', async () => {
    const result = service.getUsers({
      page: 2,
      pageSize: 10,
      keyword: 'sarah',
      role: '查看者',
      status: 'active',
      sortBy: 'displayName',
      sortDirection: 'asc',
    });
    const request = http.expectOne(
      (candidate) => candidate.url === '/api/v1/users' && candidate.params.get('page') === '2',
    );
    expect(request.request.params.get('keyword')).toBe('sarah');
    expect(request.request.params.get('role')).toBe('查看者');
    expect(request.request.params.get('status')).toBe('active');
    expect(request.request.params.get('sortBy')).toBe('displayName');
    expect(request.request.params.get('sortDirection')).toBe('asc');
    request.flush({ items: [apiUser], total: 11, page: 2, pageSize: 10, roleOptions: ['查看者'] });

    await expect(result).resolves.toEqual(
      expect.objectContaining({
        total: 11,
        page: 2,
        roleOptions: ['查看者'],
        items: [
          expect.objectContaining({
            id: '7',
            username: 'sarah',
            name: 'Sarah Chen',
            departmentId: 3,
            lastLogin: null,
          }),
        ],
      }),
    );
  });

  it('updates a user through the existing user endpoint', async () => {
    const result = service.updateUser('7', { status: 'inactive' });
    const request = http.expectOne('/api/v1/users/7');
    expect(request.request.method).toBe('PUT');
    expect(request.request.body).toEqual({ status: 'inactive' });
    request.flush({ ...apiUser, status: 'inactive' });

    await expect(result).resolves.toEqual(expect.objectContaining({ id: '7', status: 'inactive' }));
  });

  it('sends numeric role ids when assigning user roles', async () => {
    const result = service.assignUserRoles('7', ['2', '4'], 'step-up-token');
    const request = http.expectOne('/api/v1/users/7/roles');
    expect(request.request.method).toBe('PUT');
    expect(request.request.body).toEqual({ roleIds: [2, 4] });
    expect(request.request.headers.get('X-Step-Up-Token')).toBe('step-up-token');
    request.flush(null);
    await result;
  });
});
