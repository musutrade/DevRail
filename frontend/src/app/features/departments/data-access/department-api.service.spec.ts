import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { DepartmentResponse } from '../../../generated/api/models/department-response';
import { API_BASE_URL } from '../../../core/runtime-config';
import { DepartmentApiService } from './department-api.service';

const department: DepartmentResponse = {
  id: 2,
  organizationId: 1,
  parentId: 1,
  code: 'engineering',
  name: '研发部',
  status: 'active',
  depth: 1,
  memberCount: 4,
  childCount: 0,
  createdAt: '2026-08-01T00:00:00Z',
  updatedAt: '2026-08-01T00:00:00Z',
};

describe('DepartmentApiService', () => {
  let service: DepartmentApiService;
  let http: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: API_BASE_URL, useValue: '/api/v1' },
      ],
    });
    service = TestBed.inject(DepartmentApiService);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => http.verify());

  it('loads the visible department hierarchy', async () => {
    const result = service.list();
    const request = http.expectOne('/api/v1/departments');
    expect(request.request.method).toBe('GET');
    request.flush([department]);
    await expect(result).resolves.toEqual([department]);
  });

  it('sends the step-up token when creating a department', async () => {
    const result = service.create(
      { parentId: 1, code: 'engineering', name: '研发部', status: 'active' },
      'step-up-token',
    );
    const request = http.expectOne('/api/v1/departments');
    expect(request.request.method).toBe('POST');
    expect(request.request.headers.get('X-Step-Up-Token')).toBe('step-up-token');
    request.flush(department);
    await expect(result).resolves.toEqual(department);
  });
});
