import { provideZonelessChangeDetection } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { MAT_DIALOG_DATA, MatDialogRef } from '@angular/material/dialog';
import { Department } from '../../features/departments/models/department.model';
import { DepartmentEditorDialog } from './department-editor.dialog';

const departments: Department[] = [
  {
    id: 1,
    organizationId: 1,
    parentId: null,
    code: 'root',
    name: '总部',
    status: 'active',
    depth: 0,
    memberCount: 0,
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
    status: 'inactive',
    depth: 1,
    memberCount: 0,
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
    status: 'active',
    depth: 2,
    memberCount: 0,
    childCount: 0,
    createdAt: '2026-08-01T00:00:00Z',
    updatedAt: '2026-08-01T00:00:00Z',
  },
];

describe('DepartmentEditorDialog', () => {
  let fixture: ComponentFixture<DepartmentEditorDialog>;
  let close: ReturnType<typeof vi.fn>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [DepartmentEditorDialog],
      providers: [
        provideZonelessChangeDetection(),
        {
          provide: MAT_DIALOG_DATA,
          useValue: { department: departments[2], departments },
        },
        { provide: MatDialogRef, useValue: { close: vi.fn() } },
      ],
    }).compileComponents();
    fixture = TestBed.createComponent(DepartmentEditorDialog);
    close = TestBed.inject(MatDialogRef).close as ReturnType<typeof vi.fn>;
    await fixture.whenStable();
  });

  it('keeps an inactive current parent while excluding the current department and descendants', () => {
    expect(fixture.componentInstance.parentOptions().map((department) => department.id)).toEqual([
      1, 2,
    ]);
  });

  it('submits the unchanged inactive parent so the server can preserve it', () => {
    fixture.componentInstance.submit();
    expect(close).toHaveBeenCalledWith(expect.objectContaining({ parentId: 2 }));
  });
});
