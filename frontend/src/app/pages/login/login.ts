import { HttpErrorResponse } from '@angular/common/http';
import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { FormField, form, required, submit, validate } from '@angular/forms/signals';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { Router } from '@angular/router';
import { apiErrorMessage } from '../../core/api-error';
import { AuthService } from '../../core/auth.service';
import { authenticatorCodeError } from '../../core/authenticator-code';
import { AuthenticatorCodeField } from '../../core/authenticator-code-field';
import { APP_CONFIG } from '../../core/runtime-config';
import { LoginResponse } from '../../generated/api/models/login-response';
import { MfaMethodSchema } from '../../generated/api/models/mfa-method-schema';

type LoginStep = 'password' | 'totp' | 'recovery' | 'recoveryCodes';

@Component({
  selector: 'app-login',
  imports: [AuthenticatorCodeField, FormField, MatIconModule, MatProgressSpinnerModule],
  templateUrl: './login.html',
  styleUrl: './login.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LoginPage {
  readonly hidePassword = signal(true);
  readonly loading = signal(false);
  readonly error = signal<string | null>(null);
  readonly step = signal<LoginStep>('password');
  readonly challengeToken = signal('');
  readonly methods = signal<readonly MfaMethodSchema[]>([]);
  readonly enrollmentSecret = signal('');
  readonly enrollmentQrCode = signal('');
  readonly recoveryCodes = signal<readonly string[]>([]);
  readonly appConfig = inject(APP_CONFIG);

  private readonly router = inject(Router);
  readonly auth = inject(AuthService);

  readonly loginModel = signal({ username: '', password: '', remember: true });
  readonly loginForm = form(this.loginModel, (path) => {
    required(path.username, { message: '请输入用户名' });
    required(path.password, { message: '请输入密码' });
  });
  readonly totpModel = signal({ code: '' });
  readonly totpForm = form(this.totpModel, (path) => {
    validate(path.code, ({ value }) => authenticatorCodeError(value(), true));
  });
  readonly recoveryModel = signal({ code: '' });
  readonly recoveryForm = form(this.recoveryModel, (path) => {
    required(path.code, { message: '请输入恢复码' });
  });

  togglePassword(): void {
    this.hidePassword.update((value) => !value);
  }

  onPasswordSubmit(): void {
    submit(this.loginForm, async () => {
      await this.run(async () => {
        const { username, password, remember } = this.loginModel();
        const response = await this.auth.login(username, password, remember);
        await this.handleLoginResponse(response);
      }, '用户名、密码或账号状态无效');
    });
  }

  onTotpSubmit(): void {
    submit(this.totpForm, async () => {
      await this.run(async () => {
        const response = await this.auth.verifyTotp(this.challengeToken(), this.totpModel().code);
        await this.handleAuthenticatedResponse(response);
      }, '验证码无效或已过期');
    });
  }

  onRecoverySubmit(): void {
    submit(this.recoveryForm, async () => {
      await this.run(async () => {
        const response = await this.auth.verifyRecoveryCode(
          this.challengeToken(),
          this.recoveryModel().code,
        );
        await this.handleAuthenticatedResponse(response);
      }, '恢复码无效或已使用');
    });
  }

  async usePasskey(): Promise<void> {
    await this.run(async () => {
      const response = await this.auth.authenticateWithPasskey(this.challengeToken());
      await this.handleAuthenticatedResponse(response);
    }, '通行密钥验证未完成');
  }

  chooseTotp(): void {
    this.error.set(null);
    this.step.set('totp');
  }

  chooseRecovery(): void {
    this.error.set(null);
    this.step.set('recovery');
  }

  restart(): void {
    this.step.set('password');
    this.challengeToken.set('');
    this.methods.set([]);
    this.enrollmentSecret.set('');
    this.enrollmentQrCode.set('');
    this.error.set(null);
  }

  downloadRecoveryCodes(): void {
    const blob = new Blob([`${this.recoveryCodes().join('\n')}\n`], { type: 'text/plain' });
    const link = document.createElement('a');
    link.href = URL.createObjectURL(blob);
    link.download = 'arc-admin-recovery-codes.txt';
    link.click();
    URL.revokeObjectURL(link.href);
  }

  async enterApplication(): Promise<void> {
    await this.router.navigate(['/']);
  }

  private async handleLoginResponse(response: LoginResponse): Promise<void> {
    if (response.status === 'authenticated') {
      await this.handleAuthenticatedResponse(response);
      return;
    }
    this.challengeToken.set(response.challengeToken ?? '');
    this.methods.set(response.methods);
    this.enrollmentSecret.set(response.totpSecret ?? '');
    this.enrollmentQrCode.set(response.totpQrCode ?? '');
    this.step.set('totp');
  }

  private async handleAuthenticatedResponse(response: LoginResponse): Promise<void> {
    if (response.recoveryCodes.length > 0) {
      this.recoveryCodes.set(response.recoveryCodes);
      this.step.set('recoveryCodes');
      return;
    }
    await this.router.navigate(['/']);
  }

  private async run(action: () => Promise<void>, unauthorizedMessage: string): Promise<void> {
    this.loading.set(true);
    this.error.set(null);
    try {
      await action();
    } catch (error) {
      this.error.set(
        error instanceof HttpErrorResponse && error.status === 401
          ? unauthorizedMessage
          : apiErrorMessage(error, '认证服务暂时不可用，请稍后重试'),
      );
    } finally {
      this.loading.set(false);
    }
  }
}
