import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { FormField, form, maxLength, required, submit, validate } from '@angular/forms/signals';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { MatDialog, MatDialogModule } from '@angular/material/dialog';
import { firstValueFrom } from 'rxjs';
import { Router } from '@angular/router';
import { apiErrorMessage } from '../../core/api-error';
import { AuthService } from '../../core/auth.service';
import { authenticatorCodeError } from '../../core/authenticator-code';
import { AuthenticatorCodeField } from '../../core/authenticator-code-field';
import { MfaStatusResponse } from '../../generated/api/models/mfa-status-response';
import { StepUpDialog, StepUpCredentials } from '../../core/step-up.dialog';

@Component({
  selector: 'app-security',
  imports: [
    AuthenticatorCodeField,
    FormField,
    MatDialogModule,
    MatIconModule,
    MatProgressSpinnerModule,
  ],
  templateUrl: './security.html',
  styleUrl: './security.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SecurityPage {
  readonly status = signal<MfaStatusResponse | null>(null);
  readonly loading = signal(true);
  readonly submitting = signal(false);
  readonly error = signal<string | null>(null);
  readonly message = signal<string | null>(null);
  readonly recoveryCodes = signal<readonly string[]>([]);
  readonly auth = inject(AuthService);

  private readonly router = inject(Router);
  private readonly dialog = inject(MatDialog);

  readonly registrationModel = signal({ name: '', currentPassword: '', totpCode: '' });
  readonly registrationForm = form(this.registrationModel, (path) => {
    required(path.name, { message: '请输入通行密钥名称' });
    maxLength(path.name, 80, { message: '名称不能超过 80 个字符' });
    required(path.currentPassword, { message: '请输入当前密码' });
    validate(path.totpCode, ({ value }) => authenticatorCodeError(value(), true));
  });
  readonly recoveryModel = signal({ currentPassword: '', totpCode: '' });
  readonly recoveryForm = form(this.recoveryModel, (path) => {
    required(path.currentPassword, { message: '请输入当前密码' });
    validate(path.totpCode, ({ value }) => authenticatorCodeError(value(), true));
  });

  constructor() {
    void this.load();
  }

  retry(): void {
    void this.load();
  }

  registerPasskey(): void {
    submit(this.registrationForm, async () => {
      await this.run(async () => {
        const model = this.registrationModel();
        this.status.set(
          await this.auth.registerPasskey(model.name, model.currentPassword, model.totpCode),
        );
        this.registrationModel.set({ name: '', currentPassword: '', totpCode: '' });
        this.message.set('通行密钥已添加');
      });
    });
  }

  regenerateRecoveryCodes(): void {
    submit(this.recoveryForm, async () => {
      await this.run(async () => {
        const response = await this.auth.regenerateRecoveryCodes(this.recoveryModel());
        this.recoveryCodes.set(response.codes);
        this.message.set(null);
      });
    });
  }

  async confirmRevoke(id: number): Promise<void> {
    const credentials: StepUpCredentials | undefined = await firstValueFrom(
      this.dialog
        .open(StepUpDialog, {
          data: {
            title: '撤销通行密钥需要再认证',
            message: '撤销后全部会话将立即失效，请验证当前密码和身份验证器验证码。',
          },
        })
        .afterClosed(),
    );
    if (!credentials) {
      return;
    }
    await this.run(async () => {
      await this.auth.revokePasskey(id, credentials);
      await this.router.navigate(['/login']);
    });
  }

  downloadRecoveryCodes(): void {
    const blob = new Blob([`${this.recoveryCodes().join('\n')}\n`], { type: 'text/plain' });
    const link = document.createElement('a');
    link.href = URL.createObjectURL(blob);
    link.download = 'arc-admin-recovery-codes.txt';
    link.click();
    URL.revokeObjectURL(link.href);
  }

  async returnToLogin(): Promise<void> {
    await this.router.navigate(['/login']);
  }

  formatDate(value: string | null | undefined): string {
    return value
      ? new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium', timeStyle: 'short' }).format(
          new Date(value),
        )
      : '尚未使用';
  }

  private async load(): Promise<void> {
    this.loading.set(true);
    this.error.set(null);
    try {
      this.status.set(await this.auth.getMfaStatus());
    } catch (error) {
      this.error.set(apiErrorMessage(error, '无法读取账号安全设置'));
    } finally {
      this.loading.set(false);
    }
  }

  private async run(action: () => Promise<void>): Promise<void> {
    this.submitting.set(true);
    this.error.set(null);
    this.message.set(null);
    try {
      await action();
    } catch (error) {
      this.error.set(apiErrorMessage(error, '安全设置操作失败，请重试'));
    } finally {
      this.submitting.set(false);
    }
  }
}
