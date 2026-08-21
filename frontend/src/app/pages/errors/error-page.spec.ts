import { Component, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Router, provideRouter } from '@angular/router';
import { RouterTestingHarness } from '@angular/router/testing';
import { ErrorPage } from './error-page';

@Component({ template: '' })
class PermissionsStub {}

describe('ErrorPage', () => {
  let harness: RouterTestingHarness;

  beforeEach(async () => {
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        provideRouter([
          { path: '403', component: ErrorPage, data: { status: 403 } },
          { path: '404', component: ErrorPage, data: { status: 404 } },
          { path: '500', component: ErrorPage, data: { status: 500 } },
          { path: 'error', component: ErrorPage },
          { path: 'permissions', component: PermissionsStub },
        ]),
      ],
    });
    harness = await RouterTestingHarness.create();
  });

  it.each([
    ['/403', 403, '无权访问'],
    ['/404', 404, '页面不存在'],
    ['/500', 500, '服务器内部错误'],
  ] as const)('renders %s from route data', async (path, status, title) => {
    const page = await harness.navigateByUrl(path, ErrorPage);

    expect(page.status).toBe(status);
    expect(harness.routeNativeElement?.textContent).toContain(title);
  });

  it('falls back to the not-found view when route data is missing', async () => {
    const page = await harness.navigateByUrl('/error', ErrorPage);

    expect(page.status).toBe(404);
    expect(harness.routeNativeElement?.textContent).toContain('页面不存在');
  });

  it('navigates back to the application home page', async () => {
    await harness.navigateByUrl('/403', ErrorPage);
    const button = harness.routeNativeElement?.querySelector<HTMLButtonElement>('.btn-primary');

    button?.click();
    await harness.fixture.whenStable();

    expect(TestBed.inject(Router).url).toBe('/permissions');
  });
});
