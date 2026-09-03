import { readdir, readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { validateRiskAcceptance } from './check-risk-acceptances.mjs';

const root = resolve(import.meta.dirname, '..');
const actionAllowlist = new Map([
  ['actions/checkout', '3d3c42e5aac5ba805825da76410c181273ba90b1'],
  ['actions/setup-node', '820762786026740c76f36085b0efc47a31fe5020'],
  ['actions/upload-artifact', '043fb46d1a93c77aae656e7c1c64a875d1fc6a0a'],
  ['actions/dependency-review-action', 'a1d282b36b6f3519aa1f3fc636f609c47dddb294'],
  ['github/codeql-action/init', 'cdf488f595d80d6e07e03d4674febd5ab45fa938'],
  ['github/codeql-action/analyze', 'cdf488f595d80d6e07e03d4674febd5ab45fa938'],
  ['dorny/paths-filter', '0e4a8c6effa4802afeda77dc8d303f8176d7dfad'],
  ['actions-rust-lang/setup-rust-toolchain', '166cdcfd11aee3cb47222f9ddb555ce30ddb9659'],
  ['rustsec/audit-check', '69366f33c96575abad1ee0dba8212993eecbe998'],
  ['EmbarkStudios/cargo-deny-action', '3c6349835b2b7b196a839186cb8b78e02f7b5f25'],
  ['docker/setup-buildx-action', '8d2750c68a42422c14e847fe6c8ac0403b4cbd6f'],
  ['docker/build-push-action', '10e90e3645eae34f1e60eeb005ba3a3d33f178e8'],
  ['aquasecurity/trivy-action', 'ed142fd0673e97e23eac54620cfb913e5ce36c25'],
  ['anchore/sbom-action', 'e22c389904149dbc22b58101806040fa8d37a610'],
]);

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

export function validateActionReferences(workflows) {
  const seen = new Set();
  for (const [path, source] of workflows) {
    for (const match of source.matchAll(
      /\buses:\s*([A-Za-z0-9_.-]+\/[A-Za-z0-9_.\/-]+)@([^\s#]+)/g,
    )) {
      const [, action, reference] = match;
      const expected = actionAllowlist.get(action);
      if (!expected) {
        throw new Error(`${path} 使用未登记的第三方 Action：${action}`);
      }
      if (!/^[a-f0-9]{40}$/.test(reference)) {
        throw new Error(`${path} 的 ${action} 必须固定完整 commit SHA`);
      }
      if (reference !== expected) {
        throw new Error(`${path} 的 ${action} SHA 未经允许：${reference}`);
      }
      seen.add(action);
    }
  }
  for (const action of actionAllowlist.keys()) {
    if (!seen.has(action)) {
      throw new Error(`允许列表中的 Action 未在 workflow 使用：${action}`);
    }
  }
}

export function validateDockerfileDigests(path, source) {
  const images = [...source.matchAll(/^FROM\s+(\S+)/gm)].map((match) => match[1]);
  if (images.length < 2) {
    throw new Error(`${path} 必须保持多阶段构建`);
  }
  for (const image of images) {
    if (!/@sha256:[a-f0-9]{64}$/i.test(image)) {
      throw new Error(`${path} 的基础镜像未固定 sha256 digest：${image}`);
    }
  }
}

async function main() {
  const workflowDir = resolve(root, '.github/workflows');
  const workflowFiles = (await readdir(workflowDir))
    .filter((file) => file.endsWith('.yml') || file.endsWith('.yaml'))
    .sort();
  const workflows = new Map(
    await Promise.all(
      workflowFiles.map(async (file) => [
        `.github/workflows/${file}`,
        await readRequired(`.github/workflows/${file}`),
      ]),
    ),
  );
  const [
    deny,
    security,
    codeql,
    ci,
    backendDockerfile,
    frontendDockerfile,
    productionCompose,
    riskRecord,
  ] = await Promise.all([
    readRequired('deny.toml'),
    readRequired('.github/workflows/security.yml'),
    readRequired('.github/workflows/codeql.yml'),
    readRequired('.github/workflows/ci.yml'),
    readRequired('backend/Dockerfile'),
    readRequired('frontend/Dockerfile'),
    readRequired('compose.production.yaml'),
    readRequired('docs/security/rustsec-2023-0071-risk-acceptance-2026-09.md'),
  ]);

  for (const section of ['[advisories]', '[licenses]', '[bans]', '[sources]']) {
    requireText(deny, section, 'deny.toml');
  }

  validateActionReferences(workflows);
  validateDockerfileDigests('backend/Dockerfile', backendDockerfile);
  validateDockerfileDigests('frontend/Dockerfile', frontendDockerfile);
  requireText(
    productionCompose,
    'postgres:17.10-bookworm@sha256:9b18b78397054fce88a9552e9d5a3ad5bb7fd258c5b3cc1c5028e46373d6ea8f',
    'compose.production.yaml',
  );
  requireText(security, "'deployment/nginx/**'", '.github/workflows/security.yml');
  if (security.includes("'docker/**'")) {
    throw new Error('.github/workflows/security.yml 仍包含无匹配的 docker/** 过滤项');
  }
  for (const condition of [
    "github.event_name == 'schedule'",
    "github.event_name == 'workflow_dispatch'",
  ]) {
    const count = security.split(condition).length - 1;
    if (count < 2) {
      throw new Error(`安全 workflow 未确保依赖和镜像任务在 ${condition} 时执行`);
    }
  }
  requireText(security, 'severity: HIGH,CRITICAL', '.github/workflows/security.yml');
  requireText(security, 'format: spdx-json', '.github/workflows/security.yml');
  requireText(
    security,
    'node scripts/check-risk-acceptances.mjs',
    '.github/workflows/security.yml',
  );
  requireText(codeql, 'language: [javascript-typescript, rust]', '.github/workflows/codeql.yml');
  if (codeql.includes('repository.private') || ci.includes('repository.private')) {
    throw new Error('CodeQL 或依赖审查仍受仓库可见性条件限制');
  }
  requireText(
    ci,
    'actions/dependency-review-action@a1d282b36b6f3519aa1f3fc636f609c47dddb294',
    '.github/workflows/ci.yml',
  );
  validateRiskAcceptance({
    deny,
    workflow: security,
    record: riskRecord,
    now: process.env.DEVRAIL_RISK_ACCEPTANCE_NOW
      ? new Date(process.env.DEVRAIL_RISK_ACCEPTANCE_NOW)
      : new Date(),
  });

  console.log('供应链安全配置检查通过');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
