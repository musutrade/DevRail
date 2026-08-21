import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { FormBuilder, ReactiveFormsModule } from '@angular/forms';
import { MAT_DIALOG_DATA, MatDialogModule, MatDialogRef } from '@angular/material/dialog';
import { MatIconModule } from '@angular/material/icon';
import { Role } from '../../core/models';

@Component({
  selector: 'app-role-selection-dialog',
  imports: [ReactiveFormsModule, MatDialogModule, MatIconModule],
  template: `
    <form class="editor-dialog compact-dialog" [formGroup]="form" (ngSubmit)="submit()">
      <div class="dialog-title-row">
        <mat-icon>rule</mat-icon>
        <h2>修改角色</h2>
      </div>
      <p class="dialog-message">所选角色将替换用户现有的角色分配。</p>
      <div class="form-field">
        <label for="selected-roles">角色</label>
        <select id="selected-roles" formControlName="roleIds" multiple>
          @for (role of roles; track role.id) {
            <option [value]="role.id">{{ role.name }}</option>
          }
        </select>
      </div>
      <div class="dialog-actions">
        <button type="button" class="btn-outline" mat-dialog-close>取消</button>
        <button type="submit" class="btn-primary">应用角色</button>
      </div>
    </form>
  `,
  styleUrl: '../../core/editor-dialog.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class RoleSelectionDialog {
  readonly roles = inject<Role[]>(MAT_DIALOG_DATA);
  private readonly dialogRef = inject(MatDialogRef<RoleSelectionDialog>);
  private readonly fb = inject(FormBuilder);
  readonly form = this.fb.nonNullable.group({ roleIds: [[] as string[]] });

  submit(): void {
    this.dialogRef.close(this.form.getRawValue().roleIds);
  }
}
