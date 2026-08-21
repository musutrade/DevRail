import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { DashboardApiService } from '../../core/api/dashboard-api.service';
import { DepartmentApiService } from '../../features/departments/data-access/department-api.service';
import { RoleApiService } from '../../core/api/role-api.service';
import { UserApiService } from '../../core/api/user-api.service';
import { AuthService } from '../../core/auth.service';
import { User } from '../../core/models';
import { vi } from 'vitest';

const USERS: User[] = Array.from({ length: 12 }, (_, index) => ({
  id: String(index + 1),
  username: index === 0 ? 'sarah' : `user${index + 1}`,
  name: index === 0 ? 'Sarah Jenkins' : `User ${index + 1}`,
  email: index === 0 ? 'sarah@example.com' : `user${index + 1}@example.com`,
  departmentId: 1,
  roles: [index === 0 ? 'Auditor' : index % 2 === 0 ? 'Viewer' : 'Editor'],
  status: index === 1 ? 'suspended' : index % 3 === 0 ? 'inactive' : 'active',
  lastLogin: '2026-08-01T00:00:00Z',
  createdAt: '2026-01-01T00:00:00Z',
  avatarColor: '#165dff',
}));
import { UsersPage } from './users';

const getUsers = vi.fn(async (query: Parameters<UserApiService['getUsers']>[0]) => {
  const keyword = query.keyword?.toLowerCase();
  const filtered = USERS.filter(
    (user) =>
      (!keyword ||
        user.username.toLowerCase().includes(keyword) ||
        user.name.toLowerCase().includes(keyword) ||
        user.email.toLowerCase().includes(keyword)) &&
      (!query.role || user.roles.includes(query.role)) &&
      (!query.status || user.status === query.status),
  );
  const start = (query.page - 1) * query.pageSize;
  return {
    items: filtered.slice(start, start + query.pageSize),
    total: filtered.length,
    page: query.page,
    pageSize: query.pageSize,
    roleOptions: ['Auditor', 'Editor', 'Viewer'],
  };
});
const userApiStub: Partial<UserApiService> = { getUsers };
const getUserStats = vi.fn(() => Promise.resolve([]));
const dashboardApiStub: Partial<DashboardApiService> = {
  getUserStats,
};
const authServiceStub: Partial<AuthService> = {
  hasPermission: (code) => code === 'dashboard:analytics:read',
  currentUser: signal(null),
};

describe('UsersPage', () => {
  let fixture: ComponentFixture<UsersPage>;
  let page: UsersPage;

  beforeEach(async () => {
    getUsers.mockClear();
    getUserStats.mockClear();
    await TestBed.configureTestingModule({
      imports: [UsersPage],
      providers: [
        provideZonelessChangeDetection(),
        { provide: UserApiService, useValue: userApiStub },
        { provide: DashboardApiService, useValue: dashboardApiStub },
        { provide: RoleApiService, useValue: {} },
        { provide: DepartmentApiService, useValue: { list: vi.fn(async () => []) } },
        { provide: AuthService, useValue: authServiceStub },
      ],
    }).compileComponents();
    fixture = TestBed.createComponent(UsersPage);
    page = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('loads users and finishes loading on init', () => {
    expect(page.users().length).toBe(page.pageSize);
    expect(page.total()).toBe(USERS.length);
    expect(page.loading()).toBe(false);
  });

  it('formats UTC login times in China Standard Time', async () => {
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('2026-08-01 08:00');
  });

  it('uses server-provided role options', () => {
    const roles = page.roleOptions();
    expect(roles).toContain('Auditor');
  });

  it('does not reload dashboard statistics while filtering or paging', async () => {
    expect(getUserStats).toHaveBeenCalledTimes(1);
    page.goToPage(2);
    await vi.waitFor(() => expect(getUsers).toHaveBeenCalledTimes(2));
    page.applyRoleFilter('Auditor');
    await vi.waitFor(() => expect(getUsers).toHaveBeenCalledTimes(3));
    expect(getUserStats).toHaveBeenCalledTimes(1);
  });

  it('sends a debounced keyword to the server', async () => {
    vi.useFakeTimers();
    page.searchUsers('sarah');
    await vi.advanceTimersByTimeAsync(300);
    expect(getUsers).toHaveBeenLastCalledWith(
      expect.objectContaining({ page: 1, keyword: 'sarah' }),
    );
    vi.useRealTimers();
  });

  it('sends role filtering to the server', async () => {
    page.applyRoleFilter('Auditor');
    await vi.waitFor(() =>
      expect(getUsers).toHaveBeenLastCalledWith(expect.objectContaining({ role: 'Auditor' })),
    );
  });

  it('sends status filtering to the server', async () => {
    page.applyStatusFilter('suspended');
    await vi.waitFor(() =>
      expect(getUsers).toHaveBeenLastCalledWith(expect.objectContaining({ status: 'suspended' })),
    );
  });

  it('resets filters, sorting and page', async () => {
    page.applyRoleFilter('Auditor');
    page.goToPage(2);
    page.applySort('displayName:asc');
    page.resetFilters();
    expect(page.search()).toBe('');
    expect(page.roleFilter()).toBe('all');
    expect(page.statusFilter()).toBe('all');
    expect(page.sortOption()).toBe('createdAt:desc');
    expect(page.page()).toBe(1);
    await vi.waitFor(() =>
      expect(getUsers).toHaveBeenLastCalledWith(
        expect.objectContaining({ page: 1, sortBy: 'createdAt', sortDirection: 'desc' }),
      ),
    );
  });

  it('requests pages with correct totals and bounds', async () => {
    const expectedPages = Math.ceil(USERS.length / page.pageSize);
    expect(page.totalPages()).toBe(expectedPages);
    page.goToPage(99);
    expect(page.page()).toBe(expectedPages);
    await vi.waitFor(() =>
      expect(getUsers).toHaveBeenLastCalledWith(expect.objectContaining({ page: expectedPages })),
    );
    page.goToPage(-5);
    expect(page.page()).toBe(1);
    await vi.waitFor(() =>
      expect(getUsers).toHaveBeenLastCalledWith(expect.objectContaining({ page: 1 })),
    );
  });

  it('keeps page gaps explicit for long result sets', () => {
    page.total.set(200);
    page.page.set(10);

    expect(page.pageNumbers()).toEqual([1, 9, 10, 11, 20]);
  });

  it('tracks row selection', () => {
    const first = USERS[0];
    page.toggleRow(first.id, true);
    expect(page.selected().has(first.id)).toBe(true);
    expect(page.selectedCount()).toBe(1);
    page.toggleRow(first.id, false);
    expect(page.selected().has(first.id)).toBe(false);
  });
});
