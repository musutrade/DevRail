import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { FormBuilder, ReactiveFormsModule, Validators } from '@angular/forms';
import { MAT_DIALOG_DATA, MatDialogModule, MatDialogRef } from '@angular/material/dialog';
import { MatIconModule } from '@angular/material/icon';
import { Department, DepartmentStatus } from '../../features/departments/models/department.model';

export interface DepartmentEditorData {
  readonly department: Department | null;
  readonly departments: readonly Department[];
  readonly parentId?: number;
}

export interface DepartmentEditorResult {
  readonly parentId?: number;
  readonly code: string;
  readonly name: string;
  readonly status: DepartmentStatus;
}

@Component({
  selector: 'app-department-editor-dialog',
  imports: [ReactiveFormsModule, MatDialogModule, MatIconModule],
  templateUrl: './department-editor.dialog.html',
  styleUrl: '../../core/editor-dialog.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DepartmentEditorDialog {
  readonly data = inject<DepartmentEditorData>(MAT_DIALOG_DATA);
  private readonly dialogRef = inject(MatDialogRef<DepartmentEditorDialog>);
  private readonly fb = inject(FormBuilder);

  readonly parentOptions = computed(() => {
    const blocked = this.descendantIds(this.data.department?.id);
    const currentParentId = this.data.department?.parentId;
    return this.data.departments.filter(
      (department) =>
        !blocked.has(department.id) &&
        (department.status === 'active' || department.id === currentParentId),
    );
  });
  readonly canChangeParent =
    this.data.department === null ||
    this.data.departments.some((department) => department.id === this.data.department?.parentId);

  readonly form = this.fb.nonNullable.group({
    parentId: [
      this.data.department?.parentId ?? this.data.parentId ?? this.defaultParentId(),
      [Validators.required, Validators.min(1)],
    ],
    code: [
      this.data.department?.code ?? '',
      [Validators.required, Validators.pattern(/^[a-z][a-z0-9_-]{1,63}$/)],
    ],
    name: [this.data.department?.name ?? '', [Validators.required, Validators.maxLength(128)]],
    status: [this.data.department?.status ?? ('active' as DepartmentStatus)],
  });

  submit(): void {
    if (this.form.invalid) {
      this.form.markAllAsTouched();
      return;
    }
    const value = this.form.getRawValue();
    this.dialogRef.close({
      code: value.code,
      name: value.name,
      status: value.status,
      ...(this.canChangeParent ? { parentId: value.parentId } : {}),
    } satisfies DepartmentEditorResult);
  }

  private defaultParentId(): number {
    return (
      this.data.departments.find((department) => department.parentId === null)?.id ??
      this.data.departments[0]?.id ??
      0
    );
  }

  private descendantIds(departmentId: number | undefined): Set<number> {
    if (departmentId === undefined) {
      return new Set();
    }
    const blocked = new Set([departmentId]);
    let changed = true;
    while (changed) {
      changed = false;
      for (const department of this.data.departments) {
        if (
          department.parentId !== null &&
          blocked.has(department.parentId) &&
          !blocked.has(department.id)
        ) {
          blocked.add(department.id);
          changed = true;
        }
      }
    }
    return blocked;
  }
}
