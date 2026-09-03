import { readdir, readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(import.meta.dirname, '..');
const evidencePath =
  'docs/verification/evidence/security-audit-remediation-baseline-2026-09-03.json';
const historicalRecords = new Set([
  'docs/verification/security-audit-2026-09-01.md',
  'docs/verification/security-audit-followup-2026-09-03.md',
]);

async function markdownFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await markdownFiles(path)));
    } else if (entry.name.endsWith('.md')) {
      files.push(path);
    }
  }
  return files;
}

export function validateDocumentationEvidence({
  documents,
  implementationStatus,
  adr,
  evidence,
}) {
  for (const [relative, source] of documents) {
    if (historicalRecords.has(relative)) continue;
    if (/]\([^)\n]*codex-audit-pipeline\/\.codex\/reports\//.test(source)) {
      throw new Error(`${relative} 仍将 gitignore reports 文件作为链接证据`);
    }
  }

  if (!implementationStatus.includes('MVP **尚未实现完成**')) {
    throw new Error('DevRail 实现状态缺少 MVP 尚未完成的明确结论');
  }
  if (implementationStatus.includes('Angular Vitest、Playwright 专项测试已通过')) {
    throw new Error('DevRail 实现状态仍含缺少可复核证据的 continuation Playwright 通过声明');
  }

  if (!adr.includes('- 状态：Proposed')) {
    throw new Error('ADR-0010 在远端验收前必须保持 Proposed');
  }

  if (
    evidence.schemaVersion !== 1 ||
    !/^[a-f0-9]{40}$/.test(evidence.sourceBaseSha) ||
    !Array.isArray(evidence.scope) ||
    !Array.isArray(evidence.checks) ||
    evidence.checks.length === 0
  ) {
    throw new Error(`${evidencePath} 不符合证据格式版本 1`);
  }
  for (const check of evidence.checks) {
    if (
      typeof check.command !== 'string' ||
      check.command.length === 0 ||
      !['passed', 'failed', 'pending'].includes(check.status) ||
      typeof check.summary !== 'string' ||
      check.summary.length === 0
    ) {
      throw new Error(`${evidencePath} 包含无效检查记录`);
    }
  }
}

async function main() {
  const docsRoot = resolve(root, 'docs');
  const documents = new Map(
    await Promise.all(
      (await markdownFiles(docsRoot)).map(async (path) => [
        path.slice(root.length + 1),
        await readFile(path, 'utf8'),
      ]),
    ),
  );
  const [implementationStatus, adr, evidenceSource] = await Promise.all([
    readFile(resolve(root, 'docs/devrail-implementation-status.md'), 'utf8'),
    readFile(resolve(root, 'docs/adr/ADR-0010-security-audit-remediation-baseline.md'), 'utf8'),
    readFile(resolve(root, evidencePath), 'utf8'),
  ]);
  validateDocumentationEvidence({
    documents,
    implementationStatus,
    adr,
    evidence: JSON.parse(evidenceSource),
  });
  console.log('文档与验证证据一致性检查通过');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
