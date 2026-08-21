import { provideZonelessChangeDetection } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { MAT_DIALOG_DATA, MatDialogRef } from '@angular/material/dialog';
import { Role } from '../../core/models';
import { RoleEditorDialog } from './role-editor.dialog';

describe('RoleEditorDialog', () => {
  let fixture: ComponentFixture<RoleEditorDialog>;
  let dialog: RoleEditorDialog;
  let dialogRef: { close: ReturnType<typeof vi.fn> };

  async function createDialog(role: Role | null = null): Promise<void> {
    dialogRef = { close: vi.fn() };
    TestBed.resetTestingModule();
    await TestBed.configureTestingModule({
      imports: [RoleEditorDialog],
      providers: [
        provideZonelessChangeDetection(),
        { provide: MAT_DIALOG_DATA, useValue: role },
        { provide: MatDialogRef, useValue: dialogRef },
      ],
    }).compileComponents();
    fixture = TestBed.createComponent(RoleEditorDialog);
    dialog = fixture.componentInstance;
    await fixture.whenStable();
  }

  it('shows a friendly label and preview for the default icon', async () => {
    await createDialog();

    const selection = fixture.nativeElement.querySelector('.icon-select-value');
    expect(selection.textContent).toContain('badge');
    expect(selection.textContent).toContain('角色徽章');
  });

  it('preserves an existing icon outside the curated choices', async () => {
    await createDialog({ icon: 'custom_icon' } as Role);

    expect(dialog.form.controls.icon.value).toBe('custom_icon');
    expect(dialog.iconLabel('custom_icon')).toBe('当前图标');
  });

  it('keeps the built-in super administrator status read-only', async () => {
    await createDialog({ code: 'super_admin', dataScope: 'all', isActive: true } as Role);

    expect(dialog.form.controls.isActive.disabled).toBe(true);
    expect(dialog.form.controls.dataScope.disabled).toBe(true);
    expect(dialog.form.getRawValue().isActive).toBe(true);
    expect(dialog.form.getRawValue().dataScope).toBe('all');
  });

  it('submits the selected icon value', async () => {
    await createDialog();
    dialog.form.patchValue({
      code: 'auditor',
      name: 'Auditor',
      icon: 'fact_check',
    });

    dialog.submit();

    expect(dialogRef.close).toHaveBeenCalledWith(
      expect.objectContaining({ icon: 'fact_check', dataScope: 'self' }),
    );
  });
});
