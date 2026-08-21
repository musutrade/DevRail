import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { PermissionGroupResponse as ApiPermissionGroup } from '../../generated/api/models/permission-group-response';
import { API_BASE_URL } from '../runtime-config';
import { PermissionApiService } from './permission-api.service';

describe('PermissionApiService', () => {
  let service: PermissionApiService;
  let http: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: API_BASE_URL, useValue: '/api/v1' },
      ],
    });
    service = TestBed.inject(PermissionApiService);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => http.verify());

  it('maps permission groups and nullable fields', async () => {
    const result = service.getPermissionGroups();
    const group: ApiPermissionGroup = {
      id: 2,
      code: 'identity',
      name: '身份与访问模块',
      icon: null,
      permissions: [
        {
          id: 9,
          code: 'user:directory:read',
          name: '查看用户目录',
          type: 'menu',
          description: null,
        },
      ],
    };
    http.expectOne('/api/v1/permissions/groups').flush([group]);

    await expect(result).resolves.toEqual([
      expect.objectContaining({
        id: '2',
        code: 'identity',
        icon: 'folder',
        permissions: [expect.objectContaining({ id: '9', description: '' })],
      }),
    ]);
  });
});
