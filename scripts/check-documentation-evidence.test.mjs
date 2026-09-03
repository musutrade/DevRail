import assert from 'node:assert/strict';
import test from 'node:test';
import { validateDocumentationEvidence } from './check-documentation-evidence.mjs';

const validEvidence = {
  schemaVersion: 1,
  sourceBaseSha: 'a'.repeat(40),
  scope: ['workflow'],
  checks: [
    {
      command: 'cargo flow verify --components workflow',
      status: 'passed',
      summary: '门禁通过',
    },
  ],
};

const validInput = {
  documents: new Map([['docs/HANDOFF.md', '证据见 CI artifact。']]),
  implementationStatus: 'DevRail MVP **尚未实现完成**；专项验收仍待完成。',
  adr: '- 状态：Proposed',
  evidence: validEvidence,
};

test('documentation evidence accepts a reproducible pending record', () => {
  assert.doesNotThrow(() => validateDocumentationEvidence(validInput));
});

test('documentation evidence rejects gitignored report links', () => {
  assert.throws(
    () =>
      validateDocumentationEvidence({
        ...validInput,
        documents: new Map([
          [
            'docs/HANDOFF.md',
            '[报告](../codex-audit-pipeline/.codex/reports/test_result.md)',
          ],
        ]),
      }),
    /gitignore reports/,
  );
});

test('documentation evidence rejects unsupported completion claims', () => {
  assert.throws(
    () =>
      validateDocumentationEvidence({
        ...validInput,
        implementationStatus:
          'DevRail MVP **尚未实现完成**；Angular Vitest、Playwright 专项测试已通过',
      }),
    /Playwright 通过声明/,
  );
  assert.throws(
    () =>
      validateDocumentationEvidence({
        ...validInput,
        adr: '- 状态：Accepted',
      }),
    /必须保持 Proposed/,
  );
});

test('documentation evidence rejects malformed summaries', () => {
  assert.throws(
    () =>
      validateDocumentationEvidence({
        ...validInput,
        evidence: { ...validEvidence, sourceBaseSha: 'short' },
      }),
    /不符合证据格式/,
  );
});
