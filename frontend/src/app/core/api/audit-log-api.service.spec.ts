import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { API_BASE_URL } from '../runtime-config';
import { AuditLogApiService } from './audit-log-api.service';

describe('AuditLogApiService', () => {
  let service: AuditLogApiService;
  let http: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: API_BASE_URL, useValue: '/api/v1' },
      ],
    });
    service = TestBed.inject(AuditLogApiService);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => http.verify());

  it('loads filtered audit logs with contract pagination', async () => {
    const result = service.getAuditLogs(2, 20, ' admin ', 'user.update', 'cursor-token');
    const request = http.expectOne(
      (candidate) =>
        candidate.url === '/api/v1/audit-logs' &&
        candidate.params.get('page') === '2' &&
        candidate.params.get('keyword') === 'admin' &&
        candidate.params.get('action') === 'user.update' &&
        candidate.params.get('cursor') === 'cursor-token',
    );
    request.flush({ items: [], total: 0, page: 2, pageSize: 20 });
    await expect(result).resolves.toEqual({ items: [], total: 0, page: 2, pageSize: 20 });
  });
});
