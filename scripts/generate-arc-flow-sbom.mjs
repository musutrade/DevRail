#!/usr/bin/env node

import { createHash, randomUUID } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { spawn } from 'node:child_process';

const root = resolve(import.meta.dirname, '..');
const outputDir = resolve(process.argv[2] ?? join(root, 'dist/arc-flow-release'));
const binaryArg = process.argv[3];

function run(program, args) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(program, args, { cwd: root, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (chunk) => (stdout += chunk));
    child.stderr.on('data', (chunk) => (stderr += chunk));
    child.on('error', reject);
    child.on('close', (code) => {
      if (code === 0) resolvePromise(stdout);
      else reject(new Error(`${program} exited with ${code}: ${stderr}`));
    });
  });
}

await mkdir(outputDir, { recursive: true });
const metadata = JSON.parse(
  await run('cargo', [
    'metadata',
    '--locked',
    '--manifest-path',
    'codex-audit-pipeline/tools/arc-flow/Cargo.toml',
    '--format-version',
    '1',
  ]),
);
const packageIds = new Map(
  metadata.packages.map((pkg, index) => [
    pkg.id,
    `SPDXRef-Package-${pkg.name.replaceAll(/[^A-Za-z0-9.-]/g, '-')}`
      + `-${pkg.version.replaceAll(/[^A-Za-z0-9.-]/g, '-')}-${index}`,
  ]),
);
const packageData = metadata.packages.map((pkg) => ({
  SPDXID: packageIds.get(pkg.id),
  name: pkg.name,
  versionInfo: pkg.version,
  downloadLocation: pkg.source ?? 'NOASSERTION',
  filesAnalyzed: false,
  licenseConcluded: 'NOASSERTION',
  licenseDeclared: pkg.license ?? 'NOASSERTION',
  copyrightText: 'NOASSERTION',
}));
const relationshipKeys = new Set();
const relationships = [];
for (const node of metadata.resolve?.nodes ?? []) {
  for (const dependency of node.dependencies) {
    const key = `${node.id}\0${dependency}`;
    if (relationshipKeys.has(key)) continue;
    relationshipKeys.add(key);
    relationships.push({
      spdxElementId: packageIds.get(node.id),
      relationshipType: 'DEPENDS_ON',
      relatedSpdxElement: packageIds.get(dependency),
    });
  }
}
const rootPackageId = packageIds.get(metadata.resolve?.root ?? metadata.workspace_members[0]);
const document = {
  SPDXID: 'SPDXRef-DOCUMENT',
  spdxVersion: 'SPDX-2.3',
  dataLicense: 'CC0-1.0',
  name: 'arc-flow-release',
  documentNamespace: `https://devrail.local/spdx/arc-flow/${randomUUID()}`,
  creationInfo: {
    created: new Date().toISOString(),
    creators: ['Tool: cargo-metadata', 'Tool: DevRail generate-arc-flow-sbom.mjs'],
  },
  packages: packageData,
  documentDescribes: [rootPackageId],
  relationships,
};
const sbomPath = join(outputDir, 'arc-flow.spdx.json');
await writeFile(sbomPath, `${JSON.stringify(document, null, 2)}\n`);

if (binaryArg) {
  const binaryPath = resolve(binaryArg);
  const binary = await readFile(binaryPath);
  const checksum = createHash('sha256').update(binary).digest('hex');
  await writeFile(`${binaryPath}.sha256`, `${checksum}  ${binaryPath.split(/[\\/]/).pop()}\n`);
  await writeFile(join(outputDir, 'arc-flow.sha256'), `${checksum}  ${binaryPath.split(/[\\/]/).pop()}\n`);
}
console.log(`arc-flow SPDX SBOM written to ${sbomPath}`);
