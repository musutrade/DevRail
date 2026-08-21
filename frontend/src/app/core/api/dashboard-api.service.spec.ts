import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { API_BASE_URL } from '../runtime-config';
import { DashboardApiService } from './dashboard-api.service';

describe('DashboardApiService', () => {
  let service: DashboardApiService;
  let http: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: API_BASE_URL, useValue: '/api/v1' },
      ],
    });
    service = TestBed.inject(DashboardApiService);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => http.verify());

  it('maps dashboard statistics into cards', async () => {
    const result = service.getUserStats();
    http.expectOne('/api/v1/dashboard/stats').flush({
      totalUsers: 10,
      activeUsers: 8,
      totalRoles: 6,
      totalPermissions: 19,
      suspendedUsers: 1,
    });

    await expect(result).resolves.toEqual([
      expect.objectContaining({ label: '用户总数', value: '10' }),
      expect.objectContaining({ label: '启用用户', value: '8' }),
      expect.objectContaining({ label: '角色总数', value: '6' }),
      expect.objectContaining({ label: '已暂停用户', value: '1' }),
    ]);
  });
});
