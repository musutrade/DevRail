import { HttpErrorResponse } from '@angular/common/http';
import { signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { MatDialogRef } from '@angular/material/dialog';
import { AuthService } from './auth.service';
import { ChangePasswordDialog } from './change-password.dialog';

describe('ChangePasswordDialog', () => {
  let component: ChangePasswordDialog;
  let fixture: ComponentFixture<ChangePasswordDialog>;
  let auth: {
    changePassword: ReturnType<typeof vi.fn>;
    ensureMfaStatus: ReturnType<typeof vi.fn>;
    issueStepUp: ReturnType<typeof vi.fn>;
    mfaStatus: ReturnType<typeof signal>;
  };
  let dialogRef: { close: ReturnType<typeof vi.fn> };

  beforeEach(async () => {
    const mfaStatus = signal({
      passkeys: [],
      recoveryCodesRemaining: 0,
      required: false,
      totpEnabled: true,
    });
    auth = {
      changePassword: vi.fn(),
      ensureMfaStatus: vi.fn().mockResolvedValue(mfaStatus()),
      issueStepUp: vi.fn(),
      mfaStatus,
    };
    dialogRef = { close: vi.fn() };
    TestBed.configureTestingModule({
      providers: [
        { provide: AuthService, useValue: auth },
        { provide: MatDialogRef, useValue: dialogRef },
      ],
    });
    fixture = TestBed.createComponent(ChangePasswordDialog);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('rejects a confirmation that does not match the new password', async () => {
    component.passwordModel.set({
      currentPassword: 'current-password',
      newPassword: 'updated-password',
      confirmPassword: 'different-password',
      totpCode: '123456',
    });
    await fixture.whenStable();

    expect(component.passwordForm.confirmPassword().invalid()).toBe(true);
    expect(component.passwordForm.confirmPassword().errors()[0].message).toBe(
      '两次输入的新密码不一致',
    );
  });

  it('requires the same six-digit authenticator code as the login flow', async () => {
    component.passwordModel.set({
      currentPassword: 'current-password',
      newPassword: 'updated-password',
      confirmPassword: 'updated-password',
      totpCode: '12345',
    });
    await fixture.whenStable();

    expect(component.passwordForm.totpCode().invalid()).toBe(true);
    expect(component.passwordForm.totpCode().errors()[0].message).toBe('验证码应为 6 位数字');
  });

  it('does not require or display a code for users without an authenticator', async () => {
    auth.mfaStatus.set({
      passkeys: [],
      recoveryCodesRemaining: 0,
      required: false,
      totpEnabled: false,
    });
    component.passwordModel.set({
      currentPassword: 'current-password',
      newPassword: 'updated-password',
      confirmPassword: 'updated-password',
      totpCode: '',
    });
    fixture.detectChanges();
    await fixture.whenStable();

    expect(component.passwordForm.totpCode().valid()).toBe(true);
    expect(fixture.nativeElement.querySelector('#totp-code')).toBeNull();
  });

  it('submits valid passwords and closes the dialog', async () => {
    auth.changePassword.mockResolvedValue(undefined);
    auth.issueStepUp.mockResolvedValue({ token: 'step-up-token' });
    component.passwordModel.set({
      currentPassword: 'current-password',
      newPassword: 'updated-password',
      confirmPassword: 'updated-password',
      totpCode: '123456',
    });

    component.submitPassword();

    await vi.waitFor(() => {
      expect(auth.issueStepUp).toHaveBeenCalledWith(
        'auth.password.change',
        'current-password',
        '123456',
      );
      expect(auth.changePassword).toHaveBeenCalledWith(
        {
          currentPassword: 'current-password',
          newPassword: 'updated-password',
        },
        'step-up-token',
      );
      expect(dialogRef.close).toHaveBeenCalledWith(true);
    });
  });

  it('shows the API error and keeps the dialog open', async () => {
    auth.issueStepUp.mockRejectedValue(
      new HttpErrorResponse({
        status: 422,
        error: { error: { message: '身份验证器验证码不正确' } },
      }),
    );
    component.passwordModel.set({
      currentPassword: 'current-password',
      newPassword: 'updated-password',
      confirmPassword: 'updated-password',
      totpCode: '000000',
    });

    component.submitPassword();

    await vi.waitFor(() => {
      expect(component.errorMessage()).toBe('身份验证器验证码不正确');
      expect(component.submitting()).toBe(false);
      expect(component.passwordModel().totpCode).toBe('');
    });
    expect(auth.changePassword).not.toHaveBeenCalled();
    expect(dialogRef.close).not.toHaveBeenCalled();
  });
});
