import { Clipboard } from '@angular/cdk/clipboard';
import { provideZonelessChangeDetection } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { AuditLogApiService } from '../../core/api/audit-log-api.service';
import { PageAuditLog } from '../../generated/api/models/page-audit-log';

import { AuditLogs } from './audit-logs';

describe('AuditLogs', () => {
  let component: AuditLogs;
  let fixture: ComponentFixture<AuditLogs>;
  let clipboard: { copy: ReturnType<typeof vi.fn> };
  const auditPage = (id = 1): PageAuditLog => ({
    items: [
      {
        id,
        actorUserId: 1,
        actorUsername: 'admin',
        action: 'user.roles.update',
        targetType: 'user',
        targetId: 7,
        details: { roleIds: [2] },
        traceId: `audit-trace-${id}`,
        createdAt: '2026-08-08T00:00:00Z',
      },
    ],
    total: 1,
    page: 1,
    pageSize: 20,
  });
  const getAuditLogs = vi.fn(() => Promise.resolve(auditPage()));

  beforeEach(async () => {
    clipboard = { copy: vi.fn(() => true) };
    getAuditLogs.mockReset().mockImplementation(() => Promise.resolve(auditPage()));
    await TestBed.configureTestingModule({
      imports: [AuditLogs],
      providers: [
        provideZonelessChangeDetection(),
        { provide: Clipboard, useValue: clipboard },
        {
          provide: AuditLogApiService,
          useValue: {
            getAuditLogs,
          },
        },
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(AuditLogs);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  it('renders localized audit event information', () => {
    expect(fixture.nativeElement.textContent).toContain('变更用户角色');
    expect(fixture.nativeElement.textContent).toContain('admin');
    expect(fixture.nativeElement.textContent).toContain('用户 #7');
    expect(fixture.nativeElement.textContent).toContain('audit-trace-1');
    expect(component.actionLabel('auth.session.revoked')).toBe('撤销认证会话');
    expect(component.actionLabel('department.create')).toBe('创建部门');
    expect(component.targetLabel({ ...auditPage().items[0], targetType: 'department' })).toBe(
      '部门 #7',
    );
  });

  it('copies the trace id for incident lookup', async () => {
    const button: HTMLButtonElement = fixture.nativeElement.querySelector('.copy-trace');
    button.click();
    await fixture.whenStable();

    expect(clipboard.copy).toHaveBeenCalledWith('audit-trace-1');
    expect(button.title).toBe('已复制');
  });

  it('ignores a stale response after a newer filter request completes', async () => {
    let resolveOlder!: (page: PageAuditLog) => void;
    let resolveNewer!: (page: PageAuditLog) => void;
    getAuditLogs
      .mockImplementationOnce(() => new Promise((resolve) => (resolveOlder = resolve)))
      .mockImplementationOnce(() => new Promise((resolve) => (resolveNewer = resolve)));

    component.search('旧条件');
    component.filterAction('user.update');
    resolveNewer(auditPage(2));
    await vi.waitFor(() => expect(component.logs()[0]?.id).toBe(2));
    resolveOlder(auditPage(3));
    await Promise.resolve();
    expect(component.logs()[0]?.id).toBe(2);
    expect(component.loading()).toBe(false);
  });

  it('uses the server cursor when loading the next page', async () => {
    component.total.set(40);
    component.nextCursor.set('1723075200000000.1');

    component.goToPage(2);
    await vi.waitFor(() =>
      expect(getAuditLogs).toHaveBeenLastCalledWith(2, 20, '', '', '1723075200000000.1'),
    );
  });
});
