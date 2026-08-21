#!/usr/bin/env node

import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const MANIFEST_PATH = ".arc-framework/manifest.json";
const RELEASE_VERSION = /^v\d+\.\d+\.\d+$/;
const DEVELOPMENT_VERSION = /^v\d+\.\d+\.\d+-dev$/;
const FORBIDDEN_PARTS = new Set(["node_modules", "target", ".env", "reports"]);

function fail(message) {
  throw new Error(message);
}

function normalizePath(value, label) {
  if (typeof value !== "string" || !value.trim()) {
    fail(`${label} 必须是非空路径`);
  }
  const normalized = posix.normalize(value);
  if (
    normalized !== value ||
    normalized.startsWith("/") ||
    normalized === ".." ||
    normalized.startsWith("../") ||
    normalized.includes("\\")
  ) {
    fail(`${label} 不是安全的仓库相对路径: ${value}`);
  }
  if (normalized.split("/").some((part) => FORBIDDEN_PARTS.has(part))) {
    fail(`${label} 不得管理本地依赖、构建产物、秘密或报告: ${value}`);
  }
  return normalized;
}

function uniquePaths(values, label) {
  if (!Array.isArray(values) || values.length === 0) {
    fail(`${label} 必须是非空数组`);
  }
  const normalized = values.map((value, index) =>
    normalizePath(value, `${label}[${index}]`),
  );
  if (new Set(normalized).size !== normalized.length) {
    fail(`${label} 包含重复路径`);
  }
  return normalized;
}

function isManaged(path, roots, files) {
  return (
    files.includes(path) ||
    roots.some((root) => path === root || path.startsWith(`${root}/`))
  );
}

export function validateFrameworkRelease(root = ROOT) {
  const version = readFileSync(join(root, "FRAMEWORK_VERSION"), "utf8").trim();
  if (!RELEASE_VERSION.test(version) && !DEVELOPMENT_VERSION.test(version)) {
    fail("FRAMEWORK_VERSION 必须是 vX.Y.Z 或 vX.Y.Z-dev");
  }

  const changelog = readFileSync(join(root, "CHANGELOG.md"), "utf8");
  const expectedHeading = RELEASE_VERSION.test(version)
    ? `## [${version}]`
    : "## [Unreleased]";
  if (!changelog.includes(expectedHeading)) {
    fail(`CHANGELOG.md 缺少 ${expectedHeading}`);
  }

  const manifest = JSON.parse(readFileSync(join(root, MANIFEST_PATH), "utf8"));
  if (manifest.schemaVersion !== 1 || manifest.framework !== "arc-admin") {
    fail("框架升级清单 schemaVersion 或 framework 无效");
  }
  const roots = uniquePaths(manifest.managedRoots, "managedRoots");
  const files = uniquePaths(manifest.managedFiles, "managedFiles");

  for (const path of roots) {
    if (
      !existsSync(join(root, path)) ||
      !statSync(join(root, path)).isDirectory()
    ) {
      fail(`managedRoots 路径不存在或不是目录: ${path}`);
    }
  }
  for (const path of files) {
    if (!existsSync(join(root, path)) || !statSync(join(root, path)).isFile()) {
      fail(`managedFiles 路径不存在或不是文件: ${path}`);
    }
  }

  const required = [
    MANIFEST_PATH,
    "CHANGELOG.md",
    "FRAMEWORK_VERSION",
    "scripts/init-project.mjs",
    "scripts/init-project.sh",
    "scripts/init-project.test.mjs",
    "scripts/upgrade-framework.mjs",
    "scripts/upgrade-framework.sh",
    "scripts/upgrade-framework.test.mjs",
  ];
  for (const path of required) {
    if (!isManaged(path, roots, files)) {
      fail(`框架升级清单未覆盖关键文件: ${path}`);
    }
  }

  for (const path of ["scripts/init-project.sh", "scripts/upgrade-framework.sh"]) {
    const shellMode = statSync(join(root, path)).mode;
    if ((shellMode & 0o111) === 0) {
      fail(`${path} 必须可执行`);
    }
  }

  return { version, managedRoots: roots.length, managedFiles: files.length };
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    const result = validateFrameworkRelease();
    console.log(
      `框架发布配置检查通过: ${result.version}，${result.managedRoots} 个目录，${result.managedFiles} 个根文件`,
    );
  } catch (error) {
    console.error(`框架发布配置检查失败: ${error.message}`);
    process.exitCode = 1;
  }
}
