use crate::project::Project;
use anyhow::{bail, Context, Result};
use regex::bytes::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Duration;
use url::Url;

const SECRET_CONFIG_VERSION: u32 = 2;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecretConfig {
    version: u32,
    placeholders: PlaceholderConfig,
    rules: Vec<SecretRule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlaceholderConfig {
    minimum_unique_characters: usize,
    maximum_nonalphanumeric_characters: usize,
    markers: Vec<String>,
    exact: Vec<String>,
    prefixes: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalTestDatabasePolicy {
    hosts: Vec<String>,
    database_suffixes: Vec<String>,
    require_username_equals_password: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum SecretRule {
    Direct {
        id: String,
        pattern: String,
    },
    Value {
        id: String,
        pattern: String,
        capture: usize,
        minimum_length: usize,
    },
    PostgresUrl {
        id: String,
        pattern: String,
        username_capture: usize,
        password_capture: usize,
        host_capture: usize,
        database_capture: usize,
        minimum_length: usize,
        #[serde(default)]
        local_test_policy: Option<LocalTestDatabasePolicy>,
    },
    WebhookUrl {
        id: String,
        pattern: String,
        capture: usize,
        query_parameters: Vec<String>,
        query_minimum_length: usize,
        path_minimum_length: usize,
    },
}

impl SecretRule {
    fn id(&self) -> &str {
        match self {
            Self::Direct { id, .. }
            | Self::Value { id, .. }
            | Self::PostgresUrl { id, .. }
            | Self::WebhookUrl { id, .. } => id,
        }
    }

    fn pattern(&self) -> &str {
        match self {
            Self::Direct { pattern, .. }
            | Self::Value { pattern, .. }
            | Self::PostgresUrl { pattern, .. }
            | Self::WebhookUrl { pattern, .. } => pattern,
        }
    }
}

enum CompiledRule {
    Direct {
        pattern: Regex,
    },
    Value {
        pattern: Regex,
        capture: usize,
        minimum_length: usize,
    },
    PostgresUrl {
        pattern: Regex,
        username_capture: usize,
        password_capture: usize,
        host_capture: usize,
        database_capture: usize,
        minimum_length: usize,
        local_test_policy: Option<CompiledLocalTestDatabasePolicy>,
    },
    WebhookUrl {
        pattern: Regex,
        capture: usize,
        query_parameters: HashSet<String>,
        query_minimum_length: usize,
        path_minimum_length: usize,
    },
}

struct CompiledLocalTestDatabasePolicy {
    hosts: HashSet<String>,
    database_suffixes: Vec<String>,
    require_username_equals_password: bool,
}

struct SecretScanner {
    placeholders: PlaceholderConfig,
    rules: Vec<CompiledRule>,
}

impl SecretScanner {
    fn load(path: &Path) -> Result<Self> {
        let source = fs::read_to_string(path)
            .with_context(|| format!("read secret scan configuration {}", path.display()))?;
        Self::from_source(&source)
            .with_context(|| format!("parse secret scan configuration {}", path.display()))
    }

    fn from_source(source: &str) -> Result<Self> {
        let config: SecretConfig =
            toml::from_str(source).context("parse secret scan configuration")?;
        if config.version != SECRET_CONFIG_VERSION {
            bail!(
                "unsupported secret scan config version {}; expected {}",
                config.version,
                SECRET_CONFIG_VERSION
            );
        }
        if config.rules.is_empty() {
            bail!("secret scan configuration requires at least one rule");
        }
        if config.placeholders.minimum_unique_characters == 0
            || config
                .placeholders
                .markers
                .iter()
                .chain(&config.placeholders.exact)
                .chain(&config.placeholders.prefixes)
                .any(|value| value.trim().is_empty())
        {
            bail!("secret scan placeholder policy contains an invalid value");
        }

        let mut ids = HashSet::new();
        let mut rules = Vec::with_capacity(config.rules.len());
        for rule in config.rules {
            let id = rule.id().to_string();
            if id.is_empty()
                || !id
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character))
                || !ids.insert(id.clone())
            {
                bail!("secret scan rule IDs must be non-empty, portable, and unique: {id:?}");
            }
            let pattern = Regex::new(rule.pattern())
                .with_context(|| format!("secret scan rule {id:?} has an invalid regex"))?;
            let capture_count = pattern.captures_len();
            let compiled = match rule {
                SecretRule::Direct { .. } => CompiledRule::Direct { pattern },
                SecretRule::Value {
                    capture,
                    minimum_length,
                    ..
                } => {
                    validate_capture(&id, capture, capture_count)?;
                    validate_minimum(&id, minimum_length)?;
                    CompiledRule::Value {
                        pattern,
                        capture,
                        minimum_length,
                    }
                }
                SecretRule::PostgresUrl {
                    username_capture,
                    password_capture,
                    host_capture,
                    database_capture,
                    minimum_length,
                    local_test_policy,
                    ..
                } => {
                    let captures = [
                        username_capture,
                        password_capture,
                        host_capture,
                        database_capture,
                    ];
                    for capture in captures {
                        validate_capture(&id, capture, capture_count)?;
                    }
                    if captures.into_iter().collect::<HashSet<_>>().len() != captures.len() {
                        bail!("secret scan rule {id:?} requires distinct PostgreSQL captures");
                    }
                    validate_minimum(&id, minimum_length)?;
                    let local_test_policy = local_test_policy
                        .map(|policy| compile_local_test_policy(&id, policy))
                        .transpose()?;
                    CompiledRule::PostgresUrl {
                        pattern,
                        username_capture,
                        password_capture,
                        host_capture,
                        database_capture,
                        minimum_length,
                        local_test_policy,
                    }
                }
                SecretRule::WebhookUrl {
                    capture,
                    query_parameters,
                    query_minimum_length,
                    path_minimum_length,
                    ..
                } => {
                    validate_capture(&id, capture, capture_count)?;
                    validate_minimum(&id, query_minimum_length)?;
                    validate_minimum(&id, path_minimum_length)?;
                    if query_parameters.is_empty()
                        || query_parameters.iter().any(|value| value.trim().is_empty())
                    {
                        bail!("secret scan rule {id:?} requires query parameters");
                    }
                    CompiledRule::WebhookUrl {
                        pattern,
                        capture,
                        query_parameters: query_parameters
                            .into_iter()
                            .map(|value| value.to_ascii_lowercase())
                            .collect(),
                        query_minimum_length,
                        path_minimum_length,
                    }
                }
            };
            rules.push(compiled);
        }

        Ok(Self {
            placeholders: config.placeholders,
            rules,
        })
    }

    fn is_match(&self, bytes: &[u8]) -> bool {
        self.rules
            .iter()
            .any(|rule| rule.is_match(bytes, &self.placeholders))
    }
}

impl CompiledRule {
    fn is_match(&self, bytes: &[u8], placeholders: &PlaceholderConfig) -> bool {
        match self {
            Self::Direct { pattern } => pattern.is_match(bytes),
            Self::Value {
                pattern,
                capture,
                minimum_length,
            } => pattern.captures_iter(bytes).any(|captures| {
                captures.get(*capture).is_some_and(|value| {
                    looks_like_secret(value.as_bytes(), *minimum_length, placeholders)
                })
            }),
            Self::PostgresUrl {
                pattern,
                username_capture,
                password_capture,
                host_capture,
                database_capture,
                minimum_length,
                local_test_policy,
            } => pattern.captures_iter(bytes).any(|captures| {
                let values = [
                    *username_capture,
                    *password_capture,
                    *host_capture,
                    *database_capture,
                ]
                .map(|index| captures.get(index).map(|value| value.as_bytes()));
                let [Some(username), Some(password), Some(host), Some(database)] = values else {
                    return false;
                };
                if local_test_policy.as_ref().is_some_and(|policy| {
                    is_local_test_database(username, password, host, database, policy)
                }) {
                    return false;
                }
                looks_like_secret(password, *minimum_length, placeholders)
            }),
            Self::WebhookUrl {
                pattern,
                capture,
                query_parameters,
                query_minimum_length,
                path_minimum_length,
            } => pattern.captures_iter(bytes).any(|captures| {
                captures.get(*capture).is_some_and(|value| {
                    webhook_url_contains_secret(
                        value.as_bytes(),
                        query_parameters,
                        *query_minimum_length,
                        *path_minimum_length,
                        placeholders,
                    )
                })
            }),
        }
    }
}

fn validate_capture(id: &str, capture: usize, capture_count: usize) -> Result<()> {
    if capture == 0 || capture >= capture_count {
        bail!(
            "secret scan rule {id:?} references capture {capture}, but its regex has {} capture group(s)",
            capture_count.saturating_sub(1)
        );
    }
    Ok(())
}

fn validate_minimum(id: &str, minimum: usize) -> Result<()> {
    if minimum == 0 {
        bail!("secret scan rule {id:?} minimum length must be positive");
    }
    Ok(())
}

fn compile_local_test_policy(
    id: &str,
    policy: LocalTestDatabasePolicy,
) -> Result<CompiledLocalTestDatabasePolicy> {
    if policy.hosts.is_empty()
        || policy.database_suffixes.is_empty()
        || policy
            .hosts
            .iter()
            .chain(&policy.database_suffixes)
            .any(|value| value.trim().is_empty())
    {
        bail!("secret scan rule {id:?} has an invalid local test database policy");
    }
    Ok(CompiledLocalTestDatabasePolicy {
        hosts: policy
            .hosts
            .into_iter()
            .map(|host| host.to_ascii_lowercase())
            .collect(),
        database_suffixes: policy
            .database_suffixes
            .into_iter()
            .map(|suffix| suffix.to_ascii_lowercase())
            .collect(),
        require_username_equals_password: policy.require_username_equals_password,
    })
}

fn is_local_test_database(
    username: &[u8],
    password: &[u8],
    host: &[u8],
    database: &[u8],
    policy: &CompiledLocalTestDatabasePolicy,
) -> bool {
    let host = String::from_utf8_lossy(host).to_ascii_lowercase();
    let database = String::from_utf8_lossy(database).to_ascii_lowercase();
    policy.hosts.contains(&host)
        && policy
            .database_suffixes
            .iter()
            .any(|suffix| database.ends_with(suffix))
        && (!policy.require_username_equals_password || username == password)
}

fn webhook_url_contains_secret(
    value: &[u8],
    query_parameters: &HashSet<String>,
    query_minimum_length: usize,
    path_minimum_length: usize,
    placeholders: &PlaceholderConfig,
) -> bool {
    let Ok(value) = std::str::from_utf8(value) else {
        return false;
    };
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    if url.query_pairs().any(|(name, value)| {
        query_parameters.contains(name.to_ascii_lowercase().as_str())
            && looks_like_secret(value.as_bytes(), query_minimum_length, placeholders)
    }) {
        return true;
    }
    url.path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
        .is_some_and(|segment| {
            looks_like_secret(segment.as_bytes(), path_minimum_length, placeholders)
        })
}

fn looks_like_secret(
    value: &[u8],
    minimum_length: usize,
    placeholders: &PlaceholderConfig,
) -> bool {
    let value = String::from_utf8_lossy(value);
    let value = value.trim_matches(|character: char| {
        character.is_ascii_whitespace() || matches!(character, '"' | '\'')
    });
    if value.len() < minimum_length
        || placeholders
            .prefixes
            .iter()
            .any(|prefix| value.starts_with(prefix))
    {
        return false;
    }

    let lowercase = value.to_ascii_lowercase();
    if placeholders
        .markers
        .iter()
        .any(|marker| lowercase.contains(&marker.to_ascii_lowercase()))
        || placeholders
            .exact
            .iter()
            .any(|placeholder| lowercase == placeholder.to_ascii_lowercase())
    {
        return false;
    }

    let significant = value
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric())
        .collect::<Vec<_>>();
    let unique = significant.iter().copied().collect::<HashSet<_>>();
    significant.len()
        >= minimum_length.saturating_sub(placeholders.maximum_nonalphanumeric_characters)
        && unique.len() >= placeholders.minimum_unique_characters
}

#[derive(Debug, Clone, Copy)]
pub enum SecretMode {
    WorkingTree,
    Staged,
}

#[derive(Debug, Serialize)]
struct SecretReport<'a> {
    timestamp: String,
    mode: &'a str,
    findings: &'a [String],
}

fn scanner_for_mode(project: &Project, mode: SecretMode) -> Result<SecretScanner> {
    match mode {
        SecretMode::WorkingTree => SecretScanner::load(&project.secrets_config),
        SecretMode::Staged => {
            let relative = project
                .secrets_config
                .strip_prefix(&project.root)
                .context("secret scan configuration must stay inside the project")?;
            let relative = relative
                .to_str()
                .context("secret scan configuration path must be UTF-8")?;
            let relative = relative.replace('\\', "/");
            let bytes = staged_file_bytes(project, &relative)?.ok_or_else(|| {
                anyhow::anyhow!("staged secret scan configuration is missing: {relative}")
            })?;
            let source = std::str::from_utf8(&bytes)
                .context("staged secret scan configuration must be UTF-8")?;
            SecretScanner::from_source(source)
                .with_context(|| format!("parse staged secret scan configuration {relative}"))
        }
    }
}

pub fn scan(project: &Project, mode: SecretMode) -> Result<Vec<String>> {
    let files = match mode {
        SecretMode::WorkingTree => git_files(
            project,
            &[
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "-z",
            ],
        )?,
        SecretMode::Staged => git_files(
            project,
            &[
                "diff",
                "--cached",
                "--diff-filter=ACMR",
                "--name-only",
                "-z",
            ],
        )?,
    };
    let patterns = scanner_for_mode(project, mode)?;
    let mut findings = Vec::new();

    for file in files {
        let bytes = match mode {
            SecretMode::WorkingTree => match fs::read(project.root.join(&file)) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error).with_context(|| format!("read {file}")),
            },
            SecretMode::Staged => match staged_file_bytes(project, &file)? {
                Some(bytes) => bytes,
                None => continue,
            },
        };
        if patterns.is_match(&bytes) {
            findings.push(file);
        }
    }

    let mode_label = match mode {
        SecretMode::WorkingTree => "working-tree",
        SecretMode::Staged => "staged",
    };
    fs::create_dir_all(&project.reports)?;
    fs::write(
        project.reports.join("secret_scan.json"),
        serde_json::to_string_pretty(&SecretReport {
            timestamp: chrono::Utc::now().to_rfc3339(),
            mode: mode_label,
            findings: &findings,
        })?,
    )?;
    Ok(findings)
}

fn staged_file_bytes(project: &Project, file: &str) -> Result<Option<Vec<u8>>> {
    let args = vec!["show".to_string(), format!(":{file}")];
    let output = crate::process::capture("git", &args, &project.root, Duration::from_secs(30))
        .with_context(|| format!("read staged file {file}"))?;
    Ok(output.status.success().then_some(output.stdout))
}

fn git_files(project: &Project, args: &[&str]) -> Result<Vec<String>> {
    let args = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    let output = crate::process::capture("git", &args, &project.root, Duration::from_secs(30))
        .with_context(|| format!("run git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!("git {} failed", args.join(" "));
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| String::from_utf8(entry.to_vec()).context("non-UTF-8 Git path"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn scanner() -> SecretScanner {
        SecretScanner::from_source(include_str!("../../../.codex/secrets.toml"))
            .expect("project secret scan config")
    }

    #[test]
    fn default_preset_secret_config_is_valid() {
        SecretScanner::from_source(include_str!("../presets/default.secrets.toml"))
            .expect("default preset secret scan config");
    }

    #[test]
    fn invalid_secret_config_fails_closed() {
        let invalid_capture = r#"
version = 2

[placeholders]
minimum_unique_characters = 4
maximum_nonalphanumeric_characters = 2
prefixes = ["${"]
markers = ["change-me"]
exact = ["password"]

[[rules]]
id = "broken"
kind = "value"
pattern = "no-capture"
capture = 1
minimum_length = 8
"#;
        let empty_rules = r#"
version = 2
rules = []

[placeholders]
minimum_unique_characters = 4
maximum_nonalphanumeric_characters = 2
prefixes = ["${"]
markers = ["change-me"]
exact = ["password"]
"#;

        assert!(SecretScanner::from_source(invalid_capture).is_err());
        assert!(SecretScanner::from_source(empty_rules).is_err());
    }

    #[test]
    fn placeholder_prefixes_are_configurable() {
        let placeholders = PlaceholderConfig {
            minimum_unique_characters: 4,
            maximum_nonalphanumeric_characters: 2,
            markers: Vec::new(),
            exact: Vec::new(),
            prefixes: vec!["ref:".to_string()],
        };

        assert!(!looks_like_secret(
            b"ref:correct-horse-battery-staple",
            12,
            &placeholders
        ));
        assert!(looks_like_secret(
            b"${correct-horse-battery-staple}",
            12,
            &placeholders
        ));
    }

    #[test]
    fn local_test_database_policy_is_configurable() {
        let policy = compile_local_test_policy(
            "postgres",
            LocalTestDatabasePolicy {
                hosts: vec!["db.internal".to_string()],
                database_suffixes: vec!["_sandbox".to_string()],
                require_username_equals_password: false,
            },
        )
        .expect("compile local database policy");

        assert!(is_local_test_database(
            b"app",
            b"different-password",
            b"db.internal",
            b"arc_admin_sandbox",
            &policy
        ));
        assert!(!is_local_test_database(
            b"postgres",
            b"postgres",
            b"localhost",
            b"arc_admin_test",
            &policy
        ));
    }

    #[test]
    fn staged_scan_uses_the_staged_secret_config() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("arc-flow-staged-secrets-{unique}"));
        fs::create_dir_all(&root).expect("create staged scan fixture");
        crate::preset::init(&root, "generic", false).expect("initialize staged scan fixture");
        assert!(Command::new("git")
            .args(["-C", root.to_str().expect("UTF-8 path"), "init", "--quiet"])
            .status()
            .expect("initialize Git fixture")
            .success());
        assert!(Command::new("git")
            .args([
                "-C",
                root.to_str().expect("UTF-8 path"),
                "add",
                "--",
                ".arc-flow/secrets.toml",
            ])
            .status()
            .expect("stage secret config")
            .success());
        let project = Project::discover(Some(root.clone()), None).expect("discover Git fixture");
        fs::write(
            &project.secrets_config,
            "version = 2\nrules = []\n[placeholders]\nminimum_unique_characters = 4\nmaximum_nonalphanumeric_characters = 2\nprefixes = []\nmarkers = []\nexact = []\n",
        )
        .expect("replace working-tree secret config");

        assert!(scanner_for_mode(&project, SecretMode::Staged).is_ok());
        assert!(scanner_for_mode(&project, SecretMode::WorkingTree).is_err());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn detects_direct_tokens_without_matching_placeholders() {
        let patterns = scanner();
        let github_token = ["token=gh", "p_abcdefghijklmnopqrstuvwxyz123456"].concat();
        let access_key = ["AK", "IAIOSFODNN7EXAMPLE"].concat();
        let jwt = [
            "eyJhbGciOiJIUzI1NiIs",
            "InR5cCI6IkpXVCJ9",
            ".eyJzdWIiOiIxMjM0NTY3ODkwIn0",
            ".SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
        ]
        .concat();

        assert!(patterns.is_match(github_token.as_bytes()));
        assert!(patterns.is_match(access_key.as_bytes()));
        assert!(patterns.is_match(jwt.as_bytes()));
        assert!(!patterns.is_match(b"JWT_SECRET=change-me-in-production"));
    }

    #[test]
    fn detects_named_jwt_and_enterprise_messaging_secrets() {
        let patterns = scanner();
        let jwt_secret = ["JWT_", "SECRET=correct-horse-battery-staple"].concat();
        let wecom_key = ["WECOM_WEBHOOK_", "KEY=8fbf86b6-4f96-4b69-a97c-6ec55f845db1"].concat();
        let dingtalk_secret = [
            "\"DINGTALK_APP_",
            "SECRET\": \"SECc65f4f1654f544f9ba2a71eb4d498\"",
        ]
        .concat();

        assert!(patterns.is_match(jwt_secret.as_bytes()));
        assert!(patterns.is_match(wecom_key.as_bytes()));
        assert!(patterns.is_match(dingtalk_secret.as_bytes()));
        assert!(!patterns.is_match(["JWT_SECRET=", "$", "{JWT_SECRET}"].concat().as_bytes()));
        assert!(!patterns.is_match(b"WECOM_WEBHOOK_KEY=replace-me"));
    }

    #[test]
    fn detects_postgres_credentials_and_secret_bearing_webhooks() {
        let patterns = scanner();
        let database_url = [
            "DATABASE_URL=postgresql://app:",
            "m4pL9vQ2sR7x@db.internal/arc_admin",
        ]
        .concat();
        let wecom_url = [
            "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=",
            "8fbf86b6-4f96-4b69-a97c-6ec55f845db1",
        ]
        .concat();
        let dingtalk_url = [
            "WEBHOOK_URL=https://oapi.dingtalk.com/robot/send?access_token=",
            "4a8f57c90e51458a825b82d78948bffd",
        ]
        .concat();
        let generic_webhook = [
            "ALERT_WEBHOOK_URL=https://hooks.internal.example/notify/",
            "c51d43d18e6046e0b4ae192c187a44c7",
        ]
        .concat();

        assert!(patterns.is_match(database_url.as_bytes()));
        assert!(patterns.is_match(wecom_url.as_bytes()));
        assert!(patterns.is_match(dingtalk_url.as_bytes()));
        assert!(patterns.is_match(generic_webhook.as_bytes()));
        assert!(!patterns
            .is_match(b"DATABASE_URL=postgresql://postgres:postgres@localhost/arc_admin_test"));
        let remote_test_database = [
            "DATABASE_URL=postgresql://arc_admin_test:",
            "arc_admin_test@db.internal/arc_admin_test",
        ]
        .concat();
        assert!(patterns.is_match(remote_test_database.as_bytes()));
        assert!(!patterns.is_match(
            [
                "WEBHOOK_URL=https://example.com/hooks/",
                "$",
                "{WEBHOOK_TOKEN}"
            ]
            .concat()
            .as_bytes()
        ));
    }
}
