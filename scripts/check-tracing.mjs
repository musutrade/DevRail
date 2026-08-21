import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const read = (path) => readFile(resolve(root, path), "utf8");

function requireText(content, expected, path) {
  if (!content.includes(expected)) {
    throw new Error(`${path} 缺少必需配置：${expected}`);
  }
}

const [telemetry, compose, tempo, datasource, backendEnv, compliance, roadmap] =
  await Promise.all([
    read("backend/src/telemetry.rs"),
    read("observability/compose.yaml"),
    read("observability/tempo/config.yaml"),
    read("observability/grafana/provisioning/datasources/tempo.yaml"),
    read("backend/.env.example"),
    read("docs/high-compliance.md"),
    read("docs/security-roadmap.md"),
  ]);

for (const expected of [
  "OTEL_EXPORTER_OTLP_ENDPOINT",
  "TraceContextPropagator",
  "with_batch_exporter",
  "shutdown_with_timeout",
]) {
  requireText(telemetry, expected, "后端链路追踪");
}
requireText(compose, "profiles: [tracing]", "可观测性 Compose");
requireText(compose, "grafana/tempo:2.10.7", "可观测性 Compose");
requireText(tempo, "endpoint: 0.0.0.0:4318", "Tempo");
requireText(tempo, "block_retention: 72h", "Tempo");
requireText(datasource, "uid: tempo", "Tempo 数据源");
requireText(backendEnv, "OTEL_EXPORTER_OTLP_ENDPOINT=", "后端环境示例");
requireText(compliance, "不构成", "高合规文档");
requireText(compliance, "不可变", "高合规文档");
requireText(roadmap, "已实现：`super_admin` 多因素认证", "安全能力状态");
requireText(roadmap, "## 生产责任", "安全能力状态");

console.log("可选链路追踪与高合规基线检查通过");
