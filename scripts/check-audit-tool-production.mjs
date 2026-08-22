#!/usr/bin/env node

import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const files = {
  workflow: '.github/workflows/arc-flow-platform.yml',
  benchmark: 'scripts/benchmark-audit.mjs',
  sbom: 'scripts/generate-arc-flow-sbom.mjs',
  drill: 'scripts/audit-production-drill.mjs',
  docs: 'codex-audit-pipeline/README.md',
};

const contents = Object.fromEntries(
  await Promise.all(
    Object.entries(files).map(async ([key, path]) => [key, await readFile(resolve(root, path), 'utf8')]),
  ),
);

for (const [key, content] of Object.entries(contents)) {
  if (!content.trim()) throw new Error(`${files[key]} 为空`);
}
for (const expected of ['ubuntu-latest', 'windows-latest', 'cargo test', 'upload-artifact']) {
  if (!contents.workflow.includes(expected)) throw new Error(`平台 workflow 缺少：${expected}`);
}
for (const expected of [
  'ARC_FLOW_BENCH_FILES',
  '10_000',
  'ARC_FLOW_BENCH_MAX_MS',
  'ARC_FLOW_BENCH_MAX_RSS_MB',
  'RAYON_NUM_THREADS',
  'reportsIdentical',
]) {
  if (!contents.benchmark.includes(expected)) throw new Error(`benchmark 缺少：${expected}`);
}
for (const expected of [
  'SPDX-2.3',
  'dataLicense',
  'relationships',
  'sha256',
  "'metadata'",
]) {
  if (!contents.sbom.includes(expected)) throw new Error(`SBOM/checksum 脚本缺少：${expected}`);
}
for (const expected of ["'migrate'", 'invalid config unexpectedly passed', 'production drill passed']) {
  if (!contents.drill.includes(expected)) throw new Error(`生产演练缺少：${expected}`);
}
for (const expected of ['词法边界', '10,000', 'Windows', 'SBOM']) {
  if (!contents.docs.includes(expected)) throw new Error(`arc-flow 文档缺少：${expected}`);
}
console.log('审计工具生产化配置检查通过');
