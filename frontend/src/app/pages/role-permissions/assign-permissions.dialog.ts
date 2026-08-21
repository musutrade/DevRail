import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatDialogRef, MAT_DIALOG_DATA, MatDialogModule } from '@angular/material/dialog';
import { Permission, PermissionGroup, RolePermissionRow } from '../../core/models';

export interface AssignPermissionsData {
  role: RolePermissionRow;
  groups: PermissionGroup[];
  assigned: string[];
}

@Component({
  selector: 'app-assign-permissions-dialog',
  imports: [MatIconModule, MatCheckboxModule, MatDialogModule],
  templateUrl: './assign-permissions.dialog.html',
  styleUrl: './assign-permissions.dialog.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AssignPermissionsDialog {
  readonly data = inject<AssignPermissionsData>(MAT_DIALOG_DATA);
  private readonly dialogRef = inject(MatDialogRef<AssignPermissionsDialog>);

  readonly search = signal('');
  readonly assigned = signal<Set<string>>(new Set(this.data.assigned));

  /** 过滤后的权限分组(供模板渲染) */
  readonly filteredGroups = computed(() => {
    const term = this.search().trim().toLowerCase();
    return this.data.groups
      .map((g) => ({
        ...g,
        permissions: g.permissions.filter(
          (p) =>
            !term ||
            p.name.toLowerCase().includes(term) ||
            p.code.toLowerCase().includes(term) ||
            p.description.toLowerCase().includes(term),
        ),
      }))
      .filter((g) => g.permissions.length > 0);
  });

  /** 可见(过滤后)权限 id 列表 */
  readonly visibleIds = computed<string[]>(() =>
    this.filteredGroups().flatMap((g) => g.permissions.map((p) => p.id)),
  );

  readonly selectedCount = computed(() => this.assigned().size);
  readonly visibleCount = computed(() => this.visibleIds().length);

  readonly allVisibleChecked = computed(() => {
    const visible = this.visibleIds();
    return visible.length > 0 && visible.every((id) => this.assigned().has(id));
  });

  readonly someVisibleChecked = computed(() => {
    const visible = this.visibleIds();
    return visible.some((id) => this.assigned().has(id)) && !this.allVisibleChecked();
  });

  searchPermissions(value: string): void {
    this.search.set(value);
  }

  togglePermission(id: string, checked: boolean): void {
    this.assigned.update((s) => {
      const next = new Set(s);
      if (checked) {
        next.add(id);
      } else {
        next.delete(id);
      }
      return next;
    });
  }

  selectAll(): void {
    this.assigned.update((s) => {
      const next = new Set(s);
      this.visibleIds().forEach((id) => next.add(id));
      return next;
    });
  }

  invertSelection(): void {
    this.assigned.update((s) => {
      const next = new Set(s);
      this.visibleIds().forEach((id) => {
        if (next.has(id)) {
          next.delete(id);
        } else {
          next.add(id);
        }
      });
      return next;
    });
  }

  isAssigned(p: Permission): boolean {
    return this.assigned().has(p.id);
  }

  onCancel(): void {
    this.dialogRef.close();
  }

  onSave(): void {
    this.dialogRef.close([...this.assigned()]);
  }
}
