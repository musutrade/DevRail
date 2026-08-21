#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const PROJECT_FILE = ".arc-project.json";
const VERSION_PATTERN = /^v\d+\.\d+\.\d+$/;

function fail(message) {
  throw new Error(message);
}

function git(root, args) {
  return execFileSync("git", args, {
    cwd: root,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

function parseArgs(args) {
  const values = {};
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === "--help") {
      return { help: true };
    }
    if (!argument.startsWith("--") || index + 1 >= args.length) {
      fail(`未知或缺少参数值: ${argument}`);
    }
    values[argument.slice(2)] = args[++index];
  }
  const required = ["target", "slug", "title", "short-name", "database", "permission-prefix"];
  for (const name of required) {
    if (!values[name]?.trim()) fail(`缺少参数: --${name}`);
  }
  if (!/^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/.test(values.slug)) {
    fail("--slug 必须是小写字母、数字和连字符组成的标识");
  }
  if (!/^[a-z][a-z0-9_]*$/.test(values.database)) {
    fail("--database 必须是 PostgreSQL 标识");
  }
  if (!/^[a-z][a-z0-9-]*$/.test(values["permission-prefix"])) {
    fail("--permission-prefix 必须是小写业务前缀");
  }
  return values;
}

function usage() {
  return `用法:
  ./scripts/adopt-project.sh \\
    --target /path/to/project \\
    --slug aisix-panel \\
    --title "Aisix Panel" \\
    --short-name "Aisix Panel" \\
    --database aisix_panel \\
    --permission-prefix aisix

该命令只为已有业务仓库写入 .arc-project.json，不修改业务代码、README、运行时配置或 .env 文件。
`;
}

export function adoptProject(sourceRoot, options) {
  const targetRoot = resolve(options.target);
  if (!existsSync(join(targetRoot, ".git"))) fail("目标目录不是 Git 仓库");
  if (existsSync(join(targetRoot, PROJECT_FILE))) {
    fail(`目标仓库已经存在 ${PROJECT_FILE}`);
  }
  const status = git(targetRoot, ["status", "--porcelain", "--untracked-files=all"]);
  if (status) fail("目标仓库工作区必须干净");

  const version = readFileSync(join(sourceRoot, "FRAMEWORK_VERSION"), "utf8").trim();
  if (!VERSION_PATTERN.test(version)) fail("框架仓库的 FRAMEWORK_VERSION 不是正式版本");

  const metadata = {
    schemaVersion: 2,
    framework: { name: "arc-admin", version, initializedVersion: version },
    project: {
      slug: options.slug,
      title: options.title,
      shortName: options["short-name"],
      database: options.database,
      permissionPrefix: options["permission-prefix"],
      apiBaseUrl: "/api/v1",
    },
  };
  writeFileSync(join(targetRoot, PROJECT_FILE), `${JSON.stringify(metadata, null, 2)}\n`, {
    encoding: "utf8",
    flag: "wx",
  });
  return { targetRoot, version };
}

try {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    process.stdout.write(usage());
  } else {
    const sourceRoot = resolve(fileURLToPath(new URL("..", import.meta.url)));
    const result = adoptProject(sourceRoot, options);
    console.log(`项目登记完成: ${options.title} (${options.slug})`);
    console.log(`框架版本: ${result.version}`);
    console.log(`已写入: ${join(result.targetRoot, PROJECT_FILE)}`);
  }
} catch (error) {
  console.error(`项目登记失败: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
}
