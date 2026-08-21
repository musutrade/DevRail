import {
  ChangeDetectionStrategy,
  Component,
  OnInit,
  computed,
  inject,
  signal,
} from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { MatDialog } from '@angular/material/dialog';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { firstValueFrom } from 'rxjs';
import { PermissionApiService } from '../../core/api/permission-api.service';
import { RoleApiService } from '../../core/api/role-api.service';
import { apiErrorMessage } from '../../core/api-error';
import { AuthService } from '../../core/auth.service';
import { ModuleUnlockService } from '../../core/module-unlock.service';
import { RolePermissionRow } from '../../core/models';
import { StepUpDialog } from '../../core/step-up.dialog';
import { AssignPermissionsDialog } from './assign-permissions.dialog';
import { RoleEditorDialog, RoleEditorResult } from '../roles/role-editor.dialog';

@Component({
  selector: 'app-role-permissions',
  imports: [MatIconModule, MatProgressSpinnerModule, MatSnackBarModule],
  templateUrl: './role-permissions.html',
  styleUrl: './role-permissions.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class RolePermissionsPage implements OnInit {
  readonly rows = signal<RolePermissionRow[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly busy = signal(false);
  private requestSequence = 0;
  private readonly permissionApi = inject(PermissionApiService);
  private readonly roleApi = inject(RoleApiService);
  private readonly dialog = inject(MatDialog);
  private readonly auth = inject(AuthService);
  private readonly moduleUnlock = inject(ModuleUnlockService);
  private readonly snackBar = inject(MatSnackBar);

  readonly canCreateRole = computed(() => this.auth.hasPermission('role:write'));

  ngOnInit(): void {
    void this.loadRows();
  }

  retry(): void {
    void this.loadRows();
  }

  private async loadRows(): Promise<void> {
    const requestSequence = ++this.requestSequence;
    this.loading.set(true);
    this.error.set(null);
    try {
      const rows = await this.roleApi.getRolePermissionRows();
      if (requestSequence !== this.requestSequence) {
        return;
      }
      this.rows.set(rows);
    } catch (error) {
      if (requestSequence === this.requestSequence) {
        this.error.set(apiErrorMessage(error, '角色权限数据加载失败，请稍后重试'));
      }
    } finally {
      if (requestSequence === this.requestSequence) {
        this.loading.set(false);
      }
    }
  }

  async onEditPermissions(row: RolePermissionRow): Promise<void> {
    try {
      const [groups, assigned] = await Promise.all([
        this.permissionApi.getPermissionGroups(),
        this.roleApi.getAssignedPermissionIds(row.roleId),
      ]);
      const permissionIds: string[] | undefined = await firstValueFrom(
        this.dialog
          .open(AssignPermissionsDialog, {
            width: '960px',
            maxWidth: 'calc(100vw - 32px)',
            maxHeight: 'calc(100dvh - 32px)',
            panelClass: 'assign-dialog-panel',
            data: { role: row, groups, assigned },
          })
          .afterClosed(),
      );
      if (!permissionIds) {
        return;
      }
      const credentials = await firstValueFrom(
        this.dialog
          .open(StepUpDialog, {
            data: {
              title: '权限变更需要再认证',
              message: '更新角色权限前，请验证当前管理员身份。',
            },
          })
          .afterClosed(),
      );
      if (!credentials) {
        return;
      }
      this.busy.set(true);
      const stepUpToken = await this.auth.issueStepUp(
        'roles.permissions.write',
        credentials.currentPassword,
        credentials.totpCode,
      );
      await this.roleApi.assignRolePermissions(row.roleId, permissionIds, stepUpToken.token);
      await this.auth.refreshSession();
      this.snackBar.open(`已更新 ${row.roleName} 的权限`, '关闭', { duration: 3000 });
      await this.loadRows();
    } catch (error) {
      this.snackBar.open(apiErrorMessage(error, '权限保存失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  async onCreateRole(): Promise<void> {
    const result: RoleEditorResult | undefined = await firstValueFrom(
      this.dialog.open(RoleEditorDialog, { data: null }).afterClosed(),
    );
    if (!result) {
      return;
    }
    if (!(await this.moduleUnlock.ensure('roles', '角色管理'))) {
      return;
    }
    try {
      this.busy.set(true);
      await this.roleApi.createRole({
        code: result.code,
        name: result.name,
        category: result.category,
        icon: result.icon || null,
        color: result.color,
        description: result.description || null,
        dataScope: result.dataScope,
      });
      await this.auth.refreshSession();
      this.snackBar.open(`已创建 ${result.name}`, '关闭', { duration: 3000 });
      await this.loadRows();
    } catch (error) {
      this.snackBar.open(apiErrorMessage(error, '角色创建失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }
}
