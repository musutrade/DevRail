#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const SOURCE_ROOT = resolve(SCRIPT_DIR, "..");
const PROJECT_FILE = ".arc-project.json";
const MANIFEST_FILE = ".arc-framework/manifest.json";
const RELEASE_VERSION = /^v(\d+)\.(\d+)\.(\d+)$/;
const REQUIRED_UPDATE_PATHS = new Set(["FRAMEWORK_VERSION", MANIFEST_FILE]);
const HELP = `用法:
  <新版本模板>/scripts/upgrade-framework.sh [选项]

选项:
  --target <目录>       要升级的业务仓库，默认当前目录
  --check               只检查变更和冲突，不写入文件
  --accept <相对路径>   人工确认保留业务仓库中的该文件，可重复指定
  --skip-verify         跳过升级后的 doctor 和 verify --all（不推荐）
  -h, --help            显示帮助
`;

function fail(message) {
  throw new Error(message);
}

function git(root, args, { allowStatus = [] } = {}) {
  const result = spawnSync("git", args, {
    cwd: root,
    encoding: null,
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0 && !allowStatus.includes(result.status)) {
    const stderr = result.stderr?.toString("utf8").trim();
    fail(`git ${args.join(" ")} 执行失败${stderr ? `: ${stderr}` : ""}`);
  }
  return result;
}

function gitText(root, args) {
  return git(root, args).stdout.toString("utf8").trim();
}

function normalizeRelativePath(value, label = "路径") {
  if (typeof value !== "string" || !value.trim()) {
    fail(`${label}不能为空`);
  }
  const normalized = posix.normalize(value);
  if (
    normalized !== value ||
    normalized.startsWith("/") ||
    normalized === ".." ||
    normalized.startsWith("../") ||
    normalized.includes("\\")
  ) {
    fail(`${label}必须是安全的仓库相对路径: ${value}`);
  }
  return normalized;
}

function parseVersion(value, label) {
  const match = RELEASE_VERSION.exec(value);
  if (!match) {
    fail(`${label}必须是正式版本 vX.Y.Z，实际为 ${value || "(空)"}`);
  }
  return match.slice(1).map(Number);
}

function compareVersions(left, right) {
  for (let index = 0; index < 3; index += 1) {
    if (left[index] !== right[index]) {
      return left[index] - right[index];
    }
  }
  return 0;
}

function parseArguments(argv) {
  const options = {
    targetRoot: process.cwd(),
    checkOnly: false,
    skipVerify: false,
    acceptPaths: [],
  };
  for (let index = 0; index < argv.length; index += 1) {
    const option = argv[index];
    if (option === "-h" || option === "--help") {
      return { help: true };
    }
    if (option === "--check") {
      options.checkOnly = true;
      continue;
    }
    if (option === "--skip-verify") {
      options.skipVerify = true;
      continue;
    }
    if (option === "--target" || option === "--accept") {
      const value = argv[index + 1];
      if (!value || value.startsWith("--")) {
        fail(`${option} 缺少参数值`);
      }
      index += 1;
      if (option === "--target") {
        options.targetRoot = resolve(value);
      } else {
        options.acceptPaths.push(normalizeRelativePath(value, "--accept "));
      }
      continue;
    }
    fail(`未知参数: ${option}`);
  }
  return { ...options, help: false };
}

function readJsonFile(path, label) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail(`${label} 无法解析: ${error.message}`);
  }
}

function showFile(sourceRoot, ref, path) {
  return git(sourceRoot, ["show", `${ref}:${path}`]).stdout;
}

function readManifestAt(sourceRoot, ref) {
  let manifest;
  try {
    manifest = JSON.parse(
      showFile(sourceRoot, ref, MANIFEST_FILE).toString("utf8"),
    );
  } catch (error) {
    fail(
      `${ref} 缺少有效的 ${MANIFEST_FILE}；v1.1.0 是首个可自动升级基线: ${error.message}`,
    );
  }
  if (
    manifest.schemaVersion !== 1 ||
    manifest.framework !== "arc-admin" ||
    !Array.isArray(manifest.managedRoots) ||
    !Array.isArray(manifest.managedFiles)
  ) {
    fail(`${ref} 的框架升级清单无效`);
  }
  return {
    roots: manifest.managedRoots.map((path) => normalizeRelativePath(path)),
    files: new Set(
      manifest.managedFiles.map((path) => normalizeRelativePath(path)),
    ),
  };
}

function readTree(sourceRoot, ref) {
  const output = git(sourceRoot, ["ls-tree", "-r", "-z", ref]).stdout.toString(
    "utf8",
  );
  const entries = new Map();
  for (const record of output.split("\0")) {
    if (!record) {
      continue;
    }
    const match = /^(\d+)\s+(\w+)\s+([0-9a-f]+)\t(.+)$/.exec(record);
    if (!match || match[2] !== "blob") {
      continue;
    }
    entries.set(match[4], { mode: match[1], oid: match[3] });
  }
  return entries;
}

function frameworkTree(sourceRoot, ref) {
  const manifest = readManifestAt(sourceRoot, ref);
  const entries = readTree(sourceRoot, ref);
  const managedPaths = new Set(
    [...entries.keys()].filter(
      (path) =>
        manifest.files.has(path) ||
        manifest.roots.some(
          (root) => path === root || path.startsWith(`${root}/`),
        ),
    ),
  );
  return { entries, managedPaths };
}

function isText(buffer) {
  if (buffer.includes(0)) {
    return false;
  }
  try {
    new TextDecoder("utf-8", { fatal: true }).decode(buffer);
    return true;
  } catch {
    return false;
  }
}

function buffersEqual(left, right) {
  return left.length === right.length && left.equals(right);
}

function mergeText(path, local, base, incoming) {
  const directory = mkdtempSync(join(tmpdir(), "arc-framework-merge-"));
  try {
    const localPath = join(directory, "local");
    const basePath = join(directory, "base");
    const incomingPath = join(directory, "incoming");
    writeFileSync(localPath, local);
    writeFileSync(basePath, base);
    writeFileSync(incomingPath, incoming);
    const result = git(
      directory,
      [
        "merge-file",
        "-p",
        "--diff3",
        "-L",
        `${path}（业务版本）`,
        "-L",
        `${path}（旧框架）`,
        "-L",
        `${path}（新框架）`,
        localPath,
        basePath,
        incomingPath,
      ],
      { allowStatus: [1] },
    );
    return result.status === 0 ? result.stdout : null;
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

function fileMode(entry) {
  if (entry.mode === "100755") {
    return 0o755;
  }
  if (entry.mode !== "100644") {
    fail(`不支持的 Git 文件模式 ${entry.mode}`);
  }
  return 0o644;
}

function localFile(path) {
  if (!existsSync(path)) {
    return null;
  }
  const stat = lstatSync(path);
  if (!stat.isFile()) {
    return { unsupported: true };
  }
  return { content: readFileSync(path), unsupported: false };
}

function planUpgrade({ sourceRoot, targetRoot, fromRef, toRef, acceptPaths }) {
  const oldTree = frameworkTree(sourceRoot, fromRef);
  const newTree = frameworkTree(sourceRoot, toRef);
  const paths = [
    ...new Set([...oldTree.managedPaths, ...newTree.managedPaths]),
  ].sort();
  const changedPaths = new Set(
    paths.filter((path) => {
      const oldManaged = oldTree.managedPaths.has(path);
      const newManaged = newTree.managedPaths.has(path);
      const oldEntry = oldTree.entries.get(path);
      const newEntry = newTree.entries.get(path);
      if (oldManaged && !newManaged && newEntry) {
        return false;
      }
      return (
        !oldEntry ||
        !newEntry ||
        oldEntry.oid !== newEntry.oid ||
        oldEntry.mode !== newEntry.mode
      );
    }),
  );
  const accepted = new Set(acceptPaths);
  for (const path of accepted) {
    if (!changedPaths.has(path)) {
      fail(`--accept 指定的文件没有框架变更: ${path}`);
    }
    if (REQUIRED_UPDATE_PATHS.has(path)) {
      fail(`不能跳过升级机制关键文件: ${path}`);
    }
  }

  const writes = [];
  const deletes = [];
  const conflicts = [];
  let merged = 0;

  for (const path of paths) {
    if (!changedPaths.has(path) || accepted.has(path)) {
      continue;
    }
    const oldEntry = oldTree.entries.get(path);
    const newEntry = newTree.entries.get(path);
    const targetPath = join(targetRoot, path);
    const local = localFile(targetPath);
    if (local?.unsupported) {
      conflicts.push(`${path}（业务仓库中不是普通文件）`);
      continue;
    }

    if (!oldEntry && newEntry) {
      const incoming = showFile(sourceRoot, toRef, path);
      if (!local) {
        writes.push({ path, content: incoming, mode: fileMode(newEntry) });
      } else if (buffersEqual(local.content, incoming)) {
        writes.push({ path, content: local.content, mode: fileMode(newEntry) });
      } else {
        conflicts.push(`${path}（业务仓库已存在同名文件）`);
      }
      continue;
    }

    if (oldEntry && !newEntry) {
      if (!local) {
        continue;
      }
      const base = showFile(sourceRoot, fromRef, path);
      if (buffersEqual(local.content, base)) {
        deletes.push(path);
      } else {
        conflicts.push(`${path}（框架已删除，但业务仓库修改过）`);
      }
      continue;
    }

    const base = showFile(sourceRoot, fromRef, path);
    const incoming = showFile(sourceRoot, toRef, path);
    if (!local) {
      conflicts.push(`${path}（框架已更新，但业务仓库删除了该文件）`);
      continue;
    }
    if (buffersEqual(local.content, incoming)) {
      writes.push({ path, content: local.content, mode: fileMode(newEntry) });
      continue;
    }
    if (buffersEqual(local.content, base)) {
      writes.push({ path, content: incoming, mode: fileMode(newEntry) });
      continue;
    }
    if (buffersEqual(base, incoming)) {
      writes.push({ path, content: local.content, mode: fileMode(newEntry) });
      continue;
    }
    if (isText(local.content) && isText(base) && isText(incoming)) {
      const content = mergeText(path, local.content, base, incoming);
      if (content) {
        writes.push({ path, content, mode: fileMode(newEntry) });
        merged += 1;
        continue;
      }
    }
    conflicts.push(`${path}（框架与业务修改冲突）`);
  }

  return { writes, deletes, conflicts, accepted: [...accepted], merged };
}

function assertCleanTarget(targetRoot) {
  if (!existsSync(join(targetRoot, ".git"))) {
    fail("目标目录不是 Git 仓库");
  }
  const status = gitText(targetRoot, [
    "status",
    "--porcelain",
    "--untracked-files=all",
  ]);
  if (status) {
    fail("业务仓库必须保持干净；请先提交或处理现有变更");
  }
}

function resolveReleaseRef(sourceRoot, version, requireReleaseTag) {
  const ref = `refs/tags/${version}`;
  const commit = gitText(sourceRoot, [
    "rev-parse",
    "--verify",
    `${ref}^{commit}`,
  ]);
  if (requireReleaseTag) {
    const head = gitText(sourceRoot, ["rev-parse", "HEAD"]);
    if (head !== commit) {
      fail(`模板仓库必须检出目标标签 ${version}`);
    }
  }
  return ref;
}

function applyPlan(targetRoot, plan, metadata, toVersion, now) {
  for (const path of plan.deletes) {
    unlinkSync(join(targetRoot, path));
  }
  for (const file of plan.writes) {
    const targetPath = join(targetRoot, file.path);
    mkdirSync(dirname(targetPath), { recursive: true });
    writeFileSync(targetPath, file.content);
    chmodSync(targetPath, file.mode);
  }

  metadata.schemaVersion = Math.max(Number(metadata.schemaVersion) || 1, 2);
  metadata.framework.initializedVersion ??= metadata.framework.version;
  metadata.framework.version = toVersion;
  metadata.framework.upgradedAt = now().toISOString();
  writeFileSync(
    join(targetRoot, PROJECT_FILE),
    `${JSON.stringify(metadata, null, 2)}\n`,
  );
}

function runVerification(targetRoot) {
  const doctor = spawnSync("cargo", ["flow", "doctor"], {
    cwd: targetRoot,
    stdio: "inherit",
  });
  if (doctor.status !== 0) {
    fail("升级内容已写入，但 cargo flow doctor 未通过");
  }
  const verify = spawnSync("cargo", ["flow", "verify", "--all"], {
    cwd: targetRoot,
    stdio: "inherit",
  });
  if (verify.status !== 0) {
    fail("升级内容已写入，但 cargo flow verify --all 未通过");
  }
}

export function upgradeProject({
  sourceRoot = SOURCE_ROOT,
  targetRoot,
  checkOnly = false,
  skipVerify = false,
  acceptPaths = [],
  requireReleaseTag = true,
  now = () => new Date(),
}) {
  sourceRoot = resolve(sourceRoot);
  targetRoot = resolve(targetRoot);
  if (sourceRoot === targetRoot) {
    fail("升级目标不能是框架源仓库本身");
  }
  assertCleanTarget(targetRoot);

  const metadataPath = join(targetRoot, PROJECT_FILE);
  if (!existsSync(metadataPath)) {
    fail(`目标仓库缺少 ${PROJECT_FILE}，请先执行项目初始化`);
  }
  const metadata = readJsonFile(metadataPath, PROJECT_FILE);
  if (metadata.framework?.name !== "arc-admin" || !metadata.framework.version) {
    fail(`${PROJECT_FILE} 缺少有效的 arc-admin 框架版本`);
  }

  const toVersion = readFileSync(
    join(sourceRoot, "FRAMEWORK_VERSION"),
    "utf8",
  ).trim();
  const fromVersion = metadata.framework.version;
  const fromParsed = parseVersion(fromVersion, "业务仓库框架版本");
  const toParsed = parseVersion(toVersion, "模板仓库框架版本");
  const comparison = compareVersions(fromParsed, toParsed);
  if (comparison > 0) {
    fail(`拒绝从 ${fromVersion} 降级到 ${toVersion}`);
  }
  if (comparison === 0) {
    return {
      fromVersion,
      toVersion,
      current: true,
      writes: 0,
      deletes: 0,
      merged: 0,
    };
  }

  const fromRef = resolveReleaseRef(sourceRoot, fromVersion, false);
  const toRef = resolveReleaseRef(sourceRoot, toVersion, requireReleaseTag);
  const taggedVersion = showFile(sourceRoot, toRef, "FRAMEWORK_VERSION")
    .toString("utf8")
    .trim();
  if (taggedVersion !== toVersion) {
    fail(`${toVersion} 标签中的 FRAMEWORK_VERSION 不一致`);
  }

  const plan = planUpgrade({
    sourceRoot,
    targetRoot,
    fromRef,
    toRef,
    acceptPaths,
  });
  if (plan.conflicts.length > 0) {
    fail(
      `检测到 ${plan.conflicts.length} 个升级冲突，未写入任何文件:\n- ${plan.conflicts.join("\n- ")}\n人工合并并提交后，可用 --accept <相对路径> 显式保留该文件。`,
    );
  }

  const result = {
    fromVersion,
    toVersion,
    current: false,
    writes: plan.writes.length,
    deletes: plan.deletes.length,
    merged: plan.merged,
    accepted: plan.accepted,
  };
  if (checkOnly) {
    return result;
  }

  applyPlan(targetRoot, plan, metadata, toVersion, now);
  if (!skipVerify) {
    runVerification(targetRoot);
  }
  return result;
}

function printResult(result, options) {
  if (result.current) {
    console.log(`业务仓库已经是最新框架版本 ${result.toVersion}`);
    return;
  }
  const action = options.checkOnly ? "升级预检通过" : "框架升级完成";
  console.log(`${action}: ${result.fromVersion} -> ${result.toVersion}`);
  console.log(
    `写入 ${result.writes} 个文件，删除 ${result.deletes} 个文件，自动合并 ${result.merged} 个文件，人工保留 ${result.accepted.length} 个文件。`,
  );
  if (options.checkOnly) {
    console.log("当前为 --check 模式，尚未写入任何文件。");
  } else if (options.skipVerify) {
    console.warn(
      "已跳过完整验证；提交前必须手工执行 cargo flow verify --all。",
    );
  } else {
    console.log("环境体检和全量验证已通过，请检查 git diff 后提交升级结果。");
  }
}

function main() {
  try {
    const options = parseArguments(process.argv.slice(2));
    if (options.help) {
      process.stdout.write(HELP);
      return;
    }
    const result = upgradeProject({ sourceRoot: SOURCE_ROOT, ...options });
    printResult(result, options);
  } catch (error) {
    console.error(`框架升级失败: ${error.message}`);
    process.exitCode = 1;
  }
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main();
}
