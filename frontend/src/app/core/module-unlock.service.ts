import { Injectable, inject } from '@angular/core';
import { MatDialog } from '@angular/material/dialog';
import { MatSnackBar } from '@angular/material/snack-bar';
import { firstValueFrom } from 'rxjs';
import { ModuleUnlockScopeSchema } from '../generated/api/models/module-unlock-scope-schema';
import { apiErrorMessage } from './api-error';
import { AuthService } from './auth.service';
import { StepUpCredentials, StepUpDialog } from './step-up.dialog';

@Injectable({ providedIn: 'root' })
export class ModuleUnlockService {
  private readonly auth = inject(AuthService);
  private readonly dialog = inject(MatDialog);
  private readonly snackBar = inject(MatSnackBar);
  private readonly expiresAt = new Map<ModuleUnlockScopeSchema, number>();
  private userId: number | undefined;

  async ensure(module: ModuleUnlockScopeSchema, moduleLabel: string): Promise<boolean> {
    this.resetForCurrentUser();
    if ((this.expiresAt.get(module) ?? 0) > Date.now() + 1000) {
      return true;
    }

    try {
      const status = await this.auth.getModuleUnlockStatus(module);
      if (this.remember(module, status.unlocked, status.expiresAt)) {
        return true;
      }
    } catch (error) {
      this.showError(error);
      return false;
    }

    const credentials: StepUpCredentials | undefined = await firstValueFrom(
      this.dialog
        .open(StepUpDialog, {
          data: {
            title: `${moduleLabel}需要身份验证`,
            message: '验证成功后，5 分钟内可继续完成本模块的常规操作。',
          },
        })
        .afterClosed(),
    );
    if (!credentials) {
      return false;
    }

    try {
      const status = await this.auth.unlockModule(
        module,
        credentials.currentPassword,
        credentials.totpCode,
      );
      return this.remember(module, status.unlocked, status.expiresAt);
    } catch (error) {
      this.showError(error);
      return false;
    }
  }

  private resetForCurrentUser(): void {
    const currentUserId = this.auth.currentUser()?.id;
    if (currentUserId !== this.userId) {
      this.expiresAt.clear();
      this.userId = currentUserId;
    }
  }

  private remember(
    module: ModuleUnlockScopeSchema,
    unlocked: boolean,
    expiresAt?: string | null,
  ): boolean {
    const expiration = expiresAt ? Date.parse(expiresAt) : 0;
    if (!unlocked || !Number.isFinite(expiration) || expiration <= Date.now()) {
      this.expiresAt.delete(module);
      return false;
    }
    this.expiresAt.set(module, expiration);
    return true;
  }

  private showError(error: unknown): void {
    this.snackBar.open(apiErrorMessage(error, '模块身份验证失败'), '关闭', { duration: 5000 });
  }
}
