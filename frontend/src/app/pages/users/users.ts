import {
  ChangeDetectionStrategy,
  Component,
  OnDestroy,
  OnInit,
  computed,
  inject,
  signal,
} from '@angular/core';
import { DatePipe } from '@angular/common';
import { MatIconModule } from '@angular/material/icon';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { MatDialog, MatDialogModule } from '@angular/material/dialog';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { firstValueFrom } from 'rxjs';
import { DashboardApiService } from '../../core/api/dashboard-api.service';
import { RoleApiService } from '../../core/api/role-api.service';
import { UserApiService, UserListQuery } from '../../core/api/user-api.service';
import { Role, StatCard, User, UserStatus } from '../../core/models';
import { AuthService } from '../../core/auth.service';
import { apiErrorMessage } from '../../core/api-error';
import { ConfirmDialog } from '../../core/confirm.dialog';
import { ModuleUnlockService } from '../../core/module-unlock.service';
import { StepUpCredentials, StepUpDialog } from '../../core/step-up.dialog';
import { DepartmentApiService } from '../../features/departments/data-access/department-api.service';
import { DEPARTMENT_PERMISSIONS } from '../../features/departments/departments.permissions';
import { PasswordDialog } from './password.dialog';
import { RoleSelectionDialog } from './role-selection.dialog';
import { UserEditorDialog, UserEditorResult } from './user-editor.dialog';

const STATUS_META: Record<UserStatus, { cls: string; label: string }> = {
  active: { cls: 'st-active', label: '启用' },
  inactive: { cls: 'st-inactive', label: '停用' },
  suspended: { cls: 'st-suspended', label: '已暂停' },
};

@Component({
  selector: 'app-users',
  imports: [
    MatIconModule,
    MatCheckboxModule,
    MatProgressSpinnerModule,
    MatDialogModule,
    MatSnackBarModule,
    DatePipe,
  ],
  templateUrl: './users.html',
  styleUrl: './users.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class UsersPage implements OnInit, OnDestroy {
  readonly users = signal<User[]>([]);
  readonly stats = signal<StatCard[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly search = signal('');
  readonly roleFilter = signal('all');
  readonly statusFilter = signal('all');
  readonly sortOption = signal('createdAt:desc');
  readonly selected = signal<Set<string>>(new Set());
  readonly page = signal(1);
  readonly pageSize = 10;
  readonly total = signal(0);
  readonly roleOptions = signal<string[]>([]);
  readonly busy = signal(false);

  private searchTimer: ReturnType<typeof setTimeout> | undefined;
  private requestSequence = 0;

  private readonly dashboardApi = inject(DashboardApiService);
  private readonly departmentApi = inject(DepartmentApiService);
  private readonly roleApi = inject(RoleApiService);
  private readonly userApi = inject(UserApiService);
  private readonly auth = inject(AuthService);
  private readonly moduleUnlock = inject(ModuleUnlockService);
  private readonly dialog = inject(MatDialog);
  private readonly snackBar = inject(MatSnackBar);

  readonly canWrite = computed(() => this.auth.hasPermission('user:write'));
  readonly canResetPassword = computed(() => this.auth.hasPermission('user:admin:reset_password'));
  readonly canDeactivate = computed(() => this.auth.hasPermission('user:admin:deactivate'));
  readonly canManageStatus = computed(() => this.canWrite() && this.canDeactivate());
  readonly canManageRoles = computed(() => this.auth.hasPermission('user:roles:write'));
  readonly canGrantSuperAdmin = computed(() => this.auth.hasPermission('user:super_admin:grant'));
  readonly canManageDepartment = computed(() =>
    this.auth.hasPermission(DEPARTMENT_PERMISSIONS.read),
  );

  /** 状态展示元数据暴露给模板 */
  readonly statusMeta = STATUS_META;

  /** 总页数 */
  readonly totalPages = computed(() => Math.max(1, Math.ceil(this.total() / this.pageSize)));

  /** 当前页数据 */
  readonly pagedUsers = computed(() => this.users());

  readonly selectedCount = computed(() => this.selected().size);

  readonly allChecked = computed(() => {
    const rows = this.users().filter((user) => !this.isCurrentUser(user));
    return rows.length > 0 && rows.every((u) => this.selected().has(u.id));
  });

  readonly someChecked = computed(() => {
    const rows = this.users().filter((user) => !this.isCurrentUser(user));
    return rows.some((u) => this.selected().has(u.id)) && !this.allChecked();
  });

  /** 当前页首条序号(1-based) */
  readonly pageStart = computed(() =>
    this.total() === 0 ? 0 : (this.page() - 1) * this.pageSize + 1,
  );

  /** 当前页末条序号 */
  readonly pageEnd = computed(() => Math.min(this.page() * this.pageSize, this.total()));

  /** 页码数组(少于 7 页全部展示,否则首尾+省略) */
  readonly pageNumbers = computed<number[]>(() => {
    const total = this.totalPages();
    if (total <= 7) {
      return Array.from({ length: total }, (_, i) => i + 1);
    }
    const cur = this.page();
    const pages = new Set<number>([1, total, cur - 1, cur, cur + 1]);
    return [...pages].filter((p) => p >= 1 && p <= total).sort((a, b) => a - b);
  });

  retry(): void {
    void this.loadData();
    void this.loadStats();
  }

  ngOnInit(): void {
    void this.loadData();
    void this.loadStats();
  }

  ngOnDestroy(): void {
    if (this.searchTimer) {
      clearTimeout(this.searchTimer);
    }
  }

  private async loadData(): Promise<void> {
    const requestSequence = ++this.requestSequence;
    this.loading.set(true);
    this.error.set(null);
    try {
      const result = await this.userApi.getUsers(this.userQuery());
      if (requestSequence !== this.requestSequence) {
        return;
      }
      this.users.set(result.items);
      this.total.set(result.total);
      this.page.set(result.page);
      this.roleOptions.set(result.roleOptions);
      this.selected.set(new Set());
    } catch (error) {
      if (requestSequence === this.requestSequence) {
        this.error.set(apiErrorMessage(error, '用户数据加载失败，请稍后重试'));
      }
    } finally {
      if (requestSequence === this.requestSequence) {
        this.loading.set(false);
      }
    }
  }

  private async loadStats(): Promise<void> {
    if (!this.auth.hasPermission('dashboard:analytics:read')) {
      this.stats.set([]);
      return;
    }
    try {
      this.stats.set(await this.dashboardApi.getUserStats());
    } catch {
      this.stats.set([]);
    }
  }

  searchUsers(value: string): void {
    this.search.set(value);
    this.page.set(1);
    if (this.searchTimer) {
      clearTimeout(this.searchTimer);
    }
    this.searchTimer = setTimeout(() => void this.loadData(), 300);
  }

  applyRoleFilter(value: string): void {
    this.roleFilter.set(value);
    this.page.set(1);
    void this.loadData();
  }

  applyStatusFilter(value: string): void {
    this.statusFilter.set(value);
    this.page.set(1);
    void this.loadData();
  }

  applySort(value: string): void {
    this.sortOption.set(value);
    this.page.set(1);
    void this.loadData();
  }

  resetFilters(): void {
    this.search.set('');
    this.roleFilter.set('all');
    this.statusFilter.set('all');
    this.sortOption.set('createdAt:desc');
    this.page.set(1);
    void this.loadData();
  }

  goToPage(p: number): void {
    const next = Math.min(Math.max(1, p), this.totalPages());
    if (next !== this.page()) {
      this.page.set(next);
      void this.loadData();
    }
  }

  toggleRow(id: string, checked: boolean): void {
    if (this.isCurrentUserId(id)) {
      return;
    }
    this.selected.update((s) => {
      const next = new Set(s);
      if (checked) {
        next.add(id);
      } else {
        next.delete(id);
      }
      return next;
    });
  }

  toggleAll(checked: boolean): void {
    this.selected.update((s) => {
      const next = new Set(s);
      this.users().forEach((u) => {
        if (this.isCurrentUser(u)) {
          return;
        }
        if (checked) {
          next.add(u.id);
        } else {
          next.delete(u.id);
        }
      });
      return next;
    });
  }

  async onCreate(): Promise<void> {
    await this.openEditor();
  }

  async onEdit(user: User): Promise<void> {
    await this.openEditor(user);
  }

  async onResetPassword(user: User): Promise<void> {
    const password: string | undefined = await firstValueFrom(
      this.dialog.open(PasswordDialog, { data: user.username }).afterClosed(),
    );
    if (!password) {
      return;
    }
    const stepUpToken = await this.stepUp(
      'users.sensitive',
      '敏感操作需要再认证',
      '重置用户密码前，请验证当前管理员身份。',
    );
    if (!stepUpToken) {
      return;
    }
    await this.runMutation(
      () => this.userApi.updateUser(user.id, { password }, stepUpToken),
      `已重置 ${user.name} 的密码`,
    );
  }

  async onToggleStatus(user: User): Promise<void> {
    const activating = user.status !== 'active';
    const action = activating ? '启用' : '停用';
    const confirmed = await this.confirm(
      `${action}用户`,
      activating
        ? `确定重新启用 ${user.name} 吗？启用后该账号可以正常登录。`
        : `确定停用 ${user.name} 吗？停用后该账号将无法登录。`,
      `${action}用户`,
      !activating,
    );
    if (!confirmed) {
      return;
    }
    const stepUpToken = await this.stepUp(
      'users.sensitive',
      '敏感操作需要再认证',
      `${action}用户前，请验证当前管理员身份。`,
    );
    if (!stepUpToken) {
      return;
    }
    await this.runMutation(
      () =>
        this.userApi.updateUser(
          user.id,
          { status: activating ? 'active' : 'inactive' },
          stepUpToken,
        ),
      `已${action} ${user.name}`,
    );
  }

  isCurrentUser(user: User): boolean {
    return String(this.auth.currentUser()?.id ?? '') === user.id;
  }

  async onDelete(user: User): Promise<void> {
    const confirmed = await this.confirm(
      '删除用户',
      `确定删除 ${user.name} 吗？删除后该账号将立即停用。`,
      '删除用户',
    );
    if (!confirmed) {
      return;
    }
    const stepUpToken = await this.stepUp(
      'users.delete',
      '删除用户需要再认证',
      '删除后账号将立即停用，请验证当前管理员身份。',
    );
    if (!stepUpToken) {
      return;
    }
    await this.runMutation(
      () => this.userApi.deleteUser(user.id, stepUpToken),
      `已删除 ${user.name}`,
    );
  }

  async deleteSelected(): Promise<void> {
    const ids = [...this.selected()].filter((id) => !this.isCurrentUserId(id));
    if (ids.length === 0) {
      this.snackBar.open('请选择可删除的用户，当前登录账号不能删除', '关闭', { duration: 5000 });
      return;
    }
    if (
      !(await this.confirm('删除所选用户', `确定删除选中的 ${ids.length} 个账号吗？`, '删除用户'))
    ) {
      return;
    }
    const credentials = await this.stepUpCredentials(
      '删除用户需要再认证',
      '批量删除前，请验证当前管理员身份。',
    );
    if (!credentials) {
      return;
    }
    await this.runBatchMutation(
      'users.delete',
      credentials,
      (token) => this.userApi.batchDeleteUsers(ids, token),
      `已删除 ${ids.length} 个用户`,
    );
  }

  async changeSelectedRoles(): Promise<void> {
    const ids = [...this.selected()];
    if (ids.length === 0) {
      return;
    }
    try {
      const roles = this.filterGrantableRoles(await this.roleApi.getRoles());
      const roleIds: string[] | undefined = await firstValueFrom(
        this.dialog.open(RoleSelectionDialog, { data: roles }).afterClosed(),
      );
      if (!roleIds) {
        return;
      }
      const credentials = await this.stepUpCredentials(
        '角色分配需要再认证',
        '批量更新角色前，请验证当前管理员身份。',
      );
      if (!credentials) {
        return;
      }
      await this.runBatchMutation(
        'users.roles.write',
        credentials,
        (token) => this.userApi.batchAssignUserRoles(ids, roleIds, token),
        `已更新 ${ids.length} 个用户的角色`,
      );
    } catch (error) {
      this.showError(error, '角色数据加载失败');
    }
  }

  private async openEditor(user?: User): Promise<void> {
    try {
      const canManageRoles = this.canManageRoles();
      const canManageStatus = this.canManageStatus();
      const canResetPassword = this.canResetPassword();
      const canManageDepartment = this.canManageDepartment();
      const [loadedRoles, departments] = await Promise.all([
        canManageRoles ? this.roleApi.getRoles() : Promise.resolve([]),
        canManageDepartment ? this.departmentApi.list() : Promise.resolve([]),
      ]);
      const roles = this.filterGrantableRoles(loadedRoles);
      const result: UserEditorResult | undefined = await firstValueFrom(
        this.dialog
          .open(UserEditorDialog, {
            data: {
              user,
              roles,
              departments,
              defaultDepartmentId: this.auth.currentUser()?.departmentId ?? null,
              canManageRoles,
              canManageStatus,
              canResetPassword,
              canManageDepartment,
            },
          })
          .afterClosed(),
      );
      if (!result) {
        return;
      }
      if (user) {
        const email = result.email || null;
        const basicChanged = result.displayName !== user.name || (email ?? '') !== user.email;
        const statusChanged = canManageStatus && result.status !== user.status;
        const passwordChanged = canResetPassword && Boolean(result.password);
        const departmentChanged =
          canManageDepartment &&
          result.departmentId > 0 &&
          result.departmentId !== user.departmentId;
        const currentRoleIds = roles
          .filter((role) => user.roles.includes(role.name))
          .map((role) => role.id);
        const rolesChanged = canManageRoles && !sameValues(currentRoleIds, result.roleIds);
        const updateRequest = {
          displayName: result.displayName,
          email,
          ...(statusChanged ? { status: result.status } : {}),
          ...(passwordChanged ? { password: result.password } : {}),
          ...(departmentChanged ? { departmentId: result.departmentId } : {}),
        };
        const updateNeeded = basicChanged || statusChanged || passwordChanged || departmentChanged;
        const updateNeedsStepUp = statusChanged || passwordChanged || departmentChanged;
        const updateToken = updateNeedsStepUp
          ? await this.stepUp(
              'users.sensitive',
              '敏感操作需要再认证',
              '修改用户密码、状态或所属部门前，请验证当前管理员身份。',
            )
          : undefined;
        if (updateNeedsStepUp && !updateToken) {
          return;
        }
        if (
          updateNeeded &&
          !updateNeedsStepUp &&
          !(await this.moduleUnlock.ensure('users', '用户管理'))
        ) {
          return;
        }
        const roleToken = rolesChanged
          ? await this.stepUp(
              'users.roles.write',
              '角色分配需要再认证',
              '更新用户角色前，请验证当前管理员身份。',
            )
          : undefined;
        if (rolesChanged && !roleToken) {
          return;
        }
        if (!updateNeeded && !rolesChanged) {
          this.snackBar.open('未检测到需要保存的变更', '关闭', { duration: 3000 });
          return;
        }
        await this.runMutation(async () => {
          if (updateNeeded) {
            await this.userApi.updateUser(user.id, updateRequest, updateToken);
          }
          if (rolesChanged) {
            await this.userApi.assignUserRoles(user.id, result.roleIds, roleToken!);
          }
        }, `已更新 ${result.displayName}`);
      } else {
        const currentDepartmentId = this.auth.currentUser()?.departmentId ?? null;
        const departmentChanged =
          canManageDepartment &&
          result.departmentId > 0 &&
          result.departmentId !== currentDepartmentId;
        const createRequest = {
          username: result.username,
          password: result.password,
          displayName: result.displayName,
          email: result.email || null,
          ...(canManageStatus ? { status: result.status } : {}),
          ...(canManageRoles ? { roleIds: result.roleIds.map(Number) } : {}),
          ...(canManageDepartment && result.departmentId > 0
            ? { departmentId: result.departmentId }
            : {}),
        };
        const createNeedsStepUp =
          (canManageStatus && result.status !== 'active') ||
          (canManageRoles && result.roleIds.length > 0) ||
          departmentChanged;
        const createToken = createNeedsStepUp
          ? await this.stepUp(
              'users.sensitive',
              '敏感操作需要再认证',
              '创建带有角色、非启用状态或跨部门归属的用户前，请验证当前管理员身份。',
            )
          : undefined;
        if (createNeedsStepUp && !createToken) {
          return;
        }
        if (!createNeedsStepUp && !(await this.moduleUnlock.ensure('users', '用户管理'))) {
          return;
        }
        await this.runMutation(
          () => this.userApi.createUser(createRequest, createToken),
          `已创建 ${result.displayName}`,
        );
      }
    } catch (error) {
      this.showError(error, '用户编辑器加载失败');
    }
  }

  private async runMutation(action: () => Promise<unknown>, success: string): Promise<void> {
    this.busy.set(true);
    try {
      await action();
      await this.auth.refreshSession();
      this.snackBar.open(success, '关闭', { duration: 3000 });
      await this.auth.refreshSession();
      await Promise.all([this.loadData(), this.loadStats()]);
    } catch (error) {
      this.showError(error, '操作失败，请稍后重试');
    } finally {
      this.busy.set(false);
    }
  }

  private async stepUp(
    scope: import('../../core/auth.service').StepUpScope,
    title: string,
    message: string,
  ): Promise<string | undefined> {
    const credentials = await this.stepUpCredentials(title, message);
    if (!credentials) {
      return undefined;
    }
    try {
      return (await this.auth.issueStepUp(scope, credentials.currentPassword, credentials.totpCode))
        .token;
    } catch (error) {
      this.showError(error, '身份再认证失败');
      return undefined;
    }
  }

  private stepUpCredentials(
    title: string,
    message: string,
  ): Promise<StepUpCredentials | undefined> {
    return firstValueFrom(
      this.dialog.open(StepUpDialog, { data: { title, message } }).afterClosed(),
    );
  }

  private userQuery(): UserListQuery {
    const [sortBy, sortDirection] = this.sortOption().split(':') as [
      UserListQuery['sortBy'],
      UserListQuery['sortDirection'],
    ];
    const keyword = this.search().trim();
    return {
      page: this.page(),
      pageSize: this.pageSize,
      ...(keyword ? { keyword } : {}),
      ...(this.roleFilter() !== 'all' ? { role: this.roleFilter() } : {}),
      ...(this.statusFilter() !== 'all'
        ? { status: this.statusFilter() as UserListQuery['status'] }
        : {}),
      sortBy,
      sortDirection,
    };
  }

  private filterGrantableRoles(roles: Role[]): Role[] {
    return roles.filter(
      (role) => role.isActive && (this.canGrantSuperAdmin() || role.code !== 'super_admin'),
    );
  }

  private async runBatchMutation(
    scope: import('../../core/auth.service').StepUpScope,
    credentials: StepUpCredentials,
    action: (token: string) => Promise<unknown>,
    success: string,
  ): Promise<void> {
    this.busy.set(true);
    try {
      const token = await this.auth.issueStepUp(
        scope,
        credentials.currentPassword,
        credentials.totpCode,
      );
      await action(token.token);
      await this.auth.refreshSession();
      await Promise.all([this.loadData(), this.loadStats()]);
      this.snackBar.open(success, '关闭', { duration: 3000 });
    } catch (error) {
      this.showError(error, '批量操作失败，未保存任何变更');
    } finally {
      this.busy.set(false);
    }
  }

  private confirm(
    title: string,
    message: string,
    confirmLabel: string,
    danger = true,
  ): Promise<boolean> {
    return firstValueFrom(
      this.dialog
        .open(ConfirmDialog, {
          data: { title, message, confirmLabel, danger },
        })
        .afterClosed(),
    ).then(Boolean);
  }

  private showError(error: unknown, fallback: string): void {
    this.snackBar.open(apiErrorMessage(error, fallback), '关闭', { duration: 5000 });
  }

  private isCurrentUserId(id: string): boolean {
    return String(this.auth.currentUser()?.id ?? '') === id;
  }

  initials(name: string): string {
    return name
      .split(' ')
      .map((n) => n[0])
      .slice(0, 2)
      .join('')
      .toUpperCase();
  }
}

function sameValues(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((value) => right.includes(value));
}
