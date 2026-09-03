import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(import.meta.dirname, '..');
const advisoryId = 'RUSTSEC-2023-0071';
const expiresAt = '2026-12-31T00:00:00Z';
const recordPath = 'docs/security/rustsec-2023-0071-risk-acceptance-2026-09.md';

export function validateRiskAcceptance({ deny, workflow, record, now }) {
  const ignored = deny.includes(advisoryId) || workflow.includes(`ignore: ${advisoryId}`);
  if (!ignored) {
    return;
  }
  for (const expected of [
    advisoryId,
    'web-push -> jwt-simple -> superboring -> rsa',
    'Owner: DevRail maintainers',
    'Review date: 2026-12-31',
  ]) {
    if (!record.includes(expected)) {
      throw new Error(`风险接受记录缺少必需证据：${expected}`);
    }
  }
  const expiry = Date.parse(expiresAt);
  if (!Number.isFinite(expiry)) {
    throw new Error(`风险接受到期时间无效：${expiresAt}`);
  }
  if (now.getTime() >= expiry) {
    throw new Error(
      `${advisoryId} 风险接受已于 UTC ${expiresAt} 失效；请移除 ignore、替换依赖或重新评审`,
    );
  }
}

async function main() {
  const [deny, workflow, record] = await Promise.all([
    readFile(resolve(root, 'deny.toml'), 'utf8'),
    readFile(resolve(root, '.github/workflows/security.yml'), 'utf8'),
    readFile(resolve(root, recordPath), 'utf8'),
  ]);
  const override = process.env.DEVRAIL_RISK_ACCEPTANCE_NOW;
  const now = override ? new Date(override) : new Date();
  if (Number.isNaN(now.getTime())) {
    throw new Error('DEVRAIL_RISK_ACCEPTANCE_NOW 必须是有效 ISO-8601 时间');
  }
  validateRiskAcceptance({ deny, workflow, record, now });
  console.log(`风险接受校验通过：${advisoryId}，到期 UTC ${expiresAt}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
