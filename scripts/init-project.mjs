#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");
const PROJECT_FILE = ".arc-project.json";
const FRAMEWORK_MANIFEST = ".arc-framework/manifest.json";
const RELEASE_VERSION = /^v\d+\.\d+\.\d+$/;
const HEADER_START = "<!-- ARC_PROJECT_HEADER_START -->";
const HEADER_END = "<!-- ARC_PROJECT_HEADER_END -->";
const TEMPLATE_USAGE_START = "<!-- ARC_TEMPLATE_USAGE_START -->";
const TEMPLATE_USAGE_END = "<!-- ARC_TEMPLATE_USAGE_END -->";

const HELP = `用法:
  ./scripts/init-project.sh \\
    --slug stock-analysis \\
    --title 股票分析系统 \\
    --database stock_analysis \\
    --permission-prefix stock \\
    [--short-name 投研平台] \\
    [--api-base-url /api/v1]

必填参数:
  --slug               项目标识，小写字母、数字和连字符
  --title              用户可见的产品名称
  --database           PostgreSQL 数据库名称
  --permission-prefix  业务权限前缀，例如 stock、oa、token

可选参数:
  --short-name         侧栏使用的产品简称，默认与 title 相同
  --api-base-url       前端 API 基址，默认 /api/v1
  -h, --help           显示帮助
`;

const OPTION_KEYS = new Map([
  ["--slug", "slug"],
  ["--title", "title"],
  ["--database", "database"],
  ["--permission-prefix", "permissionPrefix"],
  ["--short-name", "shortName"],
  ["--api-base-url", "apiBaseUrl"],
]);

function fail(message) {
  throw new Error(message);
}

function normalizedText(value, option, maximumLength) {
  const normalized = value?.trim();
  if (!normalized) {
    fail(`${option} 不能为空`);
  }
  if (normalized.includes("\n") || normalized.includes("\r")) {
    fail(`${option} 不能包含换行符`);
  }
  if ([...normalized].length > maximumLength) {
    fail(`${option} 最多允许 ${maximumLength} 个字符`);
  }
  return normalized;
}

function normalizeApiBaseUrl(value) {
  const normalized = normalizedText(value, "--api-base-url", 200).replace(/\/+$/, "");
  if (!normalized) {
    return "/";
  }
  if (normalized.startsWith("/")) {
    if (normalized.startsWith("//") || /[\s\\?#]/.test(normalized)) {
      fail("--api-base-url 必须以单斜杠开头，且不能包含空白、反斜杠、查询参数或片段");
    }
    return normalized;
  }

  let parsed;
  try {
    parsed = new URL(normalized);
  } catch {
    fail("--api-base-url 必须是以 / 开头的路径或有效的 HTTP(S) URL");
  }
  if (!["http:", "https:"].includes(parsed.protocol)) {
    fail("--api-base-url 仅支持 HTTP(S) URL");
  }
  if (parsed.username || parsed.password || parsed.search || parsed.hash) {
    fail("--api-base-url 不能包含凭据、查询参数或片段");
  }
  return normalized;
}

export function parseArguments(argv) {
  if (argv.includes("-h") || argv.includes("--help")) {
    return { help: true };
  }

  const values = {};
  for (let index = 0; index < argv.length; index += 2) {
    const option = argv[index];
    const key = OPTION_KEYS.get(option);
    if (!key) {
      fail(`未知参数: ${option ?? "(空)"}`);
    }
    const value = argv[index + 1];
    if (value === undefined || value.startsWith("--")) {
      fail(`${option} 缺少参数值`);
    }
    if (values[key] !== undefined) {
      fail(`${option} 不能重复指定`);
    }
    values[key] = value;
  }

  const slug = normalizedText(values.slug, "--slug", 63);
  if (!/^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/.test(slug)) {
    fail("--slug 必须以小写字母开头，并且只能包含小写字母、数字和单个连字符");
  }

  const database = normalizedText(values.database, "--database", 63);
  if (!/^[a-z][a-z0-9_]*$/.test(database)) {
    fail("--database 必须以小写字母开头，并且只能包含小写字母、数字和下划线");
  }

  const permissionPrefix = normalizedText(
    values.permissionPrefix,
    "--permission-prefix",
    32,
  );
  if (!/^[a-z][a-z0-9_]*$/.test(permissionPrefix)) {
    fail(
      "--permission-prefix 必须以小写字母开头，并且只能包含小写字母、数字和下划线",
    );
  }

  const title = normalizedText(values.title, "--title", 80);
  return {
    help: false,
    slug,
    title,
    shortName: normalizedText(values.shortName ?? title, "--short-name", 24),
    database,
    permissionPrefix,
    apiBaseUrl: normalizeApiBaseUrl(values.apiBaseUrl ?? "/api/v1"),
  };
}

function runGit(root, args) {
  return execFileSync("git", args, {
    cwd: root,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

function assertSafeRepository(root) {
  if (!existsSync(join(root, ".git"))) {
    fail("当前目录不是 Git 仓库");
  }
  if (existsSync(join(root, PROJECT_FILE))) {
    fail("项目已经初始化，拒绝重复执行");
  }

  let origin = "";
  try {
    origin = runGit(root, ["remote", "get-url", "origin"]);
  } catch {
    // A local template copy may not have an origin yet.
  }
  if (/(?:^|[/:])[^/]+\/arc-admin(?:-starter)?(?:\.git)?$/i.test(origin)) {
    fail("不能在 arc-admin 框架源仓库执行初始化，请先从模板创建新的业务仓库");
  }

  const status = runGit(root, [
    "status",
    "--porcelain",
    "--untracked-files=all",
  ]);
  if (status) {
    fail("Git 工作区必须干净，请先提交或处理现有变更");
  }
}

function requiredFile(root, relativePath) {
  const path = join(root, relativePath);
  if (!existsSync(path)) {
    fail(`模板文件缺失: ${relativePath}`);
  }
  return path;
}

function replaceEnvValue(contents, key, replacement, fileLabel = "环境文件") {
  const pattern = new RegExp(`^${key}=(.*)$`, "m");
  const match = contents.match(pattern);
  if (!match) {
    fail(`${fileLabel} 缺少 ${key}`);
  }
  return contents.replace(pattern, `${key}=${replacement(match[1].trim())}`);
}

function unquote(value) {
  if (
    value.length >= 2 &&
    ((value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'")))
  ) {
    return value.slice(1, -1);
  }
  return value;
}

function buildEnvironment(example, options) {
  const withDatabase = replaceEnvValue(example, "DATABASE_URL", (value) => {
    let databaseUrl;
    try {
      databaseUrl = new URL(unquote(value));
    } catch {
      fail("backend/.env.example 中的 DATABASE_URL 无效");
    }
    databaseUrl.pathname = `/${options.database}`;
    return JSON.stringify(databaseUrl.toString());
  });

  const withServiceName = replaceEnvValue(
    withDatabase,
    "SERVICE_NAME",
    () => `${options.slug}-backend`,
  );
  return replaceEnvValue(
    withServiceName,
    "WEBAUTHN_RP_NAME",
    () => JSON.stringify(options.title),
  );
}

function buildObservabilityEnvironment(example, options, grafanaPassword) {
  const withProjectName = replaceEnvValue(
    example,
    "COMPOSE_PROJECT_NAME",
    () => `${options.slug}-observability`,
    "observability/.env.example",
  );
  const withMonitoringNetwork = replaceEnvValue(
    withProjectName,
    "MONITORING_NETWORK",
    () => `${options.slug}-monitoring`,
    "observability/.env.example",
  );
  return replaceEnvValue(
    withMonitoringNetwork,
    "GRAFANA_ADMIN_PASSWORD",
    () => grafanaPassword,
    "observability/.env.example",
  );
}

function buildProductionEnvironment(example, options, frameworkVersion) {
  const replacements = new Map([
    ["COMPOSE_PROJECT_NAME", options.slug],
    ["POSTGRES_DB", options.database],
    ["POSTGRES_USER", options.database],
    ["WEBAUTHN_RP_NAME", JSON.stringify(options.title)],
    ["SERVICE_NAME", `${options.slug}-backend`],
    ["BACKEND_IMAGE", `${options.slug}-backend:${frameworkVersion}`],
    ["FRONTEND_IMAGE", `${options.slug}-frontend:${frameworkVersion}`],
    ["MONITORING_NETWORK", `${options.slug}-monitoring`],
  ]);

  let environment = example;
  for (const [key, value] of replacements) {
    environment = replaceEnvValue(
      environment,
      key,
      () => value,
      "deployment/.env.production.example",
    );
  }
  return environment;
}

function buildRuntimeConfig(options) {
  return `// Public runtime settings only. Never put credentials or secrets in this file.
window.__ARC_ADMIN_CONFIG__ = window.__ARC_ADMIN_CONFIG__ || {
  appName: ${JSON.stringify(options.title)},
  appShortName: ${JSON.stringify(options.shortName)},
  appSlug: ${JSON.stringify(options.slug)},
  apiBaseUrl: ${JSON.stringify(options.apiBaseUrl)},
  themeStorageKey: ${JSON.stringify(`${options.slug}-theme`)},
};
`;
}

function buildReadme(source, options, frameworkVersion) {
  const start = source.indexOf(HEADER_START);
  const end = source.indexOf(HEADER_END);
  if (start < 0 || end < start) {
    fail("README.md 缺少项目标题初始化标记");
  }

  const header = `${HEADER_START}
# ${options.title}

基于 arc-admin ${frameworkVersion} 权限框架构建。项目标识：\`${options.slug}\`。
${HEADER_END}`;
  const withHeader = `${source.slice(0, start)}${header}${source.slice(end + HEADER_END.length)}`;
  const usageStart = withHeader.indexOf(TEMPLATE_USAGE_START);
  const usageEnd = withHeader.indexOf(TEMPLATE_USAGE_END);
  if (usageStart < 0 || usageEnd < usageStart) {
    fail("README.md 缺少模板使用说明初始化标记");
  }

  return `${withHeader.slice(0, usageStart)}${withHeader.slice(
    usageEnd + TEMPLATE_USAGE_END.length,
  )}`;
}

function buildProjectMetadata(options, frameworkVersion) {
  return `${JSON.stringify(
    {
      schemaVersion: 2,
      framework: {
        name: "arc-admin",
        version: frameworkVersion,
        initializedVersion: frameworkVersion,
      },
      project: {
        slug: options.slug,
        title: options.title,
        shortName: options.shortName,
        database: options.database,
        permissionPrefix: options.permissionPrefix,
        apiBaseUrl: options.apiBaseUrl,
      },
    },
    null,
    2,
  )}\n`;
}

function maybeRunDoctor(root, doctorMode) {
  if (
    doctorMode === "never" ||
    !existsSync(join(root, "frontend", "node_modules"))
  ) {
    return "skipped";
  }
  try {
    execFileSync("cargo", ["flow", "doctor"], { cwd: root, stdio: "inherit" });
    return "passed";
  } catch {
    return "failed";
  }
}

export function initializeProject(
  root,
  options,
  {
    doctorMode = "auto",
    grafanaSecretFactory = () => randomBytes(24).toString("base64url"),
  } = {},
) {
  assertSafeRepository(root);

  const frameworkVersionPath = requiredFile(root, "FRAMEWORK_VERSION");
  requiredFile(root, FRAMEWORK_MANIFEST);
  const readmePath = requiredFile(root, "README.md");
  const configPath = requiredFile(root, "frontend/public/config.js");
  const environmentExamplePath = requiredFile(root, "backend/.env.example");
  const observabilityExamplePath = requiredFile(
    root,
    "observability/.env.example",
  );
  const productionEnvironmentExamplePath = requiredFile(
    root,
    "deployment/.env.production.example",
  );
  const environmentPath = join(root, "backend/.env");
  const observabilityPath = join(root, "observability/.env");
  const productionEnvironmentPath = join(root, "deployment/.env.production");
  if (existsSync(environmentPath)) {
    fail("backend/.env 已存在，拒绝覆盖本地配置");
  }
  if (existsSync(observabilityPath)) {
    fail("observability/.env 已存在，拒绝覆盖本地配置");
  }
  if (existsSync(productionEnvironmentPath)) {
    fail("deployment/.env.production 已存在，拒绝覆盖本地配置");
  }

  const frameworkVersion = readFileSync(frameworkVersionPath, "utf8").trim();
  if (!frameworkVersion) {
    fail("FRAMEWORK_VERSION 不能为空");
  }
  if (!RELEASE_VERSION.test(frameworkVersion)) {
    fail("只能从正式版本初始化项目，FRAMEWORK_VERSION 必须是 vX.Y.Z");
  }

  const environment = buildEnvironment(
    readFileSync(environmentExamplePath, "utf8"),
    options,
  );
  const observabilityEnvironment = buildObservabilityEnvironment(
    readFileSync(observabilityExamplePath, "utf8"),
    options,
    grafanaSecretFactory(),
  );
  const runtimeConfig = buildRuntimeConfig(options);
  const readme = buildReadme(
    readFileSync(readmePath, "utf8"),
    options,
    frameworkVersion,
  );
  const metadata = buildProjectMetadata(options, frameworkVersion);
  const productionEnvironment = buildProductionEnvironment(
    readFileSync(productionEnvironmentExamplePath, "utf8"),
    options,
    frameworkVersion,
  );

  writeFileSync(environmentPath, environment, {
    encoding: "utf8",
    mode: 0o600,
    flag: "wx",
  });
  writeFileSync(observabilityPath, observabilityEnvironment, {
    encoding: "utf8",
    mode: 0o600,
    flag: "wx",
  });
  writeFileSync(productionEnvironmentPath, productionEnvironment, {
    encoding: "utf8",
    mode: 0o600,
    flag: "wx",
  });
  writeFileSync(configPath, runtimeConfig, "utf8");
  writeFileSync(readmePath, readme, "utf8");
  writeFileSync(join(root, PROJECT_FILE), metadata, {
    encoding: "utf8",
    flag: "wx",
  });

  return { frameworkVersion, doctorStatus: maybeRunDoctor(root, doctorMode) };
}

function main() {
  try {
    const options = parseArguments(process.argv.slice(2));
    if (options.help) {
      process.stdout.write(HELP);
      return;
    }

    const result = initializeProject(REPO_ROOT, options);
    console.log(`项目初始化完成: ${options.title} (${options.slug})`);
    console.log(`框架版本: ${result.frameworkVersion}`);
    console.log(`业务权限前缀: ${options.permissionPrefix}:*`);
    if (result.doctorStatus === "skipped") {
      console.log("依赖尚未安装，已跳过环境体检。");
    } else if (result.doctorStatus === "failed") {
      console.warn("项目已初始化，但环境体检未通过，请修复提示的问题。");
    }
    console.log("下一步: cd frontend && npm ci && cd .. && cargo flow doctor");
  } catch (error) {
    console.error(
      `初始化失败: ${error instanceof Error ? error.message : String(error)}`,
    );
    process.exitCode = 1;
  }
}

if (resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  main();
}
