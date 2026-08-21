import {
  ChangeDetectionStrategy,
  Component,
  OnInit,
  computed,
  inject,
  signal,
} from '@angular/core';
import { MatDialog, MatDialogModule } from '@angular/material/dialog';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { firstValueFrom } from 'rxjs';
import { apiErrorMessage } from '../../core/api-error';
import { AuthService, StepUpScope } from '../../core/auth.service';
import { ConfirmDialog } from '../../core/confirm.dialog';
import { StepUpCredentials, StepUpDialog } from '../../core/step-up.dialog';
import { DepartmentApiService } from '../../features/departments/data-access/department-api.service';
import { DEPARTMENT_PERMISSIONS } from '../../features/departments/departments.permissions';
import { Department } from '../../features/departments/models/department.model';
import { DepartmentEditorDialog, DepartmentEditorResult } from './department-editor.dialog';

@Component({
  selector: 'app-departments',
  imports: [MatDialogModule, MatIconModule, MatProgressSpinnerModule, MatSnackBarModule],
  templateUrl: './departments.html',
  styleUrl: './departments.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DepartmentsPage implements OnInit {
  readonly departments = signal<Department[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly search = signal('');
  readonly statusFilter = signal<'all' | Department['status']>('all');
  readonly collapsed = signal<ReadonlySet<number>>(new Set());
  readonly busy = signal(false);
  private requestSequence = 0;

  private readonly departmentApi = inject(DepartmentApiService);
  private readonly auth = inject(AuthService);
  private readonly dialog = inject(MatDialog);
  private readonly snackBar = inject(MatSnackBar);

  readonly canWrite = computed(() => this.auth.hasPermission(DEPARTMENT_PERMISSIONS.write));
  readonly activeCount = computed(
    () => this.departments().filter((department) => department.status === 'active').length,
  );
  readonly memberCount = computed(() =>
    this.departments().reduce((total, department) => total + department.memberCount, 0),
  );
  readonly visibleDepartments = computed(() => {
    const departments = this.departments();
    const keyword = this.search().trim().toLocaleLowerCase();
    const status = this.statusFilter();
    if (keyword || status !== 'all') {
      const included = new Set<number>();
      const byId = new Map(departments.map((department) => [department.id, department]));
      for (const department of departments) {
        const matchesKeyword =
          !keyword ||
          department.name.toLocaleLowerCase().includes(keyword) ||
          department.code.toLocaleLowerCase().includes(keyword);
        if (matchesKeyword && (status === 'all' || department.status === status)) {
          let current: Department | undefined = department;
          while (current) {
            included.add(current.id);
            current = current.parentId === null ? undefined : byId.get(current.parentId);
          }
        }
      }
      return departments.filter((department) => included.has(department.id));
    }
    const collapsed = this.collapsed();
    const byId = new Map(departments.map((department) => [department.id, department]));
    return departments.filter((department) => {
      let parentId = department.parentId;
      while (parentId !== null) {
        if (collapsed.has(parentId)) {
          return false;
        }
        parentId = byId.get(parentId)?.parentId ?? null;
      }
      return true;
    });
  });

  ngOnInit(): void {
    void this.loadDepartments();
  }

  setSearch(value: string): void {
    this.search.set(value);
  }

  setStatus(value: string): void {
    this.statusFilter.set(value as 'all' | Department['status']);
  }

  toggle(department: Department): void {
    if (department.childCount === 0) {
      return;
    }
    this.collapsed.update((current) => {
      const next = new Set(current);
      if (next.has(department.id)) {
        next.delete(department.id);
      } else {
        next.add(department.id);
      }
      return next;
    });
  }

  async onCreate(parent?: Department): Promise<void> {
    if (parent && parent.status !== 'active') {
      this.snackBar.open('停用部门不能新增下级部门，请先启用该部门', '关闭', { duration: 5000 });
      return;
    }
    await this.openEditor(null, parent?.id);
  }

  retry(): void {
    void this.loadDepartments();
  }

  async onEdit(department: Department): Promise<void> {
    await this.openEditor(department);
  }

  async onDelete(department: Department): Promise<void> {
    if (department.childCount > 0 || department.memberCount > 0) {
      this.snackBar.open('请先迁移下级部门和部门成员', '关闭', { duration: 5000 });
      return;
    }
    const confirmed = await firstValueFrom(
      this.dialog
        .open(ConfirmDialog, {
          data: {
            title: '删除部门',
            message: `确定删除 ${department.name} 吗？此操作不可撤销。`,
            confirmLabel: '删除部门',
            danger: true,
          },
        })
        .afterClosed(),
    );
    if (!confirmed) {
      return;
    }
    const token = await this.stepUp(
      'departments.delete',
      '删除部门需要再认证',
      '删除部门前，请验证当前管理员身份。',
    );
    if (!token) {
      return;
    }
    await this.runMutation(
      () => this.departmentApi.delete(department.id, token),
      `已删除 ${department.name}`,
    );
  }

  isRoot(department: Department): boolean {
    return department.parentId === null;
  }

  private async loadDepartments(): Promise<void> {
    const requestSequence = ++this.requestSequence;
    this.loading.set(true);
    this.error.set(null);
    try {
      const departments = await this.departmentApi.list();
      if (requestSequence !== this.requestSequence) {
        return;
      }
      this.departments.set(departments);
    } catch (error) {
      if (requestSequence === this.requestSequence) {
        this.error.set(apiErrorMessage(error, '部门数据加载失败，请稍后重试'));
      }
    } finally {
      if (requestSequence === this.requestSequence) {
        this.loading.set(false);
      }
    }
  }

  private async openEditor(department: Department | null, parentId?: number): Promise<void> {
    const result = (await firstValueFrom(
      this.dialog
        .open(DepartmentEditorDialog, {
          width: '640px',
          maxWidth: 'calc(100vw - 32px)',
          data: { department, departments: this.departments(), parentId },
        })
        .afterClosed(),
    )) as DepartmentEditorResult | undefined;
    if (!result) {
      return;
    }
    if (!department && result.parentId === undefined) {
      this.snackBar.open('请选择有效的上级部门', '关闭', { duration: 5000 });
      return;
    }
    const token = await this.stepUp(
      'departments.write',
      '部门变更需要再认证',
      '保存部门前，请验证当前管理员身份。',
    );
    if (!token) {
      return;
    }
    await this.runMutation(
      () => {
        if (department) {
          return this.departmentApi.update(department.id, result, token);
        }
        return this.departmentApi.create(
          {
            parentId: result.parentId!,
            code: result.code,
            name: result.name,
            status: result.status,
          },
          token,
        );
      },
      department ? `已更新 ${result.name}` : `已创建 ${result.name}`,
    );
  }

  private async runMutation(action: () => Promise<unknown>, success: string): Promise<void> {
    this.busy.set(true);
    try {
      await action();
      this.snackBar.open(success, '关闭', { duration: 3000 });
      await this.loadDepartments();
    } catch (error) {
      this.snackBar.open(apiErrorMessage(error, '操作失败，请稍后重试'), '关闭', {
        duration: 5000,
      });
    } finally {
      this.busy.set(false);
    }
  }

  private async stepUp(
    scope: StepUpScope,
    title: string,
    message: string,
  ): Promise<string | undefined> {
    const credentials = (await firstValueFrom(
      this.dialog.open(StepUpDialog, { data: { title, message } }).afterClosed(),
    )) as StepUpCredentials | undefined;
    if (!credentials) {
      return undefined;
    }
    try {
      return (await this.auth.issueStepUp(scope, credentials.currentPassword, credentials.totpCode))
        .token;
    } catch (error) {
      this.snackBar.open(apiErrorMessage(error, '身份再认证失败'), '关闭', { duration: 5000 });
      return undefined;
    }
  }
}
