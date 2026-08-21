import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MAT_DIALOG_DATA, MatDialogModule } from '@angular/material/dialog';
import { MatIconModule } from '@angular/material/icon';

export interface ConfirmDialogData {
  title: string;
  message: string;
  confirmLabel: string;
  danger?: boolean;
}

@Component({
  selector: 'app-confirm-dialog',
  imports: [MatDialogModule, MatIconModule],
  template: `
    <div class="editor-dialog compact-dialog">
      <div class="dialog-title-row">
        <mat-icon>{{ data.danger ? 'warning' : 'help' }}</mat-icon>
        <h2>{{ data.title }}</h2>
      </div>
      <p class="dialog-message">{{ data.message }}</p>
      <div class="dialog-actions">
        <button type="button" class="btn-outline" mat-dialog-close>取消</button>
        <button
          type="button"
          class="btn-primary"
          [class.danger-action]="data.danger"
          [mat-dialog-close]="true"
        >
          {{ data.confirmLabel }}
        </button>
      </div>
    </div>
  `,
  styleUrl: './editor-dialog.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ConfirmDialog {
  readonly data = inject<ConfirmDialogData>(MAT_DIALOG_DATA);
}
