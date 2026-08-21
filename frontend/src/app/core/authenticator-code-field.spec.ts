import { Component, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { FormField, form, validate } from '@angular/forms/signals';
import { authenticatorCodeError } from './authenticator-code';
import { AuthenticatorCodeField } from './authenticator-code-field';

@Component({
  imports: [AuthenticatorCodeField, FormField],
  template: ` <app-authenticator-code-field controlId="test-code" [formField]="codeForm.code" /> `,
})
class TestHost {
  readonly model = signal({ code: '' });
  readonly codeForm = form(this.model, (path) => {
    validate(path.code, ({ value }) => authenticatorCodeError(value(), true));
  });
}

describe('AuthenticatorCodeField', () => {
  let fixture: ComponentFixture<TestHost>;
  let host: TestHost;

  beforeEach(async () => {
    fixture = TestBed.createComponent(TestHost);
    host = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('keeps only the first six digits and updates the form model', async () => {
    const input: HTMLInputElement = fixture.nativeElement.querySelector('#test-code');
    input.value = '12a34567';
    input.dispatchEvent(new Event('input'));
    await fixture.whenStable();

    expect(input.value).toBe('123456');
    expect(host.model().code).toBe('123456');
  });

  it('shows the shared validation error after the field is touched', async () => {
    const input: HTMLInputElement = fixture.nativeElement.querySelector('#test-code');
    input.value = '12345';
    input.dispatchEvent(new Event('input'));
    input.dispatchEvent(new Event('blur'));
    await fixture.whenStable();

    expect(input.getAttribute('aria-invalid')).toBe('true');
    expect(fixture.nativeElement.querySelector('[role="alert"]').textContent).toContain(
      '验证码应为 6 位数字',
    );
  });
});
