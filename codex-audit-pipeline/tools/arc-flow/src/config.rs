use anyhow::{bail, Context, Result};
use globset::{Glob, GlobSetBuilder};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::env;
use std::fs;
use std::path::{Component as PathComponent, Path, PathBuf};

pub const CONFIG_VERSION: u32 = 2;
pub const DEFAULT_CONFIG_PATH: &str = ".arc-flow/flow.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowConfig {
    pub version: u32,
    pub project: ProjectConfig,
    pub paths: PathsConfig,
    #[serde(default)]
    pub policy: PolicyConfig,
    #[serde(default)]
    pub doctor: DoctorConfig,
    #[serde(default)]
    pub services: BTreeMap<String, ServiceConfig>,
    #[serde(default)]
    pub parsers: BTreeMap<String, ParserConfig>,
    pub scope: ScopeConfig,
    pub steps: Vec<StepConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub name: String,
    pub default_profile: String,
    pub hook_profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathsConfig {
    pub reports: String,
    pub audit_config: String,
    #[serde(default = "default_secrets_config_path")]
    pub secrets_config: String,
    #[serde(default)]
    pub aliases: BTreeMap<String, PathAlias>,
}

fn default_secrets_config_path() -> String {
    ".arc-flow/secrets.toml".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathAlias {
    pub path: String,
    #[serde(default)]
    pub env: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    #[serde(default)]
    pub required_steps: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoctorConfig {
    #[serde(default)]
    pub checks: Vec<DoctorCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub id: String,
    pub label: String,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default)]
    pub help: Option<String>,
    #[serde(default = "default_doctor_timeout")]
    pub timeout_secs: u64,
    #[serde(flatten)]
    pub kind: DoctorCheckKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DoctorCheckKind {
    Command {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Path {
        path: String,
        #[serde(default)]
        path_type: PathType,
    },
    Glob {
        pattern: String,
    },
    Env {
        name: String,
    },
    EnvOrFile {
        env: String,
        path: String,
        contains: String,
    },
    GitConfig {
        key: String,
        expected: String,
    },
    GitRemotes,
    Version {
        program: String,
        #[serde(default)]
        args: Vec<String>,
        path: String,
        #[serde(default)]
        trim_prefix: String,
    },
    Service {
        service: String,
    },
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PathType {
    #[default]
    Any,
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ServiceConfig {
    Docker {
        image: String,
        #[serde(default)]
        image_env: Option<String>,
        #[serde(default)]
        external_env: Option<String>,
        inject_env: String,
        #[serde(default)]
        external_value_policy: ExternalValuePolicy,
        startup_timeout_secs: u64,
        #[serde(default)]
        timeout_env: Option<String>,
        container_port: u16,
        #[serde(default)]
        environment: BTreeMap<String, String>,
        healthcheck: Vec<String>,
        connection: String,
    },
    Environment {
        source_env: String,
        inject_env: String,
    },
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExternalValuePolicy {
    #[default]
    None,
    IsolatedPostgres,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ParserConfig {
    Regex {
        patterns: Vec<String>,
        #[serde(default = "default_capture")]
        capture: usize,
        #[serde(default = "default_minimum")]
        minimum: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeConfig {
    #[serde(default)]
    pub unmatched: UnmatchedScope,
    pub rules: Vec<ScopeRule>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnmatchedScope {
    #[default]
    Fail,
    All,
    Ignore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeRule {
    pub patterns: Vec<String>,
    pub components: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepConfig {
    pub id: String,
    pub label: String,
    pub component: String,
    pub profiles: BTreeSet<String>,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub log: String,
    pub timeout_secs: u64,
    #[serde(default)]
    pub timeout_env: Option<String>,
    #[serde(default)]
    pub parser: Option<String>,
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub remove_env: Vec<String>,
}

impl FlowConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let source = fs::read_to_string(path)
            .with_context(|| format!("read workflow config {}", path.display()))?;
        let mut config: Self = toml::from_str(&source)
            .with_context(|| format!("parse workflow config {}", path.display()))?;
        config.apply_environment()?;
        config.validate()?;
        Ok(config)
    }

    pub fn from_source(source: &str) -> Result<Self> {
        let config: Self = toml::from_str(source).context("parse workflow config")?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != CONFIG_VERSION {
            bail!(
                "unsupported workflow config version {}; expected {}; run `arc-flow config migrate` for v1 configurations",
                self.version,
                CONFIG_VERSION
            );
        }
        validate_id("project.name", &self.project.name)?;
        validate_id("project.default_profile", &self.project.default_profile)?;
        validate_id("project.hook_profile", &self.project.hook_profile)?;
        validate_repo_path("paths.reports", &self.paths.reports)?;
        validate_repo_path("paths.audit_config", &self.paths.audit_config)?;
        validate_repo_path("paths.secrets_config", &self.paths.secrets_config)?;

        for (alias, entry) in &self.paths.aliases {
            validate_id("path alias", alias)?;
            if matches!(
                alias.as_str(),
                "root" | "reports" | "audit_config" | "secrets_config" | "host_port"
            ) {
                bail!("path alias {alias:?} is reserved");
            }
            validate_repo_path(&format!("paths.aliases.{alias}"), &entry.path)?;
            if let Some(name) = &entry.env {
                validate_env_name(&format!("paths.aliases.{alias}.env"), name)?;
            }
        }

        self.validate_services()?;
        self.validate_parsers()?;
        self.validate_doctor()?;
        self.validate_scope()?;
        self.validate_steps()?;
        Ok(())
    }

    pub fn classify_paths(&self, paths: &[String]) -> Result<(BTreeSet<String>, Vec<String>)> {
        let mut components = BTreeSet::new();
        let mut unmatched = Vec::new();
        for path in paths {
            let mut matched = false;
            for rule in &self.scope.rules {
                let mut builder = GlobSetBuilder::new();
                for pattern in &rule.patterns {
                    builder.add(Glob::new(pattern)?);
                }
                let matcher = builder.build()?;
                if matcher.is_match(path) {
                    matched = true;
                    components.extend(rule.components.iter().cloned());
                }
            }
            if !matched {
                unmatched.push(path.clone());
            }
        }
        Ok((components, unmatched))
    }

    pub fn components(&self) -> BTreeSet<String> {
        self.steps
            .iter()
            .map(|step| step.component.clone())
            .collect()
    }

    pub fn step(&self, id: &str) -> Option<&StepConfig> {
        self.steps.iter().find(|step| step.id == id)
    }

    pub fn parser(&self, id: &str) -> Option<&ParserConfig> {
        self.parsers.get(id)
    }

    pub fn service(&self, id: &str) -> Option<&ServiceConfig> {
        self.services.get(id)
    }

    pub fn allowed_placeholder(&self, name: &str) -> bool {
        matches!(name, "root" | "reports" | "audit_config" | "secrets_config")
            || self.paths.aliases.contains_key(name)
    }

    fn apply_environment(&mut self) -> Result<()> {
        override_string("REPORT_DIR", &mut self.paths.reports);
        override_string("ARC_FLOW_REPORTS", &mut self.paths.reports);
        override_string("AUDITOR_CONFIG", &mut self.paths.audit_config);
        override_string("ARC_FLOW_AUDIT_CONFIG", &mut self.paths.audit_config);
        override_string("ARC_FLOW_SECRETS_CONFIG", &mut self.paths.secrets_config);

        for entry in self.paths.aliases.values_mut() {
            if let Some(name) = &entry.env {
                override_string(name, &mut entry.path);
            }
        }
        for service in self.services.values_mut() {
            if let ServiceConfig::Docker {
                image,
                image_env,
                startup_timeout_secs,
                timeout_env,
                ..
            } = service
            {
                if let Some(name) = image_env {
                    override_string(name, image);
                }
                if let Some(name) = timeout_env {
                    override_u64(name, startup_timeout_secs)?;
                }
            }
        }
        for step in &mut self.steps {
            if let Some(name) = &step.timeout_env {
                override_u64(name, &mut step.timeout_secs)?;
            }
        }
        Ok(())
    }

    fn validate_services(&self) -> Result<()> {
        for (id, service) in &self.services {
            validate_id("service id", id)?;
            match service {
                ServiceConfig::Environment {
                    source_env,
                    inject_env,
                } => {
                    validate_env_name("service.source_env", source_env)?;
                    validate_env_name("service.inject_env", inject_env)?;
                }
                ServiceConfig::Docker {
                    image,
                    image_env,
                    external_env,
                    inject_env,
                    external_value_policy,
                    startup_timeout_secs,
                    timeout_env,
                    container_port,
                    environment,
                    healthcheck,
                    connection,
                } => {
                    validate_image(image)?;
                    if *startup_timeout_secs == 0 || *startup_timeout_secs > 300 {
                        bail!("service {id:?} startup_timeout_secs must be between 1 and 300");
                    }
                    if *container_port == 0 {
                        bail!("service {id:?} container_port must not be zero");
                    }
                    if let Some(name) = image_env {
                        validate_env_name("service.image_env", name)?;
                    }
                    if let Some(name) = timeout_env {
                        validate_env_name("service.timeout_env", name)?;
                    }
                    if let Some(name) = external_env {
                        validate_env_name("service.external_env", name)?;
                    }
                    if *external_value_policy != ExternalValuePolicy::None && external_env.is_none()
                    {
                        bail!("Docker service {id:?} external_value_policy requires external_env");
                    }
                    validate_env_name("service.inject_env", inject_env)?;
                    if healthcheck.is_empty() {
                        bail!("Docker service {id:?} requires a healthcheck command");
                    }
                    if !connection.contains("{host_port}") {
                        bail!("Docker service {id:?} connection must contain {{host_port}}");
                    }
                    for key in environment.keys() {
                        validate_env_name("service.environment key", key)?;
                    }
                    for value in environment.values().chain(healthcheck.iter()) {
                        if value.contains('\0') {
                            bail!("Docker service {id:?} contains a NUL value");
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_parsers(&self) -> Result<()> {
        for (id, parser) in &self.parsers {
            validate_id("parser id", id)?;
            match parser {
                ParserConfig::Regex {
                    patterns,
                    capture,
                    minimum,
                } => {
                    if patterns.is_empty() || *minimum == 0 {
                        bail!("parser {id:?} requires patterns and minimum greater than zero");
                    }
                    for pattern in patterns {
                        let regex = Regex::new(pattern)
                            .with_context(|| format!("parser {id:?} has invalid regex"))?;
                        if *capture >= regex.captures_len() {
                            bail!("parser {id:?} regex has no capture group {capture}");
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_doctor(&self) -> Result<()> {
        let mut ids = HashSet::new();
        for check in &self.doctor.checks {
            validate_id("doctor check id", &check.id)?;
            if !ids.insert(check.id.as_str()) {
                bail!("duplicate doctor check id {:?}", check.id);
            }
            if check.label.trim().is_empty() {
                bail!("doctor check {:?} requires a label", check.id);
            }
            if check.timeout_secs == 0 || check.timeout_secs > 300 {
                bail!(
                    "doctor check {:?} timeout_secs must be between 1 and 300",
                    check.id
                );
            }
            match &check.kind {
                DoctorCheckKind::Command { program, args } => {
                    validate_program("doctor command", program)?;
                    validate_arguments(self, &check.id, args)?;
                }
                DoctorCheckKind::Path { path, .. } | DoctorCheckKind::Glob { pattern: path } => {
                    validate_template(self, &check.id, path)?;
                }
                DoctorCheckKind::Env { name } => validate_env_name("doctor env", name)?,
                DoctorCheckKind::EnvOrFile {
                    env,
                    path,
                    contains,
                } => {
                    validate_env_name("doctor env", env)?;
                    validate_template(self, &check.id, path)?;
                    if contains.is_empty() {
                        bail!("doctor check {:?} requires non-empty contains", check.id);
                    }
                }
                DoctorCheckKind::GitConfig { key, expected } => {
                    if key.is_empty() || expected.is_empty() {
                        bail!(
                            "doctor Git config check {:?} requires key and expected",
                            check.id
                        );
                    }
                }
                DoctorCheckKind::GitRemotes => {}
                DoctorCheckKind::Version {
                    program,
                    args,
                    path,
                    ..
                } => {
                    validate_program("doctor version program", program)?;
                    validate_arguments(self, &check.id, args)?;
                    validate_template(self, &check.id, path)?;
                }
                DoctorCheckKind::Service { service } => {
                    if !self.services.contains_key(service) {
                        bail!(
                            "doctor check {:?} references unknown service {service:?}",
                            check.id
                        );
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_scope(&self) -> Result<()> {
        if self.scope.rules.is_empty() {
            bail!("scope.rules must not be empty");
        }
        let components = self.components();
        for (index, rule) in self.scope.rules.iter().enumerate() {
            if rule.patterns.is_empty() || rule.components.is_empty() {
                bail!("scope.rules[{index}] requires patterns and components");
            }
            for component in &rule.components {
                validate_id("scope component", component)?;
                if !components.contains(component) {
                    bail!("scope.rules[{index}] references component {component:?} with no steps");
                }
            }
            for pattern in &rule.patterns {
                if pattern.contains("..") || Path::new(pattern).is_absolute() {
                    bail!("scope.rules[{index}] contains unsafe pattern {pattern:?}");
                }
                Glob::new(pattern).with_context(|| {
                    format!("scope.rules[{index}] contains invalid pattern {pattern:?}")
                })?;
            }
        }
        Ok(())
    }

    fn validate_steps(&self) -> Result<()> {
        if self.steps.is_empty() {
            bail!("steps must not be empty");
        }
        let mut ids = HashSet::new();
        let mut profiles = BTreeSet::new();
        for step in &self.steps {
            if !ids.insert(step.id.as_str()) {
                bail!("duplicate verification step id {:?}", step.id);
            }
            validate_step(self, step)?;
            profiles.extend(step.profiles.iter().cloned());
        }
        for profile in [&self.project.default_profile, &self.project.hook_profile] {
            if !profiles.contains(profile) {
                bail!("configured profile {profile:?} is not used by any step");
            }
        }
        let mut required = HashSet::new();
        for id in &self.policy.required_steps {
            if !required.insert(id.as_str()) {
                bail!("duplicate policy.required_steps entry {id:?}");
            }
            if !ids.contains(id.as_str()) {
                bail!("policy requires missing verification step {id:?}");
            }
        }
        Ok(())
    }
}

fn validate_step(config: &FlowConfig, step: &StepConfig) -> Result<()> {
    validate_id("verification step id", &step.id)?;
    validate_id("step component", &step.component)?;
    if step.label.trim().is_empty() || step.profiles.is_empty() {
        bail!(
            "step {:?} requires a label and at least one profile",
            step.id
        );
    }
    for profile in &step.profiles {
        validate_id("step profile", profile)?;
    }
    validate_program(&format!("step {} program", step.id), &step.program)?;
    if is_shell(&step.program)
        && step
            .args
            .iter()
            .any(|argument| is_shell_command_argument(argument))
    {
        bail!("step {:?} may not execute a shell command string", step.id);
    }
    validate_arguments(config, &step.id, &step.args)?;
    let Some(cwd_name) = exact_placeholder(&step.cwd) else {
        bail!("step {:?} cwd must be one path placeholder", step.id);
    };
    if cwd_name != "root" && !config.paths.aliases.contains_key(cwd_name) {
        bail!(
            "step {:?} cwd references unknown path {cwd_name:?}",
            step.id
        );
    }
    let log = Path::new(&step.log);
    if log.components().count() != 1 || log.extension().is_none_or(|value| value != "log") {
        bail!("step {:?} log must be a single .log file name", step.id);
    }
    if step.timeout_secs == 0 || step.timeout_secs > 3600 {
        bail!("step {:?} timeout_secs must be between 1 and 3600", step.id);
    }
    if let Some(name) = &step.timeout_env {
        validate_env_name("step timeout_env", name)?;
    }
    if let Some(parser) = &step.parser {
        if !config.parsers.contains_key(parser) {
            bail!("step {:?} references unknown parser {parser:?}", step.id);
        }
    }
    let mut step_services = HashSet::new();
    let mut service_envs = HashSet::new();
    for service in &step.services {
        if !step_services.insert(service) {
            bail!("step {:?} contains duplicate service {service:?}", step.id);
        }
        let service_config = config.services.get(service).ok_or_else(|| {
            anyhow::anyhow!("step {:?} references unknown service {service:?}", step.id)
        })?;
        let inject_env = match service_config {
            ServiceConfig::Docker { inject_env, .. }
            | ServiceConfig::Environment { inject_env, .. } => inject_env,
        };
        if !service_envs.insert(inject_env) {
            bail!(
                "step {:?} has multiple services injecting {inject_env}",
                step.id
            );
        }
        if step.remove_env.contains(inject_env) {
            bail!(
                "step {:?} may not remove service injection variable {inject_env}",
                step.id
            );
        }
    }
    for name in &step.remove_env {
        validate_env_name("step remove_env", name)?;
    }
    Ok(())
}

fn validate_arguments(config: &FlowConfig, owner: &str, args: &[String]) -> Result<()> {
    for arg in args {
        validate_template(config, owner, arg)?;
    }
    Ok(())
}

fn validate_template(config: &FlowConfig, owner: &str, value: &str) -> Result<()> {
    if value.contains('\0') {
        bail!("{owner:?} contains a NUL value");
    }
    let mut rest = value;
    while let Some(start) = rest.find('{') {
        let tail = &rest[start..];
        let Some(end) = tail.find('}') else {
            bail!("{owner:?} contains an unterminated placeholder in {value:?}");
        };
        let name = &tail[1..end];
        if !config.allowed_placeholder(name) {
            bail!("{owner:?} contains unsupported placeholder {{{name}}}");
        }
        rest = &tail[end + 1..];
    }
    Ok(())
}

fn exact_placeholder(value: &str) -> Option<&str> {
    value.strip_prefix('{')?.strip_suffix('}')
}

fn is_shell(program: &str) -> bool {
    matches!(program, "sh" | "bash" | "dash" | "zsh")
}

fn is_shell_command_argument(arg: &str) -> bool {
    arg.starts_with("--command")
        || (arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains('c'))
}

fn validate_id(name: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '-' | '_')
        })
    {
        bail!("{name} must be a lowercase identifier, found {value:?}");
    }
    Ok(())
}

fn validate_program(name: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.contains('/')
        || value.contains('\\')
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '+')
        })
    {
        bail!("{name} must be a bare executable name, found {value:?}");
    }
    Ok(())
}

fn validate_env_name(name: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
    {
        bail!("{name} must be an uppercase environment variable name, found {value:?}");
    }
    Ok(())
}

fn validate_image(value: &str) -> Result<()> {
    if value.is_empty()
        || value.starts_with('-')
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '.' | '_' | '/' | ':' | '@' | '-')
        })
    {
        bail!("Docker image must be an OCI image reference, found {value:?}");
    }
    Ok(())
}

fn validate_repo_path(name: &str, value: &str) -> Result<()> {
    let path = Path::new(value);
    if value.is_empty() || path.is_absolute() {
        bail!("{name} must be a non-empty repository-relative path");
    }
    if path.components().any(|component| {
        matches!(
            component,
            PathComponent::ParentDir | PathComponent::RootDir | PathComponent::Prefix(_)
        )
    }) {
        bail!("{name} may not escape the repository: {value:?}");
    }
    Ok(())
}

fn override_string(name: &str, target: &mut String) {
    if let Ok(value) = env::var(name) {
        *target = value;
    }
}

fn override_u64(name: &str, target: &mut u64) -> Result<()> {
    if let Ok(value) = env::var(name) {
        *target = value
            .parse()
            .with_context(|| format!("environment variable {name} must be an integer"))?;
    }
    Ok(())
}

const fn default_true() -> bool {
    true
}

const fn default_doctor_timeout() -> u64 {
    15
}

const fn default_capture() -> usize {
    1
}

const fn default_minimum() -> usize {
    1
}

pub fn resolve_config_path(root: &Path, override_path: Option<PathBuf>) -> Result<PathBuf> {
    let path = override_path
        .or_else(|| env::var_os("ARC_FLOW_CONFIG").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    let path = path
        .canonicalize()
        .with_context(|| format!("resolve workflow config {}", path.display()))?;
    if !path.starts_with(root) {
        bail!(
            "workflow config must be inside the repository: {}",
            path.display()
        );
    }
    Ok(path)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyConfig {
    version: u32,
    paths: LegacyPaths,
    doctor: LegacyDoctor,
    database: LegacyDatabase,
    scope: ScopeConfig,
    steps: Vec<LegacyStep>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyPaths {
    backend: String,
    frontend: String,
    reports: String,
    tool_manifest: String,
    audit_config: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyDoctor {
    required_commands: Vec<String>,
    node_version_file: String,
    hooks_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyDatabase {
    image: String,
    startup_timeout_secs: u64,
    container_port: u16,
    user: String,
    password: String,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyStep {
    id: String,
    label: String,
    component: String,
    profiles: BTreeSet<String>,
    program: String,
    args: Vec<String>,
    cwd: String,
    log: String,
    timeout_secs: u64,
    #[serde(default)]
    timeout_env: Option<String>,
    #[serde(default)]
    parser: Option<String>,
    #[serde(default)]
    requires_test_database: bool,
}

pub fn migrate_v1(source: &str, project_name: &str) -> Result<FlowConfig> {
    let legacy: LegacyConfig = toml::from_str(source).context("parse v1 workflow config")?;
    if legacy.version != 1 {
        bail!("configuration is version {}, not version 1", legacy.version);
    }

    let mut aliases = BTreeMap::new();
    aliases.insert(
        "backend".into(),
        PathAlias {
            path: legacy.paths.backend,
            env: Some("ARC_FLOW_BACKEND".into()),
        },
    );
    aliases.insert(
        "frontend".into(),
        PathAlias {
            path: legacy.paths.frontend,
            env: Some("ARC_FLOW_FRONTEND".into()),
        },
    );
    aliases.insert(
        "tool_manifest".into(),
        PathAlias {
            path: legacy.paths.tool_manifest,
            env: Some("ARC_FLOW_TOOL_MANIFEST".into()),
        },
    );

    let service_id = "test-postgres".to_string();
    let mut services = BTreeMap::new();
    services.insert(
        service_id.clone(),
        ServiceConfig::Docker {
            image: legacy.database.image,
            image_env: Some("ARC_FLOW_POSTGRES_IMAGE".into()),
            external_env: Some("TEST_DATABASE_URL".into()),
            inject_env: "TEST_DATABASE_URL".into(),
            external_value_policy: ExternalValuePolicy::IsolatedPostgres,
            startup_timeout_secs: legacy.database.startup_timeout_secs,
            timeout_env: Some("ARC_FLOW_DATABASE_TIMEOUT_SECS".into()),
            container_port: legacy.database.container_port,
            environment: BTreeMap::from([
                ("POSTGRES_USER".into(), legacy.database.user.clone()),
                ("POSTGRES_PASSWORD".into(), legacy.database.password.clone()),
                ("POSTGRES_DB".into(), legacy.database.name.clone()),
            ]),
            healthcheck: vec![
                "pg_isready".into(),
                "-U".into(),
                legacy.database.user.clone(),
                "-d".into(),
                legacy.database.name.clone(),
            ],
            connection: format!(
                "postgres://{}:{}@127.0.0.1:{{host_port}}/{}",
                legacy.database.user, legacy.database.password, legacy.database.name
            ),
        },
    );

    let mut parsers = BTreeMap::new();
    parsers.insert(
        "rust".into(),
        ParserConfig::Regex {
            patterns: vec![r"(?m)^running ([0-9]+) tests?$".into()],
            capture: 1,
            minimum: 1,
        },
    );
    parsers.insert(
        "angular".into(),
        ParserConfig::Regex {
            patterns: vec![r"Tests\s+([0-9]+) passed".into()],
            capture: 1,
            minimum: 1,
        },
    );

    let steps = legacy
        .steps
        .into_iter()
        .map(|step| StepConfig {
            id: step.id,
            label: step.label,
            component: step.component,
            profiles: step.profiles,
            program: step.program,
            args: step.args,
            cwd: step.cwd,
            log: step.log,
            timeout_secs: step.timeout_secs,
            timeout_env: step.timeout_env,
            parser: step.parser,
            services: step
                .requires_test_database
                .then(|| service_id.clone())
                .into_iter()
                .collect(),
            remove_env: step
                .requires_test_database
                .then(|| "DATABASE_URL".to_string())
                .into_iter()
                .collect(),
        })
        .collect::<Vec<_>>();
    let required_steps = steps.iter().map(|step| step.id.clone()).collect();

    let mut checks = legacy
        .doctor
        .required_commands
        .into_iter()
        .map(|program| DoctorCheck {
            id: format!("tool.{program}"),
            label: program.clone(),
            required: true,
            help: None,
            timeout_secs: default_doctor_timeout(),
            kind: DoctorCheckKind::Command {
                program,
                args: vec!["--version".into()],
            },
        })
        .collect::<Vec<_>>();
    checks.extend([
        DoctorCheck {
            id: "frontend.dependencies".into(),
            label: "frontend dependencies".into(),
            required: true,
            help: Some("run `cd frontend && npm ci`".into()),
            timeout_secs: default_doctor_timeout(),
            kind: DoctorCheckKind::Path {
                path: "{frontend}/node_modules".into(),
                path_type: PathType::Directory,
            },
        },
        DoctorCheck {
            id: "runtime.database".into(),
            label: "runtime database".into(),
            required: true,
            help: Some("create backend/.env from backend/.env.example".into()),
            timeout_secs: default_doctor_timeout(),
            kind: DoctorCheckKind::EnvOrFile {
                env: "DATABASE_URL".into(),
                path: "{backend}/.env".into(),
                contains: "DATABASE_URL=".into(),
            },
        },
        DoctorCheck {
            id: "backend.migrations".into(),
            label: "migrations".into(),
            required: true,
            help: Some("add at least one SQL migration".into()),
            timeout_secs: default_doctor_timeout(),
            kind: DoctorCheckKind::Glob {
                pattern: "{backend}/migrations/*.sql".into(),
            },
        },
        DoctorCheck {
            id: "git.hooks".into(),
            label: "Git hooks".into(),
            required: false,
            help: Some(format!(
                "run `git config core.hooksPath {}`",
                legacy.doctor.hooks_path
            )),
            timeout_secs: default_doctor_timeout(),
            kind: DoctorCheckKind::GitConfig {
                key: "core.hooksPath".into(),
                expected: legacy.doctor.hooks_path,
            },
        },
        DoctorCheck {
            id: "node.version".into(),
            label: "Node version".into(),
            required: true,
            help: None,
            timeout_secs: default_doctor_timeout(),
            kind: DoctorCheckKind::Version {
                program: "node".into(),
                args: vec!["--version".into()],
                path: format!("{{root}}/{}", legacy.doctor.node_version_file),
                trim_prefix: "v".into(),
            },
        },
        DoctorCheck {
            id: "git.remotes".into(),
            label: "Git remotes".into(),
            required: true,
            help: None,
            timeout_secs: default_doctor_timeout(),
            kind: DoctorCheckKind::GitRemotes,
        },
        DoctorCheck {
            id: "test.database".into(),
            label: "test database".into(),
            required: false,
            help: Some("configure TEST_DATABASE_URL or Docker".into()),
            timeout_secs: default_doctor_timeout(),
            kind: DoctorCheckKind::Service {
                service: service_id,
            },
        },
    ]);

    let config = FlowConfig {
        version: CONFIG_VERSION,
        project: ProjectConfig {
            name: project_name.to_string(),
            default_profile: "full".into(),
            hook_profile: "hook".into(),
        },
        paths: PathsConfig {
            reports: legacy.paths.reports,
            audit_config: legacy.paths.audit_config,
            secrets_config: ".arc-flow/secrets.toml".into(),
            aliases,
        },
        policy: PolicyConfig { required_steps },
        doctor: DoctorConfig { checks },
        services,
        parsers,
        scope: legacy.scope,
        steps,
    };
    config.validate()?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository_config() -> FlowConfig {
        toml::from_str(include_str!("../../../../.arc-flow/flow.toml")).expect("parse fixture")
    }

    #[test]
    fn repository_configuration_is_valid() {
        repository_config().validate().expect("validate config");
    }

    #[test]
    fn repository_api_generation_timeout_covers_cold_runner() {
        let config = repository_config();
        let step = config
            .steps
            .iter()
            .find(|step| step.id == "workflow.api-generation")
            .expect("API generation step");

        assert!(step.timeout_secs >= 600);
    }

    #[test]
    fn existing_v2_config_defaults_the_secret_rule_path() {
        let source = include_str!("../../../../.arc-flow/flow.toml")
            .lines()
            .filter(|line| !line.starts_with("secrets_config = "))
            .collect::<Vec<_>>()
            .join("\n");
        let config = FlowConfig::from_source(&source).expect("compatible v2 config");

        assert_eq!(config.paths.secrets_config, ".arc-flow/secrets.toml");
    }

    #[test]
    fn components_and_profiles_are_not_hard_coded() {
        let mut config = repository_config();
        config.steps[0].component = "mobile".into();
        config.steps[0].profiles.insert("ci".into());
        config.project.default_profile = "ci".into();
        config.scope.rules[0].components = BTreeSet::from(["mobile".into()]);
        config.validate().expect("custom component is valid");
    }

    #[test]
    fn policy_steps_cannot_be_missing() {
        let mut config = repository_config();
        config.steps.retain(|step| step.id != "backend.tests");
        let error = config.validate().expect_err("missing step must fail");
        assert!(error.to_string().contains("backend.tests"));
    }

    #[test]
    fn shell_command_strings_are_rejected() {
        let mut config = repository_config();
        let step = config.steps.first_mut().expect("step");
        step.program = "sh".into();
        step.args = vec!["-lc".into(), "cargo fmt".into()];
        let error = config.validate().expect_err("shell command must fail");
        assert!(error.to_string().contains("shell command string"));
    }

    #[test]
    fn duplicate_service_injection_variables_are_rejected() {
        let mut config = repository_config();
        config.services.insert(
            "test-cache".into(),
            ServiceConfig::Environment {
                source_env: "CACHE_URL".into(),
                inject_env: "TEST_DATABASE_URL".into(),
            },
        );
        let step = config
            .steps
            .iter_mut()
            .find(|step| step.id == "backend.tests")
            .expect("backend test step");
        step.services.push("test-cache".into());

        let error = config.validate().expect_err("injection must be unique");
        assert!(error.to_string().contains("multiple services injecting"));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let source = include_str!("../../../../.arc-flow/flow.toml").replacen(
            "version = 2",
            "version = 2\nunknown = true",
            1,
        );
        assert!(toml::from_str::<FlowConfig>(&source).is_err());
    }

    #[test]
    fn version_one_configuration_can_be_migrated() {
        let source = r#"
version = 1

[paths]
backend = "backend"
frontend = "frontend"
reports = "reports"
tool_manifest = "tools/arc-flow/Cargo.toml"
audit_config = "audit.toml"

[doctor]
required_commands = ["git"]
node_version_file = ".node-version"
hooks_path = "hooks"

[database]
image = "postgres:16-alpine"
startup_timeout_secs = 30
container_port = 5432
user = "test"
password = "test"
name = "test"

[[scope.rules]]
patterns = ["src/**"]
components = ["app"]

[[steps]]
id = "app.check"
label = "app check"
component = "app"
profiles = ["full", "hook"]
program = "git"
args = ["diff", "--check"]
cwd = "{root}"
log = "app_check.log"
timeout_secs = 60
"#;
        let migrated = migrate_v1(source, "example").expect("migrate");
        assert_eq!(migrated.version, 2);
        assert_eq!(migrated.project.name, "example");
        assert_eq!(migrated.paths.secrets_config, ".arc-flow/secrets.toml");
        assert!(migrated.policy.required_steps.contains(&"app.check".into()));
    }
}
