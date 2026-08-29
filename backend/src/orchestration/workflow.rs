//! Strict, fail-closed repository workflow loading and snapshot rendering.

use crate::models::{ContinuationPolicy, RepairPolicy};
use minijinja::value::Value as TemplateValue;
use minijinja::{Environment, UndefinedBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const WORKFLOW_FILE: &str = "WORKFLOW.md";
const MAX_WORKFLOW_BYTES: usize = 262_144;
const DEFAULT_WORKFLOW: &str = include_str!("../../../WORKFLOW.md");
const ALLOWED_TEMPLATE_VARIABLES: &[&str] = &[
    "environment.name",
    "environment.workspace_root",
    "repository.default_branch",
    "repository.name",
    "task.acceptance_criteria",
    "task.background",
    "task.constraints",
    "task.goal",
    "task.id",
    "task.title",
];
const ALLOWED_HOOKS: &[&str] = &["cargo-flow-scope", "cargo-flow-verify"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowSource {
    Default,
    Repository,
    Legacy,
}

impl WorkflowSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Repository => "repository",
            Self::Legacy => "legacy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Implementation,
    Review,
    Maintenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationEvent {
    AwaitingApproval,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowToolPolicy {
    pub allow: BTreeSet<String>,
    pub network: bool,
    pub dangerous_commands_require_approval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowRetryPolicy {
    pub max_attempts: i32,
    pub base_delay_seconds: i64,
    pub max_delay_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowHooks {
    pub before_run: Vec<String>,
    pub after_run: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowNotifications {
    pub events: BTreeSet<NotificationEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowConfig {
    pub version: String,
    pub execution_mode: ExecutionMode,
    pub tools: WorkflowToolPolicy,
    pub quality_gates: Vec<String>,
    pub timeout_seconds: i64,
    pub stall_timeout_seconds: i64,
    pub retry: WorkflowRetryPolicy,
    pub hooks: WorkflowHooks,
    pub notifications: WorkflowNotifications,
    #[serde(default)]
    pub continuation: ContinuationPolicy,
    #[serde(default)]
    pub repair: RepairPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSnapshot {
    pub schema_version: u32,
    pub source: WorkflowSource,
    pub declared_version: String,
    pub digest: String,
    pub config: WorkflowConfig,
    pub prompt_template: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedWorkflowSnapshot {
    #[serde(flatten)]
    pub workflow: WorkflowSnapshot,
    pub rendered_prompt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowTaskContext<'a> {
    pub task: WorkflowTaskTemplateContext<'a>,
    pub repository: WorkflowRepositoryTemplateContext<'a>,
    pub environment: WorkflowEnvironmentTemplateContext<'a>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowTaskTemplateContext<'a> {
    pub id: i64,
    pub title: &'a str,
    pub goal: &'a str,
    pub background: Option<&'a str>,
    pub acceptance_criteria: Option<&'a str>,
    pub constraints: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowRepositoryTemplateContext<'a> {
    pub name: Option<&'a str>,
    pub default_branch: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowEnvironmentTemplateContext<'a> {
    pub name: &'a str,
    pub workspace_root: &'a str,
}

#[derive(Debug, Clone)]
pub struct PlatformWorkflowPolicy {
    pub allowed_tools: BTreeSet<String>,
    pub network_allowed: bool,
    pub max_timeout_seconds: i64,
    pub max_stall_timeout_seconds: i64,
    pub max_attempts: i32,
    pub max_retry_delay_seconds: i64,
}

impl PlatformWorkflowPolicy {
    pub fn secure_default(max_timeout_seconds: i64) -> Self {
        Self {
            allowed_tools: ["read_file", "search", "apply_patch", "cargo_flow"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            network_allowed: false,
            max_timeout_seconds,
            max_stall_timeout_seconds: 600,
            max_attempts: 10,
            max_retry_delay_seconds: 3_600,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowErrorKind {
    Path,
    Size,
    FrontMatter,
    Schema,
    Template,
    Policy,
    Io,
}

impl WorkflowErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Size => "size",
            Self::FrontMatter => "front_matter",
            Self::Schema => "schema",
            Self::Template => "template",
            Self::Policy => "policy",
            Self::Io => "io",
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct WorkflowError {
    kind: WorkflowErrorKind,
    message: String,
    candidate_digest: Option<String>,
}

impl WorkflowError {
    pub const fn kind(&self) -> WorkflowErrorKind {
        self.kind
    }

    pub fn candidate_digest(&self) -> Option<&str> {
        self.candidate_digest.as_deref()
    }

    fn new(kind: WorkflowErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            candidate_digest: None,
        }
    }

    fn with_candidate_digest(mut self, digest: String) -> Self {
        self.candidate_digest = Some(digest);
        self
    }
}

pub async fn load_repository_workflow(
    controlled_root: &Path,
    repository_root: &Path,
    policy: &PlatformWorkflowPolicy,
) -> Result<WorkflowSnapshot, WorkflowError> {
    let controlled = tokio::fs::canonicalize(controlled_root)
        .await
        .map_err(|_| WorkflowError::new(WorkflowErrorKind::Path, "受控工作区根目录不可访问"))?;
    let repository = tokio::fs::canonicalize(repository_root)
        .await
        .map_err(|_| WorkflowError::new(WorkflowErrorKind::Path, "仓库工作区不可访问"))?;
    if !repository.starts_with(&controlled) {
        return Err(WorkflowError::new(
            WorkflowErrorKind::Path,
            "仓库工作区不在受控根目录内",
        ));
    }

    let workflow_path = repository.join(WORKFLOW_FILE);
    let metadata = match tokio::fs::symlink_metadata(&workflow_path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return parse_workflow(DEFAULT_WORKFLOW, WorkflowSource::Default, policy)
        }
        Err(_) => {
            return Err(WorkflowError::new(
                WorkflowErrorKind::Io,
                "无法读取 workflow 文件元数据",
            ))
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(WorkflowError::new(
            WorkflowErrorKind::Path,
            "WORKFLOW.md 不允许使用符号链接",
        ));
    }
    if !metadata.is_file() {
        return Err(WorkflowError::new(
            WorkflowErrorKind::Path,
            "WORKFLOW.md 不是普通文件",
        ));
    }
    if metadata.len() > MAX_WORKFLOW_BYTES as u64 {
        return Err(WorkflowError::new(
            WorkflowErrorKind::Size,
            "WORKFLOW.md 超过 256 KiB 限制",
        ));
    }
    let canonical_file = tokio::fs::canonicalize(&workflow_path)
        .await
        .map_err(|_| WorkflowError::new(WorkflowErrorKind::Path, "WORKFLOW.md 路径无效"))?;
    if canonical_file.parent() != Some(repository.as_path()) {
        return Err(WorkflowError::new(
            WorkflowErrorKind::Path,
            "WORKFLOW.md 解析路径越过仓库根目录",
        ));
    }
    let bytes = tokio::fs::read(&canonical_file)
        .await
        .map_err(|_| WorkflowError::new(WorkflowErrorKind::Io, "无法读取 WORKFLOW.md"))?;
    if bytes.len() > MAX_WORKFLOW_BYTES {
        return Err(WorkflowError::new(
            WorkflowErrorKind::Size,
            "WORKFLOW.md 超过 256 KiB 限制",
        ));
    }
    let content = std::str::from_utf8(&bytes).map_err(|_| {
        WorkflowError::new(WorkflowErrorKind::FrontMatter, "WORKFLOW.md 必须使用 UTF-8")
    })?;
    parse_workflow(content, WorkflowSource::Repository, policy)
        .map_err(|error| error.with_candidate_digest(candidate_digest(&bytes)))
}

pub fn parse_workflow(
    content: &str,
    source: WorkflowSource,
    policy: &PlatformWorkflowPolicy,
) -> Result<WorkflowSnapshot, WorkflowError> {
    if content.len() > MAX_WORKFLOW_BYTES {
        return Err(WorkflowError::new(
            WorkflowErrorKind::Size,
            "WORKFLOW.md 超过 256 KiB 限制",
        ));
    }
    let normalized = content.replace("\r\n", "\n");
    let (front_matter, prompt) = split_front_matter(&normalized)?;
    let config: WorkflowConfig = serde_yaml_ng::from_str(front_matter).map_err(|error| {
        let location = error
            .location()
            .map(|location| format!("（第 {} 行，第 {} 列）", location.line(), location.column()))
            .unwrap_or_default();
        WorkflowError::new(
            WorkflowErrorKind::Schema,
            format!("workflow YAML 配置无效{location}"),
        )
    })?;
    validate_config(&config, policy)?;
    validate_template(prompt)?;
    let digest = workflow_digest(&config, prompt)?;
    Ok(WorkflowSnapshot {
        schema_version: 1,
        source,
        declared_version: config.version.clone(),
        digest,
        config,
        prompt_template: prompt.to_string(),
    })
}

pub fn render_workflow(
    workflow: &WorkflowSnapshot,
    context: &WorkflowTaskContext<'_>,
) -> Result<RenderedWorkflowSnapshot, WorkflowError> {
    let environment = template_environment();
    let template = environment
        .template_from_str(&workflow.prompt_template)
        .map_err(|error| template_error(&error))?;
    let rendered_prompt = template
        .render(context)
        .map_err(|error| template_error(&error))?;
    Ok(RenderedWorkflowSnapshot {
        workflow: workflow.clone(),
        rendered_prompt,
    })
}

pub fn candidate_digest(content: &[u8]) -> String {
    hex::encode(Sha256::digest(content))
}

pub fn task_dispatch_snapshot(
    task: &Value,
    task_revision: i64,
    workflow: &RenderedWorkflowSnapshot,
) -> Value {
    let mut snapshot = task.as_object().cloned().unwrap_or_default();
    snapshot.insert("schemaVersion".to_string(), json!(1));
    snapshot.insert("taskRevision".to_string(), json!(task_revision));
    snapshot.insert("workflow".to_string(), json!(workflow));
    Value::Object(snapshot)
}

pub fn snapshot_digest(snapshot: &Value) -> Result<String, WorkflowError> {
    let bytes = serde_json::to_vec(snapshot).map_err(|_| {
        WorkflowError::new(WorkflowErrorKind::Schema, "workflow 快照无法规范化序列化")
    })?;
    Ok(candidate_digest(&bytes))
}

fn split_front_matter(content: &str) -> Result<(&str, &str), WorkflowError> {
    let rest = content.strip_prefix("---\n").ok_or_else(|| {
        WorkflowError::new(
            WorkflowErrorKind::FrontMatter,
            "WORKFLOW.md 必须以 YAML front matter 开始",
        )
    })?;
    let (front_matter, prompt) = rest.split_once("\n---\n").ok_or_else(|| {
        WorkflowError::new(
            WorkflowErrorKind::FrontMatter,
            "WORKFLOW.md 缺少 front matter 结束分隔符",
        )
    })?;
    if prompt.trim().is_empty() {
        return Err(WorkflowError::new(
            WorkflowErrorKind::FrontMatter,
            "WORKFLOW.md 提示正文不能为空",
        ));
    }
    Ok((front_matter, prompt.trim()))
}

fn validate_config(
    config: &WorkflowConfig,
    policy: &PlatformWorkflowPolicy,
) -> Result<(), WorkflowError> {
    if config.version.is_empty()
        || config.version.len() > 64
        || !config
            .version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(policy_error("workflow version 格式无效"));
    }
    if config.tools.network && !policy.network_allowed {
        return Err(policy_error("workflow 不得扩大网络权限"));
    }
    if !config.tools.dangerous_commands_require_approval {
        return Err(policy_error("危险命令必须保持审批"));
    }
    if config.tools.allow.is_empty() || !config.tools.allow.is_subset(&policy.allowed_tools) {
        return Err(policy_error("workflow 工具集合超出平台允许范围"));
    }
    if !(60..=policy.max_timeout_seconds).contains(&config.timeout_seconds) {
        return Err(policy_error("workflow 运行超时超出平台范围"));
    }
    if !(10..=policy.max_stall_timeout_seconds).contains(&config.stall_timeout_seconds)
        || config.stall_timeout_seconds >= config.timeout_seconds
    {
        return Err(policy_error("workflow stall 超时超出平台范围"));
    }
    if !(1..=policy.max_attempts).contains(&config.retry.max_attempts)
        || config.retry.base_delay_seconds < 1
        || config.retry.max_delay_seconds < config.retry.base_delay_seconds
        || config.retry.max_delay_seconds > policy.max_retry_delay_seconds
    {
        return Err(policy_error("workflow 重试策略超出平台范围"));
    }
    if config.quality_gates.is_empty()
        || config.quality_gates.len() > 16
        || config
            .quality_gates
            .iter()
            .any(|gate| !valid_identifier(gate))
    {
        return Err(policy_error("workflow 质量门禁标识无效"));
    }
    if config
        .hooks
        .before_run
        .iter()
        .chain(&config.hooks.after_run)
        .any(|hook| !ALLOWED_HOOKS.contains(&hook.as_str()))
    {
        return Err(policy_error("workflow hook 不在平台白名单内"));
    }
    validate_continuation_policy(&config.continuation)?;
    validate_repair_policy(&config.repair)?;
    Ok(())
}

fn validate_continuation_policy(policy: &ContinuationPolicy) -> Result<(), WorkflowError> {
    if policy.allowed_triggers.len() > 3
        || (policy.enabled && policy.allowed_triggers.is_empty())
        || !(1..=3).contains(&policy.max_continuations)
        || !(1..=3).contains(&policy.max_chain_depth)
        || !(1..=16 * 1024).contains(&policy.max_context_bytes)
        || !(10..=3_600).contains(&policy.claim_lease_seconds)
        || !(1..=10).contains(&policy.max_dispatch_attempts)
        || policy.retry_base_delay_seconds < 1
        || policy.retry_max_delay_seconds < policy.retry_base_delay_seconds
        || policy.retry_max_delay_seconds > 3_600
    {
        return Err(policy_error("continuation 策略超出平台范围"));
    }
    Ok(())
}

fn validate_repair_policy(policy: &RepairPolicy) -> Result<(), WorkflowError> {
    let auto_allowed = policy
        .auto_categories
        .iter()
        .all(|category| matches!(category, crate::models::DevRailRepairRiskCategory::LowRisk));
    let approval_allowed = policy.approval_categories.iter().all(|category| {
        matches!(
            category,
            crate::models::DevRailRepairRiskCategory::LogicalChange
                | crate::models::DevRailRepairRiskCategory::DependencyChange
                | crate::models::DevRailRepairRiskCategory::RemoteWrite
                | crate::models::DevRailRepairRiskCategory::SecurityChange
        )
    });
    if !(1..=5).contains(&policy.max_repairs)
        || !(1..=1_000).contains(&policy.max_cost_units)
        || !(256..=64 * 1024).contains(&policy.max_diagnostic_bytes)
        || !(60..=86_400).contains(&policy.evidence_max_age_seconds)
        || !(10..=3_600).contains(&policy.claim_lease_seconds)
        || !(1..=10).contains(&policy.max_dispatch_attempts)
        || policy.retry_base_delay_seconds < 1
        || policy.retry_max_delay_seconds < policy.retry_base_delay_seconds
        || policy.retry_max_delay_seconds > 3_600
        || !auto_allowed
        || !approval_allowed
    {
        return Err(policy_error("repair 策略超出平台范围"));
    }
    Ok(())
}

fn validate_template(prompt: &str) -> Result<(), WorkflowError> {
    let environment = template_environment();
    let template = environment
        .template_from_str(prompt)
        .map_err(|error| template_error(&error))?;
    let undeclared = template.undeclared_variables(true);
    if let Some(variable) = undeclared
        .iter()
        .find(|variable| !ALLOWED_TEMPLATE_VARIABLES.contains(&variable.as_str()))
    {
        return Err(WorkflowError::new(
            WorkflowErrorKind::Template,
            format!("workflow 模板包含未知变量 {variable}"),
        ));
    }
    let validation_context = WorkflowTaskContext {
        task: WorkflowTaskTemplateContext {
            id: 1,
            title: "任务",
            goal: "目标",
            background: Some("背景"),
            acceptance_criteria: Some("验收标准"),
            constraints: Some("约束"),
        },
        repository: WorkflowRepositoryTemplateContext {
            name: Some("仓库"),
            default_branch: Some("main"),
        },
        environment: WorkflowEnvironmentTemplateContext {
            name: "环境",
            workspace_root: "/受控工作区",
        },
    };
    template
        .render(validation_context)
        .map_err(|error| template_error(&error))?;
    Ok(())
}

fn template_environment() -> Environment<'static> {
    let mut environment = Environment::new();
    environment.set_undefined_behavior(UndefinedBehavior::Strict);
    environment.add_filter("trim", |value: String| value.trim().to_string());
    environment.add_filter("lower", |value: String| value.to_lowercase());
    environment.add_filter("upper", |value: String| value.to_uppercase());
    environment.add_filter(
        "default",
        |value: TemplateValue, fallback: TemplateValue| {
            if value.is_none() || value.is_undefined() {
                fallback
            } else {
                value
            }
        },
    );
    environment
}

fn workflow_digest(config: &WorkflowConfig, prompt: &str) -> Result<String, WorkflowError> {
    let normalized = json!({"config": config, "prompt": prompt});
    snapshot_digest(&normalized)
}

fn policy_error(message: &'static str) -> WorkflowError {
    WorkflowError::new(WorkflowErrorKind::Policy, message)
}

fn template_error(error: &minijinja::Error) -> WorkflowError {
    let location = error
        .line()
        .map(|line| format!("（第 {line} 行）"))
        .unwrap_or_default();
    WorkflowError::new(
        WorkflowErrorKind::Template,
        format!("workflow 模板无效{location}"),
    )
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub fn workflow_path(repository_root: &Path) -> PathBuf {
    repository_root.join(WORKFLOW_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn policy() -> PlatformWorkflowPolicy {
        PlatformWorkflowPolicy::secure_default(3_600)
    }

    fn valid_workflow() -> &'static str {
        DEFAULT_WORKFLOW
    }

    #[test]
    fn parses_repository_contract_and_has_stable_digest() {
        let first = parse_workflow(valid_workflow(), WorkflowSource::Repository, &policy())
            .expect("valid workflow");
        let second = parse_workflow(valid_workflow(), WorkflowSource::Repository, &policy())
            .expect("valid workflow");
        assert_eq!(first.digest, second.digest);
        assert_eq!(first.declared_version, "devrail-v1");
        assert_eq!(first.source, WorkflowSource::Repository);
        assert!(!first.config.continuation.enabled);
        assert_eq!(
            first.config.continuation.max_context_bytes,
            crate::models::DEFAULT_CONTINUATION_MAX_CONTEXT_BYTES
        );
    }

    #[test]
    fn continuation_policy_round_trips_and_rejects_unknown_trigger() {
        let workflow = valid_workflow().replace(
            "quality_gates:\n",
            "continuation:\n  enabled: true\n  allowed_triggers:\n    - user_context\n  max_continuations: 2\n  max_chain_depth: 2\n  max_context_bytes: 4096\n  claim_lease_seconds: 30\n  max_dispatch_attempts: 2\n  retry_base_delay_seconds: 2\n  retry_max_delay_seconds: 60\nquality_gates:\n",
        );
        let parsed = parse_workflow(&workflow, WorkflowSource::Repository, &policy())
            .expect("continuation policy");
        let encoded = serde_json::to_value(&parsed.config.continuation).expect("encode policy");
        let decoded: crate::models::ContinuationPolicy =
            serde_json::from_value(encoded).expect("decode policy");
        assert_eq!(decoded, parsed.config.continuation);

        let invalid = workflow.replace("- user_context", "- unknown_trigger");
        assert_eq!(
            parse_workflow(&invalid, WorkflowSource::Repository, &policy())
                .expect_err("unknown continuation trigger")
                .kind(),
            WorkflowErrorKind::Schema
        );
    }

    #[test]
    fn repair_policy_defaults_and_rejects_privilege_expansion() {
        let parsed = parse_workflow(valid_workflow(), WorkflowSource::Repository, &policy())
            .expect("default repair policy");
        assert!(!parsed.config.repair.enabled);
        assert_eq!(
            parsed.config.repair.max_diagnostic_bytes,
            crate::models::DEFAULT_REPAIR_MAX_DIAGNOSTIC_BYTES
        );

        let enabled = valid_workflow().replace(
            "quality_gates:\n",
            "repair:\n  enabled: true\n  max_repairs: 2\n  max_cost_units: 10\n  max_diagnostic_bytes: 4096\n  evidence_max_age_seconds: 600\n  claim_lease_seconds: 30\n  max_dispatch_attempts: 2\n  retry_base_delay_seconds: 2\n  retry_max_delay_seconds: 60\n  auto_categories:\n    - low_risk\n  approval_categories:\n    - logical_change\nquality_gates:\n",
        );
        let parsed_policy = parse_workflow(&enabled, WorkflowSource::Repository, &policy())
            .expect("bounded repair policy")
            .config
            .repair;
        let decoded: RepairPolicy = serde_json::from_value(
            serde_json::to_value(&parsed_policy).expect("encode repair policy"),
        )
        .expect("decode repair policy");
        assert_eq!(decoded, parsed_policy);

        let unsafe_auto = enabled.replace("- low_risk", "- dependency_change");
        assert_eq!(
            parse_workflow(&unsafe_auto, WorkflowSource::Repository, &policy())
                .expect_err("reject dependency auto repair")
                .kind(),
            WorkflowErrorKind::Policy
        );
    }

    #[test]
    fn rejects_missing_delimiter_unknown_field_and_invalid_enum() {
        let missing = valid_workflow().replacen("\n---\n", "\n", 1);
        assert_eq!(
            parse_workflow(&missing, WorkflowSource::Repository, &policy())
                .expect_err("missing delimiter")
                .kind(),
            WorkflowErrorKind::FrontMatter
        );
        let unknown = valid_workflow().replacen("version:", "unknown: true\nversion:", 1);
        assert_eq!(
            parse_workflow(&unknown, WorkflowSource::Repository, &policy())
                .expect_err("unknown field")
                .kind(),
            WorkflowErrorKind::Schema
        );
        let invalid = valid_workflow().replace("implementation", "unbounded");
        assert_eq!(
            parse_workflow(&invalid, WorkflowSource::Repository, &policy())
                .expect_err("invalid enum")
                .kind(),
            WorkflowErrorKind::Schema
        );
    }

    #[test]
    fn rejects_unknown_template_inputs_and_renders_allowed_values() {
        let unknown_variable =
            valid_workflow().replace("{{ task.title | trim }}", "{{ task.private_token | trim }}");
        assert_eq!(
            parse_workflow(&unknown_variable, WorkflowSource::Repository, &policy())
                .expect_err("unknown variable")
                .kind(),
            WorkflowErrorKind::Template
        );
        let unknown_filter = valid_workflow().replace("| trim", "| expose_secret");
        assert_eq!(
            parse_workflow(&unknown_filter, WorkflowSource::Repository, &policy())
                .expect_err("unknown filter")
                .kind(),
            WorkflowErrorKind::Template
        );
        let workflow = parse_workflow(valid_workflow(), WorkflowSource::Repository, &policy())
            .expect("valid workflow");
        let rendered = render_workflow(
            &workflow,
            &WorkflowTaskContext {
                task: WorkflowTaskTemplateContext {
                    id: 4,
                    title: "  严格任务  ",
                    goal: "完成实现",
                    background: None,
                    acceptance_criteria: None,
                    constraints: None,
                },
                repository: WorkflowRepositoryTemplateContext {
                    name: None,
                    default_branch: None,
                },
                environment: WorkflowEnvironmentTemplateContext {
                    name: "测试",
                    workspace_root: "/tmp/test",
                },
            },
        )
        .expect("render workflow");
        assert!(rendered.rendered_prompt.contains("严格任务"));
        assert!(rendered.rendered_prompt.contains("当前受控仓库"));
    }

    #[test]
    fn policy_is_fail_closed_and_diagnostics_do_not_echo_content() {
        let network = valid_workflow().replace("network: false", "network: true");
        let error = parse_workflow(&network, WorkflowSource::Repository, &policy())
            .expect_err("network expansion");
        assert_eq!(error.kind(), WorkflowErrorKind::Policy);
        let secret = valid_workflow().replace(
            "execution_mode: implementation",
            "execution_mode: implementation\npassword: top-secret-value",
        );
        let error = parse_workflow(&secret, WorkflowSource::Repository, &policy())
            .expect_err("unknown sensitive field");
        assert!(!error.to_string().contains("top-secret-value"));
    }

    #[test]
    fn rejects_oversized_document() {
        let oversized = format!("{}{}", valid_workflow(), "x".repeat(MAX_WORKFLOW_BYTES));
        assert_eq!(
            parse_workflow(&oversized, WorkflowSource::Repository, &policy())
                .expect_err("oversized")
                .kind(),
            WorkflowErrorKind::Size
        );
    }

    #[tokio::test]
    async fn loads_default_and_rejects_symlink() {
        let root = std::env::temp_dir().join(format!("devrail-workflow-{}", uuid::Uuid::new_v4()));
        let repository = root.join("repository");
        tokio::fs::create_dir_all(&repository)
            .await
            .expect("create repository");
        let loaded = load_repository_workflow(&root, &repository, &policy())
            .await
            .expect("default workflow");
        assert_eq!(loaded.source, WorkflowSource::Default);

        let outside = root.join("outside.md");
        tokio::fs::write(&outside, valid_workflow())
            .await
            .expect("write outside file");
        symlink(&outside, workflow_path(&repository)).expect("create symlink");
        let error = load_repository_workflow(&root, &repository, &policy())
            .await
            .expect_err("reject symlink");
        assert_eq!(error.kind(), WorkflowErrorKind::Path);
        tokio::fs::remove_dir_all(&root).await.expect("cleanup");
    }
}
