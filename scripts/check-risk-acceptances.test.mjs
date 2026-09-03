import assert from 'node:assert/strict';
import test from 'node:test';
import { validateRiskAcceptance } from './check-risk-acceptances.mjs';

const deny = 'ignore = [{ id = "RUSTSEC-2023-0071" }]';
const workflow = 'ignore: RUSTSEC-2023-0071';
const record = `
# RUSTSEC-2023-0071 Risk Acceptance
Owner: DevRail maintainers
Review date: 2026-12-31
web-push -> jwt-simple -> superboring -> rsa
`;

test('risk acceptance passes before UTC expiry', () => {
  assert.doesNotThrow(() =>
    validateRiskAcceptance({
      deny,
      workflow,
      record,
      now: new Date('2026-12-30T23:59:59Z'),
    }),
  );
});

test('risk acceptance fails at UTC expiry', () => {
  assert.throws(
    () =>
      validateRiskAcceptance({
        deny,
        workflow,
        record,
        now: new Date('2026-12-31T00:00:00Z'),
      }),
    /风险接受已于 UTC/,
  );
});

test('removed ignore does not require an acceptance record', () => {
  assert.doesNotThrow(() =>
    validateRiskAcceptance({
      deny: '',
      workflow: '',
      record: '',
      now: new Date('2027-01-01T00:00:00Z'),
    }),
  );
});
