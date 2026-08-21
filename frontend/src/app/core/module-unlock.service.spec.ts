import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MatDialog } from '@angular/material/dialog';
import { MatSnackBar } from '@angular/material/snack-bar';
import { of } from 'rxjs';
import { AuthService } from './auth.service';
import { ModuleUnlockService } from './module-unlock.service';

describe('ModuleUnlockService', () => {
  let service: ModuleUnlockService;
  let auth: {
    currentUser: ReturnType<typeof signal>;
    getModuleUnlockStatus: ReturnType<typeof vi.fn>;
    unlockModule: ReturnType<typeof vi.fn>;
  };
  let dialog: { open: ReturnType<typeof vi.fn> };

  beforeEach(() => {
    auth = {
      currentUser: signal({ id: 7 }),
      getModuleUnlockStatus: vi.fn(),
      unlockModule: vi.fn(),
    };
    dialog = { open: vi.fn() };
    TestBed.configureTestingModule({
      providers: [
        { provide: AuthService, useValue: auth },
        { provide: MatDialog, useValue: dialog },
        { provide: MatSnackBar, useValue: { open: vi.fn() } },
      ],
    });
    service = TestBed.inject(ModuleUnlockService);
  });

  it('reuses an active server-side unlock without opening a dialog', async () => {
    auth.getModuleUnlockStatus.mockResolvedValue({
      module: 'users',
      unlocked: true,
      expiresAt: new Date(Date.now() + 300_000).toISOString(),
    });

    await expect(service.ensure('users', '用户管理')).resolves.toBe(true);
    await expect(service.ensure('users', '用户管理')).resolves.toBe(true);

    expect(auth.getModuleUnlockStatus).toHaveBeenCalledTimes(1);
    expect(dialog.open).not.toHaveBeenCalled();
  });

  it('prompts once and stores a newly issued unlock', async () => {
    auth.getModuleUnlockStatus.mockResolvedValue({ module: 'roles', unlocked: false });
    auth.unlockModule.mockResolvedValue({
      module: 'roles',
      unlocked: true,
      expiresAt: new Date(Date.now() + 300_000).toISOString(),
    });
    dialog.open.mockReturnValue({
      afterClosed: () => of({ currentPassword: 'current-password', totpCode: '' }),
    });

    await expect(service.ensure('roles', '角色管理')).resolves.toBe(true);
    await expect(service.ensure('roles', '角色管理')).resolves.toBe(true);

    expect(auth.unlockModule).toHaveBeenCalledWith('roles', 'current-password', '');
    expect(dialog.open).toHaveBeenCalledTimes(1);
  });
});
