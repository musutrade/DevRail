import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");

async function read(path) {
  return readFile(resolve(root, path), "utf8");
}

function requireText(content, expected, path) {
  if (!content.includes(expected)) {
    throw new Error(`${path} 缺少必需配置：${expected}`);
  }
}

const [migration, repository, command, dockerfile, compose, documentation] =
  await Promise.all([
    read("backend/migrations/20260808120000_protect_audit_logs.sql"),
    read("backend/src/repositories/audit_logs.rs"),
    read("backend/src/bin/archive_audit_logs.rs"),
    read("backend/Dockerfile"),
    read("compose.production.yaml"),
    read("docs/audit-retention.md"),
  ]);

for (const guard of [
  "BEFORE UPDATE OR DELETE ON audit_logs",
  "BEFORE TRUNCATE ON audit_logs",
  "arc_admin.audit_maintenance",
]) {
  requireText(migration, guard, "审计防篡改迁移");
}
requireText(repository, "delete_archived", "审计 Repository");
requireText(repository, "set_config('arc_admin.audit_maintenance'", "审计 Repository");
requireText(command, "sync_all()", "审计归档命令");
requireText(command, "Sha256", "审计归档命令");
requireText(command, "fs::rename", "审计归档命令");
if (command.indexOf("write_archive") > command.indexOf("delete_archived")) {
  throw new Error("审计归档命令必须先落盘再删除数据库记录");
}
requireText(dockerfile, "archive_audit_logs", "后端生产镜像");
requireText(compose, "profiles: [maintenance]", "生产 Compose");
requireText(compose, "AUDIT_RETENTION_DAYS", "生产 Compose");
requireText(documentation, "不可变对象存储", "审计归档文档");

console.log("审计日志保留与归档配置检查通过");
