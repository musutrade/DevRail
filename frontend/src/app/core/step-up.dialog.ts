import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { FormField, form, required, submit, validate } from '@angular/forms/signals';
import { MAT_DIALOG_DATA, MatDialogModule, MatDialogRef } from '@angular/material/dialog';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { apiErrorMessage } from './api-error';
import { AuthService } from './auth.service';
import { authenticatorCodeError } from './authenticator-code';
import { AuthenticatorCodeField } from './authenticator-code-field';

export interface StepUpDialogData {
  title: string;
  message: string;
}

export interface StepUpCredentials {
  currentPassword: string;
  totpCode: string;
}

@Component({
  selector: 'app-step-up-dialog',
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
      (submit)="confirm(); $event.preventDefault()"
      novalidate
    >
      <div class="dialog-title-row">
        <mat-icon>verified_user</mat-icon>
        <h2>{{ data.title }}</h2>
      </div>
      <p class="dialog-message">{{ data.message }}</p>

      <div class="form-grid">
        <div class="form-field">
          <label for="step-up-current-password">当前密码</label>
          <input
            id="step-up-current-password"
            type="password"
            [formField]="stepUpForm.currentPassword"
            autocomplete="current-password"
          />
          @if (
            stepUpForm.currentPassword().touched() && stepUpForm.currentPassword().errors().length
          ) {
            <small>{{ stepUpForm.currentPassword().errors()[0].message }}</small>
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
            当前账户必须先在“账号安全”中启用身份验证器，才能继续此操作。
          </div>
        } @else if (requiresTotp()) {
          <app-authenticator-code-field
            controlId="step-up-totp"
            [formField]="stepUpForm.totpCode"
          />
        }
      </div>

      <div class="dialog-actions">
        <button type="button" class="btn-outline" mat-dialog-close [disabled]="submitting()">
          取消
        </button>
        <button
          type="submit"
          class="btn-primary"
          [disabled]="
            stepUpForm().invalid() ||
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
          <span>继续</span>
        </button>
      </div>
    </form>
  `,
  styleUrls: ['./editor-dialog.scss', './step-up.dialog.scss'],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class StepUpDialog {
  readonly data = inject<StepUpDialogData>(MAT_DIALOG_DATA);
  private readonly dialogRef = inject(MatDialogRef<StepUpDialog>);
  private readonly auth = inject(AuthService);
  readonly submitting = signal(false);
  readonly mfaLoading = signal(true);
  readonly mfaError = signal('');
  readonly mfaStatus = this.auth.mfaStatus;
  readonly requiresTotp = computed(() => this.mfaStatus()?.totpEnabled === true);
  readonly requiresEnrollment = computed(
    () => this.mfaStatus()?.required === true && !this.requiresTotp(),
  );
  readonly stepUpModel = signal({ currentPassword: '', totpCode: '' });
  readonly stepUpForm = form(this.stepUpModel, (path) => {
    required(path.currentPassword, { message: '请输入当前密码' });
    validate(path.totpCode, ({ value }) => authenticatorCodeError(value(), this.requiresTotp()));
  });

  constructor() {
    void this.loadMfaStatus();
  }

  confirm(): void {
    if (this.submitting() || this.mfaLoading() || this.mfaError() || this.requiresEnrollment()) {
      return;
    }
    submit(this.stepUpForm, async () => {
      this.submitting.set(true);
      this.dialogRef.close(this.stepUpModel());
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
