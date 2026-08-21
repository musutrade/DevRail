import { Injectable, inject, signal } from '@angular/core';
import {
  browserSupportsWebAuthn,
  startAuthentication,
  startRegistration,
  type PublicKeyCredentialCreationOptionsJSON,
  type PublicKeyCredentialRequestOptionsJSON,
} from '@simplewebauthn/browser';
import { Api } from '../generated/api/api';
import { changeCurrentUserPassword } from '../generated/api/fn/auth/change-current-user-password';
import { finishCurrentUserPasskeyRegistration } from '../generated/api/fn/auth/finish-current-user-passkey-registration';
import { finishMfaPasskeyAuthentication } from '../generated/api/fn/auth/finish-mfa-passkey-authentication';
import { getCurrentUserMfaStatus } from '../generated/api/fn/auth/get-current-user-mfa-status';
import { getCurrentUserModuleUnlockStatus } from '../generated/api/fn/auth/get-current-user-module-unlock-status';
import { getCurrentUserPermissions } from '../generated/api/fn/auth/get-current-user-permissions';
import { getCurrentUser } from '../generated/api/fn/auth/get-current-user';
import { login as loginRequest } from '../generated/api/fn/auth/login';
import { logout as logoutRequest } from '../generated/api/fn/auth/logout';
import { issueStepUpToken } from '../generated/api/fn/auth/issue-step-up-token';
import { regenerateCurrentUserRecoveryCodes } from '../generated/api/fn/auth/regenerate-current-user-recovery-codes';
import { revokeCurrentUserPasskey } from '../generated/api/fn/auth/revoke-current-user-passkey';
import { startCurrentUserPasskeyRegistration } from '../generated/api/fn/auth/start-current-user-passkey-registration';
import { startMfaPasskeyAuthentication } from '../generated/api/fn/auth/start-mfa-passkey-authentication';
import { verifyMfaRecoveryCode } from '../generated/api/fn/auth/verify-mfa-recovery-code';
import { verifyMfaTotp } from '../generated/api/fn/auth/verify-mfa-totp';
import { unlockCurrentUserModule } from '../generated/api/fn/auth/unlock-current-user-module';
import { ChangePasswordRequest } from '../generated/api/models/change-password-request';
import { LoginResponse } from '../generated/api/models/login-response';
import { MfaFactorRevokeRequest } from '../generated/api/models/mfa-factor-revoke-request';
import { MfaStatusResponse } from '../generated/api/models/mfa-status-response';
import { ModuleUnlockScopeSchema } from '../generated/api/models/module-unlock-scope-schema';
import { ModuleUnlockStatusResponse } from '../generated/api/models/module-unlock-status-response';
import { RecoveryCodesResponse } from '../generated/api/models/recovery-codes-response';
import { StepUpResponse } from '../generated/api/models/step-up-response';
import { UserResponse } from '../generated/api/models/user-response';

export type StepUpScope =
  | 'auth.password.change'
  | 'users.sensitive'
  | 'users.roles.write'
  | 'users.delete'
  | 'roles.sensitive'
  | 'roles.permissions.write'
  | 'roles.delete'
  | 'departments.write'
  | 'departments.delete';

@Injectable({ providedIn: 'root' })
export class AuthService {
  private readonly api = inject(Api);
  private readonly userState = signal<UserResponse | null>(null);
  private readonly permissionState = signal<ReadonlySet<string>>(new Set());
  private readonly mfaStatusState = signal<MfaStatusResponse | null>(null);
  private sessionCheck: Promise<boolean> | null = null;
  private permissionCheck: Promise<void> | null = null;
  private permissionEpoch = 0;
  private mfaStatusCheck: Promise<MfaStatusResponse> | null = null;

  readonly currentUser = this.userState.asReadonly();
  readonly permissions = this.permissionState.asReadonly();
  readonly mfaStatus = this.mfaStatusState.asReadonly();

  async login(username: string, password: string, remember: boolean): Promise<LoginResponse> {
    const response = await this.api.invoke(loginRequest, {
      body: { username, password, remember },
    });
    if (response.status === 'authenticated') {
      await this.completeLogin(response);
    }
    return response;
  }

  async verifyTotp(challengeToken: string, code: string): Promise<LoginResponse> {
    const response = await this.api.invoke(verifyMfaTotp, {
      body: { challengeToken, code },
    });
    await this.completeLogin(response);
    return response;
  }

  async verifyRecoveryCode(challengeToken: string, code: string): Promise<LoginResponse> {
    const response = await this.api.invoke(verifyMfaRecoveryCode, {
      body: { challengeToken, code },
    });
    await this.completeLogin(response);
    return response;
  }

  async authenticateWithPasskey(challengeToken: string): Promise<LoginResponse> {
    const challenge = await this.api.invoke(startMfaPasskeyAuthentication, {
      body: { challengeToken },
    });
    const credential = await startAuthentication({
      optionsJSON: challenge.publicKey as PublicKeyCredentialRequestOptionsJSON,
    });
    const response = await this.api.invoke(finishMfaPasskeyAuthentication, {
      body: { challengeToken: challenge.challengeToken, credential },
    });
    await this.completeLogin(response);
    return response;
  }

  async getMfaStatus(): Promise<MfaStatusResponse> {
    const status = await this.api.invoke(getCurrentUserMfaStatus);
    this.mfaStatusState.set(status);
    return status;
  }

  ensureMfaStatus(): Promise<MfaStatusResponse> {
    const cached = this.mfaStatusState();
    if (cached) {
      return Promise.resolve(cached);
    }
    if (!this.mfaStatusCheck) {
      this.mfaStatusCheck = this.getMfaStatus().finally(() => {
        this.mfaStatusCheck = null;
      });
    }
    return this.mfaStatusCheck;
  }

  supportsPasskeys(): boolean {
    return browserSupportsWebAuthn();
  }

  async registerPasskey(
    name: string,
    currentPassword: string,
    totpCode: string,
  ): Promise<MfaStatusResponse> {
    const challenge = await this.api.invoke(startCurrentUserPasskeyRegistration, {
      body: { name, currentPassword, totpCode },
    });
    const credential = await startRegistration({
      optionsJSON: challenge.publicKey as PublicKeyCredentialCreationOptionsJSON,
    });
    const status = await this.api.invoke(finishCurrentUserPasskeyRegistration, {
      body: { challengeToken: challenge.challengeToken, credential },
    });
    this.mfaStatusState.set(status);
    return status;
  }

  async revokePasskey(id: number, request: MfaFactorRevokeRequest): Promise<void> {
    await this.api.invoke(revokeCurrentUserPasskey, { id, body: request });
    this.clearSession();
  }

  async regenerateRecoveryCodes(request: MfaFactorRevokeRequest): Promise<RecoveryCodesResponse> {
    const response = await this.api.invoke(regenerateCurrentUserRecoveryCodes, { body: request });
    this.clearSession();
    return response;
  }

  private async completeLogin(response: LoginResponse): Promise<void> {
    if (response.status !== 'authenticated' || !response.user) {
      throw new Error('认证流程尚未完成');
    }
    this.userState.set(response.user);
    try {
      await this.loadPermissions();
    } catch (error) {
      await this.logout().catch(() => undefined);
      throw error;
    }
  }

  ensureSession(): Promise<boolean> {
    if (this.userState()) {
      return this.refreshPermissions()
        .then(() => true)
        .catch(() => {
          this.clearSession();
          return false;
        });
    }
    if (!this.sessionCheck) {
      this.sessionCheck = Promise.all([
        this.api.invoke(getCurrentUser),
        this.api.invoke(getCurrentUserPermissions),
      ])
        .then(([user, permissions]) => {
          this.userState.set(user);
          this.permissionState.set(new Set(permissions.codes));
          return true;
        })
        .catch(() => {
          this.clearSession();
          return false;
        })
        .finally(() => {
          this.sessionCheck = null;
        });
    }
    return this.sessionCheck;
  }

  hasPermission(code: string): boolean {
    return this.permissionState().has(code);
  }

  hasAllPermissions(codes: readonly string[]): boolean {
    return codes.every((code) => this.permissionState().has(code));
  }

  async issueStepUp(
    scope: StepUpScope,
    currentPassword: string,
    totpCode?: string,
  ): Promise<StepUpResponse> {
    return this.api.invoke(issueStepUpToken, {
      body: {
        scope,
        currentPassword,
        ...(totpCode?.trim() ? { totpCode: totpCode.trim() } : {}),
      },
    });
  }

  getModuleUnlockStatus(module: ModuleUnlockScopeSchema): Promise<ModuleUnlockStatusResponse> {
    return this.api.invoke(getCurrentUserModuleUnlockStatus, { module });
  }

  unlockModule(
    module: ModuleUnlockScopeSchema,
    currentPassword: string,
    totpCode?: string,
  ): Promise<ModuleUnlockStatusResponse> {
    return this.api.invoke(unlockCurrentUserModule, {
      body: {
        module,
        currentPassword,
        ...(totpCode?.trim() ? { totpCode: totpCode.trim() } : {}),
      },
    });
  }

  async changePassword(request: ChangePasswordRequest, stepUpToken: string): Promise<void> {
    await this.api.invoke(changeCurrentUserPassword, {
      body: request,
      'X-Step-Up-Token': stepUpToken,
    });
    this.clearSession();
  }

  async refreshSession(): Promise<void> {
    const [user, permissions] = await Promise.all([
      this.api.invoke(getCurrentUser),
      this.api.invoke(getCurrentUserPermissions),
    ]);
    this.userState.set(user);
    this.permissionState.set(new Set(permissions.codes));
  }

  async logout(): Promise<void> {
    try {
      await this.api.invoke(logoutRequest);
    } finally {
      this.clearSession();
    }
  }

  handleUnauthorized(): void {
    this.clearSession();
  }

  refreshPermissions(): Promise<void> {
    if (!this.permissionCheck) {
      const epoch = this.permissionEpoch;
      const request = this.api
        .invoke(getCurrentUserPermissions)
        .then((response) => {
          if (epoch === this.permissionEpoch) {
            this.permissionState.set(new Set(response.codes));
          }
        })
        .finally(() => {
          if (this.permissionCheck === request) {
            this.permissionCheck = null;
          }
        });
      this.permissionCheck = request;
    }
    return this.permissionCheck;
  }

  private loadPermissions(): Promise<void> {
    return this.refreshPermissions();
  }

  private clearSession(): void {
    this.permissionEpoch += 1;
    this.permissionCheck = null;
    this.userState.set(null);
    this.permissionState.set(new Set());
    this.mfaStatusState.set(null);
    this.mfaStatusCheck = null;
  }
}
