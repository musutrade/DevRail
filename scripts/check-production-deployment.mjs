import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function read(relativePath) {
  return readFileSync(join(REPO_ROOT, relativePath), "utf8");
}

function requireText(source, expected, label) {
  if (!source.includes(expected)) {
    throw new Error(`${label} 缺少必需配置: ${expected}`);
  }
}

function checkDockerfile(source, label) {
  if ((source.match(/^FROM /gm) ?? []).length < 2) {
    throw new Error(`${label} 必须使用多阶段构建`);
  }
  requireText(source, "USER ", `${label} 非 root 用户`);
  requireText(source, "HEALTHCHECK", `${label} 健康检查`);
  const images = [...source.matchAll(/^FROM\s+(\S+)/gm)].map((match) => match[1]);
  if (images.some((image) => !/@sha256:[a-f0-9]{64}$/i.test(image))) {
    throw new Error(`${label} 的每个基础镜像必须固定 sha256 digest`);
  }
}

function checkProductionDeployment() {
  const compose = read("compose.production.yaml");
  const backendDockerfile = read("backend/Dockerfile");
  const frontendDockerfile = read("frontend/Dockerfile");
  const nginx = read("deployment/nginx/nginx.conf");
  const envExample = read("deployment/.env.production.example");
  const frameworkManifest = read(".arc-framework/manifest.json");
  const frameworkVersion = read("FRAMEWORK_VERSION").trim();

  checkDockerfile(backendDockerfile, "后端 Dockerfile");
  checkDockerfile(frontendDockerfile, "前端 Dockerfile");

  for (const expected of [
    "postgres:17.10-bookworm@sha256:9b18b78397054fce88a9552e9d5a3ad5bb7fd258c5b3cc1c5028e46373d6ea8f",
    'AUTO_MIGRATE: "false"',
    "condition: service_completed_successfully",
    "read_only: true",
    "cap_drop: [ALL]",
    "no-new-privileges:true",
    "healthcheck:",
    "mem_limit:",
    "pids_limit:",
    "archive_mode=on",
    "postgres-wal-archive",
    "MONITORING_NETWORK",
    "- monitoring",
    'profiles: [maintenance]',
    "AUDIT_ARCHIVE_HOST_DIR",
  ]) {
    requireText(compose, expected, "生产 Compose");
  }
  requireText(
    compose,
    "DEVRAIL_GIT_WEBHOOK_SECRET: ${DEVRAIL_GIT_WEBHOOK_SECRET:?",
    "生产 Compose",
  );
  requireText(
    compose,
    "DEVRAIL_REPAIR_CALLBACK_SECRET: ${DEVRAIL_REPAIR_CALLBACK_SECRET:?",
    "生产 Compose",
  );

  for (const expected of [
    "listen 8443 ssl",
    "ssl_protocols TLSv1.2 TLSv1.3",
    "Strict-Transport-Security",
    "Content-Security-Policy",
    "X-Content-Type-Options",
    "proxy_set_header X-Forwarded-For",
    "proxy_connect_timeout",
    "proxy_read_timeout",
  ]) {
    requireText(nginx, expected, "Nginx");
  }

  for (const variable of ["POSTGRES_PASSWORD", "DATABASE_URL"]) {
    if (!new RegExp(`^${variable}=\\s*$`, "m").test(envExample)) {
      throw new Error(`生产环境示例中的 ${variable} 必须保持为空`);
    }
  }
  requireText(envExample, "MONITORING_NETWORK=arc-admin-monitoring", "生产环境示例");
  for (const expected of [
    `BACKEND_IMAGE=arc-admin-backend:${frameworkVersion}`,
    `FRONTEND_IMAGE=arc-admin-frontend:${frameworkVersion}`,
  ]) {
    requireText(envExample, expected, "生产环境示例");
  }
  for (const expected of [
    `arc-admin-backend:${frameworkVersion}`,
    `arc-admin-frontend:${frameworkVersion}`,
  ]) {
    requireText(compose, expected, "生产 Compose 默认镜像");
  }
  for (const managed of [
    '"deployment"',
    '"backend/Dockerfile"',
    '"frontend/Dockerfile"',
    '"compose.production.yaml"',
  ]) {
    requireText(frameworkManifest, managed, "框架文件清单");
  }
}

try {
  checkProductionDeployment();
  console.log("生产部署配置质量门禁通过");
} catch (error) {
  console.error(
    `生产部署配置质量门禁失败: ${error instanceof Error ? error.message : String(error)}`,
  );
  process.exitCode = 1;
}
