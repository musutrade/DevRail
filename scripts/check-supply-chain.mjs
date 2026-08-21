import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');

async function readRequired(path) {
  try {
    return await readFile(resolve(root, path), 'utf8');
  } catch (error) {
    throw new Error(`缺少供应链安全文件：${path}`, { cause: error });
  }
}

function requireText(content, expected, path) {
  if (!content.includes(expected)) {
    throw new Error(`${path} 缺少必需配置：${expected}`);
  }
}

const [deny, security, codeql] = await Promise.all([
  readRequired('deny.toml'),
  readRequired('.github/workflows/security.yml'),
  readRequired('.github/workflows/codeql.yml'),
]);

for (const section of ['[advisories]', '[licenses]', '[bans]', '[sources]']) {
  requireText(deny, section, 'deny.toml');
}

for (const action of [
  'rustsec/audit-check@v2.0.0',
  'EmbarkStudios/cargo-deny-action@v2.1.1',
  'aquasecurity/trivy-action@v0.36.0',
  'anchore/sbom-action@v0.24.0',
]) {
  requireText(security, action, '.github/workflows/security.yml');
}
requireText(security, 'severity: HIGH,CRITICAL', '.github/workflows/security.yml');
requireText(security, 'format: spdx-json', '.github/workflows/security.yml');
requireText(codeql, 'github/codeql-action/init@v4', '.github/workflows/codeql.yml');
requireText(codeql, 'language: [javascript-typescript, rust]', '.github/workflows/codeql.yml');

console.log('供应链安全配置检查通过');
