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

function checkObservability() {
  const gitignore = read(".gitignore");
  const compose = read("observability/compose.yaml");
  const alloy = read("observability/alloy/config.alloy");
  const prometheus = read("observability/prometheus/prometheus.yaml");
  const blackbox = read("observability/blackbox/blackbox.yaml");
  const loki = read("observability/loki/config.yaml");
  const alerts = read(
    "observability/grafana/provisioning/alerting/log-alerts.yaml",
  );
  const dashboard = JSON.parse(
    read("observability/grafana/dashboards/application-logs.json"),
  );
  const metricsDashboard = JSON.parse(
    read("observability/grafana/dashboards/application-metrics.json"),
  );
  const metricsAlerts = read(
    "observability/grafana/provisioning/alerting/metrics-alerts.yaml",
  );
  const prometheusDatasource = read(
    "observability/grafana/provisioning/datasources/prometheus.yaml",
  );
  const productionAlertingCompose = read(
    "observability/compose.production-alerting.yaml",
  );
  const productionContactPoints = read(
    "observability/grafana/provisioning/alerting/production/contact-points.yaml",
  );
  const productionNotificationPolicy = read(
    "observability/grafana/provisioning/alerting/production/notification-policy.yaml",
  );
  const observabilityEnvironment = read("observability/.env.example");

  for (const image of [
    "grafana/loki:3.7.2",
    "grafana/alloy:v1.18.1",
    "grafana/grafana:13.1.0",
    "prom/prometheus:v3.13.2",
    "quay.io/prometheus/blackbox-exporter:v0.28.0",
    "grafana/tempo:2.10.7",
  ]) {
    requireText(compose, `image: ${image}`, "Compose");
  }
  if (/image:\s*\S+:latest\b/.test(compose)) {
    throw new Error("Compose 禁止使用 latest 镜像标签");
  }
  for (const binding of [
    '"127.0.0.1:3000:3000"',
    '"127.0.0.1:3100:3100"',
    '"127.0.0.1:12345:12345"',
    '"127.0.0.1:9090:9090"',
    '"127.0.0.1:9115:9115"',
    '"127.0.0.1:3200:3200"',
  ]) {
    requireText(compose, binding, "Compose 本机端口");
  }
  requireText(compose, "GRAFANA_ADMIN_PASSWORD:?", "Grafana 管理员密码保护");
  requireText(compose, "alerting/log-alerts.yaml:ro", "Grafana 日志告警挂载");
  requireText(compose, "alerting/metrics-alerts.yaml:ro", "Grafana 指标告警挂载");
  requireText(compose, "/var/run/docker.sock:ro", "Docker 日志采集");
  requireText(compose, "external: true", "生产监控网络隔离");
  requireText(gitignore, "/observability/logs/*.jsonl", "本地日志忽略规则");

  for (const expected of [
    'regex         = "enabled"',
    'target_label  = "service_name"',
    'target_label  = "environment"',
    'service_name = "span.service"',
    'trace_id     = "fields.trace_id"',
    "stage.structured_metadata",
    'trace_id    = ""',
    'user_id     = ""',
    'status_code = ""',
    "loki.source.file",
  ]) {
    requireText(alloy, expected, "Alloy");
  }

  for (const expected of [
    "store: tsdb",
    "schema: v13",
    "retention_enabled: true",
    "retention_period: 720h",
    "allow_structured_metadata: true",
    "ingestion_rate_mb: 8",
    "max_line_size: 256KB",
  ]) {
    requireText(loki, expected, "Loki");
  }

  for (const expected of [
    "job_name: arc-admin-backend",
    "metrics_path: /metrics",
    'targets: ["backend:8080"]',
    "job_name: arc-admin-entry-probe",
    "replacement: blackbox:9115",
  ]) {
    requireText(prometheus, expected, "Prometheus");
  }
  requireText(blackbox, "valid_status_codes: [200]", "Blackbox");
  requireText(prometheusDatasource, "uid: prometheus", "Prometheus 数据源");
  requireText(metricsAlerts, "uid: application-entry-down", "指标告警");

  for (const variable of [
    "GRAFANA_ALERT_CONTACT_TYPE",
    "GRAFANA_ALERT_WEBHOOK_URL",
  ]) {
    requireText(
      productionAlertingCompose,
      `${variable}:?`,
      "生产告警 Compose 环境保护",
    );
    if (!new RegExp(`^${variable}=\\s*$`, "m").test(observabilityEnvironment)) {
      throw new Error(`可观测性环境示例中的 ${variable} 必须保持为空`);
    }
  }
  for (const expected of [
    "name: arc-admin-production-alerts",
    "uid: arc-admin-production-alerts",
    "type: $GRAFANA_ALERT_CONTACT_TYPE",
    "url: $GRAFANA_ALERT_WEBHOOK_URL",
    "disableResolveMessage: false",
  ]) {
    requireText(productionContactPoints, expected, "Grafana 生产联系点");
  }
  for (const expected of [
    "receiver: arc-admin-production-alerts",
    "group_by: [alertname]",
    "group_wait: 30s",
    "group_interval: 5m",
    "repeat_interval: 4h",
  ]) {
    requireText(productionNotificationPolicy, expected, "Grafana 生产通知策略");
  }
  if (/https?:\/\//.test(productionContactPoints)) {
    throw new Error("Grafana 生产联系点禁止包含实际 Webhook 地址");
  }

  for (const expected of [
    "uid: application-error-burst",
    "uid: application-http-5xx",
    'level="ERROR"',
    "status_code >= 500",
  ]) {
    requireText(alerts, expected, "Grafana 告警");
  }

  if (dashboard.uid !== "application-logs") {
    throw new Error("日志仪表盘 uid 必须为 application-logs");
  }
  const variableNames = new Set(
    dashboard.templating?.list?.map((variable) => variable.name),
  );
  for (const variable of ["service", "environment", "level", "trace_id"]) {
    if (!variableNames.has(variable)) {
      throw new Error(`日志仪表盘缺少 ${variable} 筛选变量`);
    }
  }
  if (!dashboard.panels?.some((panel) => panel.type === "logs")) {
    throw new Error("日志仪表盘缺少日志明细面板");
  }
  if (metricsDashboard.uid !== "application-metrics") {
    throw new Error("指标仪表盘 uid 必须为 application-metrics");
  }
  for (const expression of [
    "arc_admin_http_requests_total",
    "arc_admin_http_request_duration_seconds_bucket",
    "arc_admin_db_pool_acquired",
    "probe_success",
  ]) {
    if (
      !metricsDashboard.panels?.some((panel) =>
        panel.targets?.some((target) => target.expr?.includes(expression)),
      )
    ) {
      throw new Error(`指标仪表盘缺少 ${expression} 查询`);
    }
  }
}

try {
  checkObservability();
  console.log("集中日志配置质量门禁通过");
} catch (error) {
  console.error(
    `集中日志配置质量门禁失败: ${error instanceof Error ? error.message : String(error)}`,
  );
  process.exitCode = 1;
}
