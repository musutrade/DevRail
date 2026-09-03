import assert from 'node:assert/strict';
import test from 'node:test';
import {
  validateActionReferences,
  validateDockerfileDigests,
} from './check-supply-chain.mjs';

const allowedWorkflows = new Map([
  [
    'workflow.yml',
    [
      'uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1',
      'uses: actions/setup-node@820762786026740c76f36085b0efc47a31fe5020',
      'uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a',
      'uses: actions/dependency-review-action@a1d282b36b6f3519aa1f3fc636f609c47dddb294',
      'uses: github/codeql-action/init@cdf488f595d80d6e07e03d4674febd5ab45fa938',
      'uses: github/codeql-action/analyze@cdf488f595d80d6e07e03d4674febd5ab45fa938',
      'uses: dorny/paths-filter@0e4a8c6effa4802afeda77dc8d303f8176d7dfad',
      'uses: actions-rust-lang/setup-rust-toolchain@166cdcfd11aee3cb47222f9ddb555ce30ddb9659',
      'uses: rustsec/audit-check@69366f33c96575abad1ee0dba8212993eecbe998',
      'uses: EmbarkStudios/cargo-deny-action@3c6349835b2b7b196a839186cb8b78e02f7b5f25',
      'uses: docker/setup-buildx-action@8d2750c68a42422c14e847fe6c8ac0403b4cbd6f',
      'uses: docker/build-push-action@10e90e3645eae34f1e60eeb005ba3a3d33f178e8',
      'uses: aquasecurity/trivy-action@ed142fd0673e97e23eac54620cfb913e5ce36c25',
      'uses: anchore/sbom-action@e22c389904149dbc22b58101806040fa8d37a610',
    ].join('\n'),
  ],
]);

test('action references accept the exact immutable allowlist', () => {
  assert.doesNotThrow(() => validateActionReferences(allowedWorkflows));
});

test('action references reject mutable tags', () => {
  const workflows = new Map(allowedWorkflows);
  workflows.set(
    'workflow.yml',
    workflows.get('workflow.yml').replace(
      'actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1',
      'actions/checkout@v7',
    ),
  );
  assert.throws(() => validateActionReferences(workflows), /完整 commit SHA/);
});

test('action references reject unknown SHA and action identity', () => {
  const wrongSha = new Map(allowedWorkflows);
  wrongSha.set(
    'workflow.yml',
    wrongSha
      .get('workflow.yml')
      .replace(
        '3d3c42e5aac5ba805825da76410c181273ba90b1',
        '0000000000000000000000000000000000000000',
      ),
  );
  assert.throws(() => validateActionReferences(wrongSha), /SHA 未经允许/);

  const unknown = new Map(allowedWorkflows);
  unknown.set(
    'workflow.yml',
    `${unknown.get('workflow.yml')}\nuses: unknown/action@0000000000000000000000000000000000000000`,
  );
  assert.throws(() => validateActionReferences(unknown), /未登记的第三方 Action/);
});

test('Dockerfiles require a digest for every stage', () => {
  assert.doesNotThrow(() =>
    validateDockerfileDigests(
      'Dockerfile',
      [
        'FROM node:24@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa AS builder',
        'FROM nginx:1@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
      ].join('\n'),
    ),
  );
  assert.throws(
    () =>
      validateDockerfileDigests(
        'Dockerfile',
        ['FROM node:24 AS builder', 'FROM nginx:1'].join('\n'),
      ),
    /未固定 sha256 digest/,
  );
});
