#!/usr/bin/env node

import { access, mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { spawn } from 'node:child_process';
import { performance } from 'node:perf_hooks';

const root = resolve(import.meta.dirname, '..');
const toolManifest = join(root, 'codex-audit-pipeline/tools/arc-flow/Cargo.toml');
const fileCount = Number(process.env.ARC_FLOW_BENCH_FILES ?? 10_000);
const maxMs = Number(process.env.ARC_FLOW_BENCH_MAX_MS ?? 30_000);
const maxRssMb = Number(process.env.ARC_FLOW_BENCH_MAX_RSS_MB ?? 512);
const parallelThreads = Number(process.env.ARC_FLOW_BENCH_THREADS ?? 4);
const evidencePath = process.env.ARC_FLOW_BENCH_OUTPUT
  ? resolve(root, process.env.ARC_FLOW_BENCH_OUTPUT)
  : undefined;

if (![fileCount, maxMs, maxRssMb, parallelThreads].every(Number.isFinite)
  || ![fileCount, maxMs, maxRssMb, parallelThreads].every((value) => value > 0)) {
  throw new Error('benchmark files, budgets, and thread count must be positive numbers');
}

let timeBinary;
try {
  await access('/usr/bin/time');
  timeBinary = '/usr/bin/time';
} catch {
  timeBinary = undefined;
}

function run(program, args, cwd, { env = process.env, measureResources = false } = {}) {
  return new Promise((resolvePromise, reject) => {
    const measured = measureResources && timeBinary;
    const command = measured ? timeBinary : program;
    const commandArgs = measured ? ['-f', 'ARC_FLOW_MAX_RSS_KB=%M', program, ...args] : args;
    const child = spawn(command, commandArgs, {
      cwd,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let output = '';
    child.stdout.on('data', (chunk) => (output += chunk));
    child.stderr.on('data', (chunk) => (output += chunk));
    child.on('error', reject);
    child.on('close', (code) =>
      resolvePromise({
        code,
        output,
        maxRssKb: Number(output.match(/ARC_FLOW_MAX_RSS_KB=(\d+)/)?.[1] ?? 0),
      }),
    );
  });
}

const fixture = await mkdtemp(join(tmpdir(), 'arc-flow-audit-bench-'));
try {
  await mkdir(join(fixture, 'src'), { recursive: true });
  await mkdir(join(fixture, '.arc-flow'), { recursive: true });
  let result = await run(
    'cargo',
    [
      'run',
      '--quiet',
      '--locked',
      '--manifest-path',
      toolManifest,
      '--',
      '--project-root',
      fixture,
      'init',
      '--preset',
      'generic',
    ],
    root,
  );
  if (result.code !== 0) throw new Error(`arc-flow init failed\n${result.output}`);
  const auditPath = join(fixture, '.arc-flow', 'audit.toml');
  const audit = await readFile(auditPath, 'utf8');
  await writeFile(
    auditPath,
    `${audit}\n[[hard_rules]]\nname = "benchmark rule"\nseverity = "error"\npaths = ["src"]\nextensions = ["rs"]\npatterns = ["forbidden_benchmark_token"]\nallowlist = []\nexclude_patterns = []\n`,
  );
  const source = (index) =>
    index === 0
      ? 'fn fixture() { forbidden_benchmark_token(); }\n'
      : 'fn fixture() { let value = 1; }\n';
  await Promise.all(
    Array.from({ length: fileCount }, (_, index) =>
      writeFile(join(fixture, 'src', `fixture-${index}.rs`), source(index)),
    ),
  );
  result = await run('cargo', ['build', '--quiet', '--locked', '--manifest-path', toolManifest], root);
  if (result.code !== 0) throw new Error(`arc-flow debug build failed\n${result.output}`);
  const binary = join(
    root,
    'codex-audit-pipeline/tools/arc-flow/target/debug',
    process.platform === 'win32' ? 'arc-flow.exe' : 'arc-flow',
  );
  const reportPath = join(fixture, '.arc-flow', 'reports', 'review_context.json');
  const auditArgs = ['--project-root', fixture, 'audit'];
  const serialStarted = performance.now();
  const serial = await run(binary, auditArgs, root, {
    env: { ...process.env, RAYON_NUM_THREADS: '1' },
    measureResources: true,
  });
  const serialElapsedMs = Math.round(performance.now() - serialStarted);
  if (serial.code !== 1) throw new Error(`serial audit failed\n${serial.output}`);
  const serialReport = JSON.parse(await readFile(reportPath, 'utf8'));
  delete serialReport.timestamp;

  const parallelStarted = performance.now();
  const parallel = await run(binary, auditArgs, root, {
    env: { ...process.env, RAYON_NUM_THREADS: String(parallelThreads) },
    measureResources: true,
  });
  const parallelElapsedMs = Math.round(performance.now() - parallelStarted);
  if (parallel.code !== 1) throw new Error(`parallel audit failed\n${parallel.output}`);
  const parallelReport = JSON.parse(await readFile(reportPath, 'utf8'));
  delete parallelReport.timestamp;
  if (JSON.stringify(serialReport) !== JSON.stringify(parallelReport)) {
    throw new Error('serial and parallel audit reports differ');
  }

  const peakRssKb = Math.max(serial.maxRssKb, parallel.maxRssKb);
  console.log(
    `arc-flow audit benchmark: ${fileCount} files; serial=${serialElapsedMs} ms, `
      + `parallel=${parallelElapsedMs} ms, peak_rss=${peakRssKb || 'unavailable'} KB; `
      + 'reports identical',
  );
  if (Math.max(serialElapsedMs, parallelElapsedMs) > maxMs) {
    throw new Error(`audit benchmark exceeded ${maxMs} ms budget`);
  }
  if (peakRssKb > maxRssMb * 1024) {
    throw new Error(`audit benchmark exceeded ${maxRssMb} MiB peak RSS budget`);
  }
  if (evidencePath) {
    await mkdir(dirname(evidencePath), { recursive: true });
    await writeFile(
      evidencePath,
      `${JSON.stringify({
        generatedAt: new Date().toISOString(),
        platform: process.platform,
        fileCount,
        serialThreads: 1,
        parallelThreads,
        serialElapsedMs,
        parallelElapsedMs,
        peakRssKb: peakRssKb || null,
        maxElapsedMs: maxMs,
        maxRssMb,
        reportsIdentical: true,
      }, null, 2)}\n`,
    );
  }
} finally {
  await rm(fixture, { recursive: true, force: true });
}
