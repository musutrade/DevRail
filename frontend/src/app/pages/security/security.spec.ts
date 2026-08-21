import { provideZonelessChangeDetection } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { MatDialog } from '@angular/material/dialog';
import { Router } from '@angular/router';
import { AuthService } from '../../core/auth.service';
import { MfaStatusResponse } from '../../generated/api/models/mfa-status-response';
import { SecurityPage } from './security';

const STATUS: MfaStatusResponse = {
  passkeys: [],
  recoveryCodesRemaining: 10,
  required: true,
  totpEnabled: true,
};

describe('SecurityPage', () => {
  let fixture: ComponentFixture<SecurityPage>;
  let page: SecurityPage;
  let getMfaStatus: ReturnType<typeof vi.fn>;

  beforeEach(async () => {
    getMfaStatus = vi
      .fn<() => Promise<MfaStatusResponse>>()
      .mockRejectedValueOnce(new Error('temporary failure'))
      .mockResolvedValueOnce(STATUS);

    await TestBed.configureTestingModule({
      imports: [SecurityPage],
      providers: [
        provideZonelessChangeDetection(),
        {
          provide: AuthService,
          useValue: {
            getMfaStatus,
            supportsPasskeys: () => false,
          },
        },
        { provide: MatDialog, useValue: { open: vi.fn() } },
        { provide: Router, useValue: { navigate: vi.fn() } },
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(SecurityPage);
    page = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('clears a load error after a successful retry', async () => {
    expect(page.status()).toBeNull();
    expect(page.error()).toBeTruthy();

    page.retry();
    await fixture.whenStable();

    expect(getMfaStatus).toHaveBeenCalledTimes(2);
    expect(page.status()).toEqual(STATUS);
    expect(page.error()).toBeNull();
  });
});
