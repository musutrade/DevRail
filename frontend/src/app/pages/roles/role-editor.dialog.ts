import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { FormBuilder, ReactiveFormsModule, Validators } from '@angular/forms';
import { MAT_DIALOG_DATA, MatDialogModule, MatDialogRef } from '@angular/material/dialog';
import { MatIconModule } from '@angular/material/icon';
import { MatSelectModule } from '@angular/material/select';
import { DataScope, Role } from '../../core/models';

interface RoleIconOption {
  value: string;
  label: string;
}

interface RoleIconGroup {
  label: string;
  options: readonly RoleIconOption[];
}

const ROLE_ICON_GROUPS: readonly RoleIconGroup[] = [
  {
    label: '人员',
    options: [
      { value: 'badge', label: '角色徽章' },
      { value: 'person', label: '人员' },
      { value: 'group', label: '团队' },
      { value: 'groups', label: '组织' },
      { value: 'supervisor_account', label: '主管' },
      { value: 'manage_accounts', label: '账号管理员' },
      { value: 'engineering', label: '工程师' },
      { value: 'support_agent', label: '客服支持' },
    ],
  },
  {
    label: '访问与安全',
    options: [
      { value: 'admin_panel_settings', label: '管理员' },
      { value: 'shield', label: '安全' },
      { value: 'verified_user', label: '已验证用户' },
      { value: 'lock', label: '受限访问' },
      { value: 'key', label: '访问密钥' },
      { value: 'policy', label: '策略' },
      { value: 'gavel', label: '合规' },
      { value: 'visibility', label: '只读' },
    ],
  },
  {
    label: '组织',
    options: [
      { value: 'business', label: '业务' },
      { value: 'corporate_fare', label: '公司' },
      { value: 'account_balance', label: '机构' },
      { value: 'apartment', label: '部门' },
      { value: 'store', label: '分支' },
      { value: 'public', label: '全局' },
      { value: 'hub', label: '网络' },
      { value: 'folder', label: '分组' },
    ],
  },
  {
    label: '工作与流程',
    options: [
      { value: 'work', label: '工作' },
      { value: 'assignment', label: '分派' },
      { value: 'task_alt', label: '任务' },
      { value: 'approval', label: '审批' },
      { value: 'fact_check', label: '审核' },
      { value: 'dashboard', label: '仪表盘' },
      { value: 'settings', label: '设置' },
      { value: 'tune', label: '配置' },
    ],
  },
];

const ROLE_ICON_LABELS = new Map(
  ROLE_ICON_GROUPS.flatMap((group) => group.options.map((option) => [option.value, option.label])),
);

export interface RoleEditorResult {
  code: string;
  name: string;
  category: string;
  icon: string;
  color: Role['color'];
  description: string;
  dataScope: DataScope;
  isActive: boolean;
}

const DEFAULT_DATA_SCOPE: DataScope = 'self';

@Component({
  selector: 'app-role-editor-dialog',
  imports: [ReactiveFormsModule, MatDialogModule, MatIconModule, MatSelectModule],
  templateUrl: './role-editor.dialog.html',
  styleUrl: '../../core/editor-dialog.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class RoleEditorDialog {
  readonly role = inject<Role | null>(MAT_DIALOG_DATA);
  private readonly dialogRef = inject(MatDialogRef<RoleEditorDialog>);
  private readonly fb = inject(FormBuilder);
  readonly iconGroups = ROLE_ICON_GROUPS;

  readonly form = this.fb.nonNullable.group({
    code: [
      { value: this.role?.code ?? '', disabled: Boolean(this.role) },
      [Validators.required, Validators.pattern(/^[a-z][a-z0-9_]{2,63}$/)],
    ],
    name: [this.role?.name ?? '', [Validators.required, Validators.maxLength(128)]],
    category: [this.role?.category ?? '通用', [Validators.required]],
    icon: [this.role?.icon ?? 'badge', [Validators.required]],
    color: [this.role?.color ?? ('neutral' as Role['color'])],
    description: [this.role?.description ?? ''],
    dataScope: [
      {
        value: this.role?.dataScope ?? DEFAULT_DATA_SCOPE,
        disabled: this.role?.code === 'super_admin',
      },
    ],
    isActive: [{ value: this.role?.isActive ?? true, disabled: this.role?.code === 'super_admin' }],
  });

  iconLabel(icon: string): string {
    return ROLE_ICON_LABELS.get(icon) ?? '当前图标';
  }

  isKnownIcon(icon: string): boolean {
    return ROLE_ICON_LABELS.has(icon);
  }

  submit(): void {
    if (this.form.invalid) {
      this.form.markAllAsTouched();
      return;
    }
    this.dialogRef.close(this.form.getRawValue() satisfies RoleEditorResult);
  }
}
