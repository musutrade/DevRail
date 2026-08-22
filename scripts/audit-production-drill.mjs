#!/usr/bin/env node

import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn } from 'node:child_process';

const root = resolve(import.meta.dirname, '..');
const manifest = join(root, 'codex-audit-pipeline/tools/arc-flow/Cargo.toml');

function run(args, cwd) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn('cargo', ['run', '--quiet', '--locked', '--manifest-path', manifest, '--', ...args], {
      cwd,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let output = '';
    child.stdout.on('data', (chunk) => (output += chunk));
    child.stderr.on('data', (chunk) => (output += chunk));
    child.on('error', reject);
    child.on('close', (code) => resolvePromise({ code, output }));
  });
}

function runExternal(program, args, cwd) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(program, args, { cwd, stdio: ['ignore', 'pipe', 'pipe'] });
    let output = '';
    child.stdout.on('data', (chunk) => (output += chunk));
    child.stderr.on('data', (chunk) => (output += chunk));
    child.on('error', reject);
    child.on('close', (code) => resolvePromise({ code, output }));
  });
}

const fixture = await mkdtemp(join(tmpdir(), 'arc-flow-production-drill-'));
try {
  let result = await run(['--project-root', fixture, 'init', '--preset', 'generic'], root);
  if (result.code !== 0) throw new Error(`init failed\n${result.output}`);
  result = await run(['--project-root', fixture, 'config', 'check'], root);
  if (result.code !== 0) throw new Error(`config check failed\n${result.output}`);

  const legacyFlow = join(fixture, 'legacy-flow.toml');
  await writeFile(
    legacyFlow,
    `version = 1\n\n[paths]\nbackend = "backend"\nfrontend = "frontend"\nreports = "reports"\ntool_manifest = "codex-audit-pipeline/tools/arc-flow/Cargo.toml"\naudit_config = ".arc-flow/audit.toml"\n\n[doctor]\nrequired_commands = ["git"]\nnode_version_file = ".node-version"\nhooks_path = "codex-audit-pipeline/hooks"\n\n[database]\nimage = "postgres:16-alpine"\nstartup_timeout_secs = 30\ncontainer_port = 5432\nuser = "test"\npassword = "test"\nname = "test"\n\n[[scope.rules]]\npatterns = ["**"]\ncomponents = ["project"]\n\n[[steps]]\nid = "project.check"\nlabel = "project check"\ncomponent = "project"\nprofiles = ["full", "hook"]\nprogram = "git"\nargs = ["diff", "--check"]\ncwd = "{root}"\nlog = "project_check.log"\ntimeout_secs = 60\n`,
  );
  result = await run(
    [
      '--project-root',
      fixture,
      'config',
      'migrate',
      '--input',
      'legacy-flow.toml',
      '--output',
      '.arc-flow/flow.toml',
      '--force',
    ],
    root,
  );
  if (result.code !== 0) throw new Error(`config migration failed\n${result.output}`);
  result = await run(['--project-root', fixture, 'config', 'check'], root);
  if (result.code !== 0) throw new Error(`migrated config check failed\n${result.output}`);

  const auditPath = join(fixture, '.arc-flow', 'audit.toml');
  const audit = await readFile(auditPath, 'utf8');
  await writeFile(
    auditPath,
    `${audit}\n[[hard_rules]]\nname = "drill rule"\nseverity = "error"\npaths = ["src"]\nextensions = ["rs"]\npatterns = ["forbidden_drill_token"]\nallowlist = []\nexclude_patterns = []\n`,
  );
  await mkdir(join(fixture, 'src'), { recursive: true });
  await writeFile(join(fixture, 'src', 'src.rs'), 'fn ok() {}\n');
  result = await run(['--project-root', fixture, 'audit'], root);
  if (result.code !== 0) throw new Error(`audit failed\n${result.output}`);

  await writeFile(auditPath, `${audit}\nversion = 99\n`);
  result = await run(['--project-root', fixture, 'audit'], root);
  if (result.code === 0) throw new Error('invalid config unexpectedly passed');
  await writeFile(auditPath, audit);
  result = await run(['--project-root', fixture, 'audit'], root);
  if (result.code !== 0) throw new Error(`recovery audit failed\n${result.output}`);

  for (const args of [['init'], ['add', '.']]) {
    result = await runExternal('git', args, fixture);
    if (result.code !== 0) throw new Error(`git ${args[0]} failed\n${result.output}`);
  }
  result = await run(['--project-root', fixture, 'hook'], root);
  if (result.code !== 0) throw new Error(`pre-commit hook profile failed\n${result.output}`);
  result = await run(['--project-root', fixture, 'verify', '--all'], root);
  if (result.code !== 0) throw new Error(`PR-style full verify failed\n${result.output}`);
  console.log('arc-flow production drill passed: init, first rule, fail-closed config, and recovery');
} finally {
  await rm(fixture, { recursive: true, force: true });
}
