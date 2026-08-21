import { DOCUMENT } from '@angular/common';
import { HttpClient, provideHttpClient, withInterceptors } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { authInterceptor, readCsrfToken } from './auth.interceptor';
import { AuthService } from './auth.service';
import { API_BASE_URL } from './runtime-config';

describe('authInterceptor', () => {
  let http: HttpClient;
  let controller: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(withInterceptors([authInterceptor])),
        provideHttpClientTesting(),
        { provide: API_BASE_URL, useValue: '/api/v1' },
        { provide: DOCUMENT, useValue: { cookie: 'arc_csrf=csrf-token' } },
        { provide: Router, useValue: { navigate: vi.fn() } },
        { provide: AuthService, useValue: { handleUnauthorized: vi.fn() } },
      ],
    });
    http = TestBed.inject(HttpClient);
    controller = TestBed.inject(HttpTestingController);
  });

  afterEach(() => controller.verify());

  it('sends credentials and a CSRF header on API writes', () => {
    http.post('/api/v1/users', {}).subscribe();
    const request = controller.expectOne('/api/v1/users');

    expect(request.request.withCredentials).toBe(true);
    expect(request.request.headers.get('X-CSRF-Token')).toBe('csrf-token');
    request.flush({});
  });

  it('does not send credentials to unrelated origins', () => {
    http.get('https://assets.example.test/catalog.json').subscribe();
    const request = controller.expectOne('https://assets.example.test/catalog.json');

    expect(request.request.withCredentials).toBe(false);
    expect(request.request.headers.has('X-CSRF-Token')).toBe(false);
    request.flush({});
  });

  it('prefers the production __Host CSRF cookie', () => {
    expect(readCsrfToken('arc_csrf=old; __Host-arc_csrf=current')).toBe('current');
    expect(readCsrfToken('__Host-arc_csrf=current; arc_csrf=old')).toBe('current');
  });

  it('ignores malformed encoded Cookie values', () => {
    expect(readCsrfToken('arc_csrf=%broken')).toBeNull();
  });
});
