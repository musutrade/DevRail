import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import {
  FormField,
  form,
  maxLength,
  minLength,
  required,
  submit,
  validate,
} from '@angular/forms/signals';
import { MatDialogModule, MatDialogRef } from '@angular/material/dialog';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { apiErrorMessage } from './api-error';
import { AuthService } from './auth.service';
import { authenticatorCodeError } from './authenticator-code';
import { AuthenticatorCodeField } from './authenticator-code-field';

@Component({
  selector: 'app-change-password-dialog',
  imports: [
    AuthenticatorCodeField,
    FormField,
    MatDialogModule,
    MatIconModule,
    MatProgressSpinnerModule,
  ],
  template: `
    <form
      class="editor-dialog compact-dialog"
      (submit)="submitPassword(); $event.preventDefault()"
      novalidate
    >
      <div class="dialog-title-row">
        <mat-icon>password</mat-icon>
        <h2>修改密码</h2>
      </div>
      <p class="dialog-message">修改当前账户的登录密码。</p>

      <div class="password-fields">
        <div class="form-field">
          <label for="current-password">当前密码</label>
          <div class="password-input">
            <input
              id="current-password"
              [type]="showCurrentPassword() ? 'text' : 'password'"
              [formField]="passwordForm.currentPassword"
              autocomplete="current-password"
            />
            <button
              type="button"
              class="password-toggle"
              (click)="showCurrentPassword.update(toggle)"
              [attr.aria-label]="showCurrentPassword() ? '隐藏当前密码' : '显示当前密码'"
              [title]="showCurrentPassword() ? '隐藏当前密码' : '显示当前密码'"
            >
              <mat-icon>{{ showCurrentPassword() ? 'visibility_off' : 'visibility' }}</mat-icon>
            </button>
          </div>
          @if (
            passwordForm.currentPassword().touched() &&
            passwordForm.currentPassword().errors().length
          ) {
            <small>{{ passwordForm.currentPassword().errors()[0].message }}</small>
          }
        </div>

        <div class="form-field">
          <label for="new-password">新密码</label>
          <div class="password-input">
            <input
              id="new-password"
              [type]="showNewPassword() ? 'text' : 'password'"
              [formField]="passwordForm.newPassword"
              autocomplete="new-password"
            />
            <button
              type="button"
              class="password-toggle"
              (click)="showNewPassword.update(toggle)"
              [attr.aria-label]="showNewPassword() ? '隐藏新密码' : '显示新密码'"
              [title]="showNewPassword() ? '隐藏新密码' : '显示新密码'"
            >
              <mat-icon>{{ showNewPassword() ? 'visibility_off' : 'visibility' }}</mat-icon>
            </button>
          </div>
          @if (passwordForm.newPassword().touched() && passwordForm.newPassword().errors().length) {
            <small>{{ passwordForm.newPassword().errors()[0].message }}</small>
          }
        </div>

        <div class="form-field">
          <label for="confirm-password">确认新密码</label>
          <div class="password-input">
            <input
              id="confirm-password"
              [type]="showConfirmPassword() ? 'text' : 'password'"
              [formField]="passwordForm.confirmPassword"
              autocomplete="new-password"
            />
            <button
              type="button"
              class="password-toggle"
              (click)="showConfirmPassword.update(toggle)"
              [attr.aria-label]="showConfirmPassword() ? '隐藏确认密码' : '显示确认密码'"
              [title]="showConfirmPassword() ? '隐藏确认密码' : '显示确认密码'"
            >
              <mat-icon>{{ showConfirmPassword() ? 'visibility_off' : 'visibility' }}</mat-icon>
            </button>
          </div>
          @if (
            passwordForm.confirmPassword().touched() &&
            passwordForm.confirmPassword().errors().length
          ) {
            <small>{{ passwordForm.confirmPassword().errors()[0].message }}</small>
          }
        </div>

        @if (mfaLoading()) {
          <div class="dialog-inline-status" role="status">
            <mat-progress-spinner diameter="18" mode="indeterminate" />
            <span>正在读取账户安全状态</span>
          </div>
        } @else if (mfaError()) {
          <div class="dialog-error" role="alert">{{ mfaError() }}</div>
        } @else if (requiresEnrollment()) {
          <div class="dialog-warning" role="alert">
            当前账户必须先在“账号安全”中启用身份验证器，才能修改密码。
          </div>
        } @else if (requiresTotp()) {
          <app-authenticator-code-field controlId="totp-code" [formField]="passwordForm.totpCode" />
        }
      </div>

      @if (errorMessage()) {
        <div class="dialog-error" role="alert">{{ errorMessage() }}</div>
      }

      <div class="dialog-actions">
        <button type="button" class="btn-outline" mat-dialog-close [disabled]="submitting()">
          取消
        </button>
        <button
          type="submit"
          class="btn-primary"
          [disabled]="
            passwordForm().invalid() ||
            submitting() ||
            mfaLoading() ||
            !!mfaError() ||
            requiresEnrollment()
          "
          [attr.aria-busy]="submitting()"
        >
          @if (submitting()) {
            <mat-progress-spinner diameter="18" mode="indeterminate" />
          }
          <span>{{ submitting() ? '正在保存' : '保存修改' }}</span>
        </button>
      </div>
    </form>
  `,
  styleUrls: ['./editor-dialog.scss', './change-password.dialog.scss'],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ChangePasswordDialog {
  private readonly auth = inject(AuthService);
  private readonly dialogRef = inject(MatDialogRef<ChangePasswordDialog>);

  readonly submitting = signal(false);
  readonly errorMessage = signal('');
  readonly mfaLoading = signal(true);
  readonly mfaError = signal('');
  readonly mfaStatus = this.auth.mfaStatus;
  readonly requiresTotp = computed(() => this.mfaStatus()?.totpEnabled === true);
  readonly requiresEnrollment = computed(
    () => this.mfaStatus()?.required === true && !this.requiresTotp(),
  );
  readonly showCurrentPassword = signal(false);
  readonly showNewPassword = signal(false);
  readonly showConfirmPassword = signal(false);
  readonly passwordModel = signal({
    currentPassword: '',
    newPassword: '',
    confirmPassword: '',
    totpCode: '',
  });
  readonly passwordForm = form(this.passwordModel, (path) => {
    required(path.currentPassword, { message: '请输入当前密码' });
    required(path.newPassword, { message: '请输入新密码' });
    minLength(path.newPassword, 12, { message: '新密码至少需要 12 个字符' });
    maxLength(path.newPassword, 128, { message: '新密码不能超过 128 个字符' });
    validate(path.newPassword, ({ value, valueOf }) =>
      value().length > 0 && value() === valueOf(path.currentPassword)
        ? { kind: 'samePassword', message: '新密码不能与当前密码相同' }
        : undefined,
    );
    required(path.confirmPassword, { message: '请再次输入新密码' });
    validate(path.totpCode, ({ value }) => authenticatorCodeError(value(), this.requiresTotp()));
    validate(path.confirmPassword, ({ value, valueOf }) =>
      value().length > 0 && value() !== valueOf(path.newPassword)
        ? { kind: 'passwordMismatch', message: '两次输入的新密码不一致' }
        : undefined,
    );
  });

  readonly toggle = (visible: boolean): boolean => !visible;

  constructor() {
    void this.loadMfaStatus();
  }

  submitPassword(): void {
    if (this.submitting() || this.mfaLoading() || this.mfaError() || this.requiresEnrollment()) {
      return;
    }
    submit(this.passwordForm, async () => {
      this.submitting.set(true);
      this.errorMessage.set('');
      try {
        const { currentPassword, newPassword, totpCode } = this.passwordModel();
        const stepUp = await this.auth.issueStepUp(
          'auth.password.change',
          currentPassword,
          this.requiresTotp() ? totpCode : undefined,
        );
        await this.auth.changePassword({ currentPassword, newPassword }, stepUp.token);
        this.dialogRef.close(true);
      } catch (error) {
        this.errorMessage.set(apiErrorMessage(error, '密码修改失败，请稍后重试'));
        this.passwordModel.update((model) => ({ ...model, totpCode: '' }));
      } finally {
        this.submitting.set(false);
      }
    });
  }

  private async loadMfaStatus(): Promise<void> {
    try {
      await this.auth.ensureMfaStatus();
    } catch (error) {
      this.mfaError.set(apiErrorMessage(error, '账户安全状态读取失败'));
    } finally {
      this.mfaLoading.set(false);
    }
  }
}
