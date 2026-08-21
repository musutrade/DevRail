import { provideZonelessChangeDetection } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { MatDialog } from '@angular/material/dialog';
import { MatSnackBar } from '@angular/material/snack-bar';
import { AuthService } from '../../core/auth.service';
import { DepartmentApiService } from '../../features/departments/data-access/department-api.service';
import { Department } from '../../features/departments/models/department.model';
import { DepartmentsPage } from './departments';

const DEPARTMENTS: Department[] = [
  {
    id: 1,
    organizationId: 1,
    parentId: null,
    code: 'root',
    name: '总部',
    status: 'active',
    depth: 0,
    memberCount: 1,
    childCount: 1,
    createdAt: '2026-08-01T00:00:00Z',
    updatedAt: '2026-08-01T00:00:00Z',
  },
  {
    id: 2,
    organizationId: 1,
    parentId: 1,
    code: 'engineering',
    name: '研发部',
    status: 'active',
    depth: 1,
    memberCount: 3,
    childCount: 1,
    createdAt: '2026-08-01T00:00:00Z',
    updatedAt: '2026-08-01T00:00:00Z',
  },
  {
    id: 3,
    organizationId: 1,
    parentId: 2,
    code: 'platform',
    name: '平台组',
    status: 'inactive',
    depth: 2,
    memberCount: 2,
    childCount: 0,
    createdAt: '2026-08-01T00:00:00Z',
    updatedAt: '2026-08-01T00:00:00Z',
  },
];

describe('DepartmentsPage', () => {
  let fixture: ComponentFixture<DepartmentsPage>;
  let page: DepartmentsPage;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [DepartmentsPage],
      providers: [
        provideZonelessChangeDetection(),
        { provide: DepartmentApiService, useValue: { list: vi.fn(async () => DEPARTMENTS) } },
        { provide: AuthService, useValue: { hasPermission: () => true } },
        { provide: MatDialog, useValue: { open: vi.fn() } },
        { provide: MatSnackBar, useValue: { open: vi.fn() } },
      ],
    }).compileComponents();
    fixture = TestBed.createComponent(DepartmentsPage);
    page = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('loads the hierarchy and calculates summary data', () => {
    expect(page.departments()).toHaveLength(3);
    expect(page.activeCount()).toBe(2);
    expect(page.memberCount()).toBe(6);
  });

  it('collapses descendants and keeps ancestors during search', () => {
    page.toggle(DEPARTMENTS[0]);
    expect(page.visibleDepartments().map((department) => department.id)).toEqual([1]);

    page.setSearch('platform');
    expect(page.visibleDepartments().map((department) => department.id)).toEqual([1, 2, 3]);
  });

  it('explains why a child department cannot be created under an inactive department', async () => {
    const snackBar = (page as unknown as { snackBar: MatSnackBar }).snackBar;
    const open = vi.spyOn(snackBar, 'open').mockImplementation(() => null as never);

    await page.onCreate(DEPARTMENTS[2]);

    expect(open).toHaveBeenCalledWith(
      '停用部门不能新增下级部门，请先启用该部门',
      '关闭',
      expect.objectContaining({ duration: 5000 }),
    );
    open.mockRestore();
  });
});
