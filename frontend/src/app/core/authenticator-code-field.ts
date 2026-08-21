import { ChangeDetectionStrategy, Component, computed, input, model, output } from '@angular/core';
import { FormValueControl, ValidationError, WithOptionalFieldTree } from '@angular/forms/signals';
import { MatIconModule } from '@angular/material/icon';

@Component({
  selector: 'app-authenticator-code-field',
  imports: [MatIconModule],
  host: {
    '[class.login-appearance]': "appearance() === 'login'",
  },
  template: `
    <div class="authenticator-code-field">
      <label [for]="controlId()">{{ label() }}</label>
      <div class="input-wrap" [class.input-error]="touched() && invalid()">
        <mat-icon>verified_user</mat-icon>
        <input
          [id]="controlId()"
          type="text"
          inputmode="numeric"
          autocomplete="one-time-code"
          placeholder="000000"
          maxlength="6"
          [value]="value()"
          [disabled]="disabled()"
          [readOnly]="readonly()"
          [attr.aria-invalid]="touched() && invalid()"
          [attr.aria-describedby]="touched() && errors().length ? errorId() : null"
          (input)="updateValue($event)"
          (blur)="touch.emit()"
        />
      </div>
      @if (touched() && errors().length) {
        <small [id]="errorId()" role="alert">{{ errors()[0].message }}</small>
      }
    </div>
  `,
  styles: `
    :host {
      display: block;
      min-width: 0;
    }

    .authenticator-code-field {
      display: grid;
      gap: 6px;
    }

    label {
      color: var(--ui-color-text-secondary);
      font-size: 13px;
    }

    .input-wrap {
      position: relative;
    }

    mat-icon {
      position: absolute;
      top: 50%;
      left: 12px;
      width: 20px;
      height: 20px;
      color: var(--ui-color-text-tertiary);
      font-size: 20px;
      line-height: 20px;
      transform: translateY(-50%);
      pointer-events: none;
    }

    input {
      box-sizing: border-box;
      width: 100%;
      min-width: 0;
      min-height: 40px;
      padding: 8px 11px 8px 41px;
      border: 1px solid var(--ui-color-border);
      border-radius: var(--ui-radius-md);
      background: var(--ui-color-surface-panel);
      color: var(--ui-color-text-primary);
      font: inherit;
    }

    input:focus {
      border-color: var(--ui-color-primary);
      box-shadow: var(--ui-focus-ring);
    }

    input:disabled {
      opacity: 0.65;
      cursor: not-allowed;
    }

    .input-error input {
      border-color: var(--mat-sys-error);
    }

    .input-error input:focus {
      box-shadow: var(--ui-error-focus-ring);
    }

    small {
      color: var(--mat-sys-error);
      font-size: 12px;
    }

    :host(.login-appearance) .authenticator-code-field {
      gap: 8px;
    }

    :host(.login-appearance) label {
      color: var(--ui-color-text-primary);
      font-size: 14px;
      font-weight: 500;
    }

    :host(.login-appearance) input {
      min-height: 42px;
      padding: 10px 12px 10px 40px;
      border-radius: var(--ui-radius-lg);
      background: transparent;
      font-size: 14px;
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AuthenticatorCodeField implements FormValueControl<string> {
  readonly value = model('');
  readonly controlId = input.required<string>();
  readonly label = input('身份验证器验证码');
  readonly appearance = input<'standard' | 'login'>('standard');
  readonly disabled = input(false);
  readonly readonly = input(false);
  readonly invalid = input(false);
  readonly touched = input(false);
  readonly errors = input<readonly WithOptionalFieldTree<ValidationError>[]>([]);
  readonly touch = output<void>();

  protected readonly errorId = computed(() => `${this.controlId()}-error`);

  protected updateValue(event: Event): void {
    const inputElement = event.target as HTMLInputElement;
    const code = inputElement.value.replace(/\D/g, '').slice(0, 6);
    inputElement.value = code;
    this.value.set(code);
  }
}
