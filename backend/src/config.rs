//! Runtime configuration and production safety checks.

use crate::auth::CSRF_HEADER;
use crate::db::{DatabasePoolConfig, DatabasePoolConfigValues};
use crate::telemetry::REQUEST_ID_HEADER;
use anyhow::{bail, Context};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderValue, Method};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ipnet::IpNet;
use sha2::{Digest, Sha256};
use tower_http::cors::{AllowOrigin, CorsLayer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEnvironment {
    Development,
    Test,
    Production,
}

impl AppEnvironment {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Test => "test",
            Self::Production => "production",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Pretty,
    Json,
}

impl LogFormat {
    pub fn bootstrap_from_env() -> Self {
        match std::env::var("LOG_FORMAT").as_deref() {
            Ok("json") => Self::Json,
            Ok("pretty") => Self::Pretty,
            _ if matches!(
                std::env::var("APP_ENV").as_deref(),
                Ok("production" | "prod")
            ) =>
            {
                Self::Json
            }
            _ => Self::Pretty,
        }
    }
}

pub struct AppConfig {
    pub database_url: String,
    pub database_pool: DatabasePoolConfig,
    pub auto_migrate: bool,
    pub port: u16,
    pub session_ttl_secs: i64,
    pub session_idle_timeout_secs: i64,
    pub persistent_session_ttl_secs: i64,
    pub persistent_session_idle_timeout_secs: i64,
    pub max_sessions_per_user: i64,
    pub login_max_failures: i32,
    pub login_ip_max_failures: i32,
    pub login_account_ip_max_failures: i32,
    pub login_failure_window_secs: i64,
    pub login_lockout_secs: i64,
    pub trusted_proxy_cidrs: Vec<IpNet>,
    pub environment: AppEnvironment,
    pub log_format: LogFormat,
    pub service_name: String,
    pub mfa_encryption_key: Vec<u8>,
    pub webauthn_rp_id: String,
    pub webauthn_rp_origin: String,
    pub webauthn_rp_name: String,
    pub harness_command: String,
    pub harness_max_concurrency: usize,
    pub run_max_duration_secs: i64,
    pub run_workspace_root: String,
    pub run_graceful_interrupt_secs: i64,
    pub web_push_public_key: Option<String>,
    pub web_push_private_key: Option<String>,
    pub web_push_subject: Option<String>,
    allowed_origins: Vec<HeaderValue>,
}

#[derive(Default)]
struct ConfigValues {
    database_url: Option<String>,
    db_max_connections: Option<String>,
    db_min_connections: Option<String>,
    db_acquire_timeout_secs: Option<String>,
    db_connect_timeout_secs: Option<String>,
    db_idle_timeout_secs: Option<String>,
    db_max_lifetime_secs: Option<String>,
    db_statement_timeout_ms: Option<String>,
    auto_migrate: Option<String>,
    port: Option<String>,
    session_ttl_secs: Option<String>,
    session_idle_timeout_secs: Option<String>,
    persistent_session_ttl_secs: Option<String>,
    persistent_session_idle_timeout_secs: Option<String>,
    max_sessions_per_user: Option<String>,
    login_max_failures: Option<String>,
    login_ip_max_failures: Option<String>,
    login_account_ip_max_failures: Option<String>,
    login_failure_window_secs: Option<String>,
    login_lockout_secs: Option<String>,
    trusted_proxy_cidrs: Option<String>,
    app_env: Option<String>,
    cors_allowed_origins: Option<String>,
    log_format: Option<String>,
    service_name: Option<String>,
    mfa_encryption_key: Option<String>,
    webauthn_rp_id: Option<String>,
    webauthn_rp_origin: Option<String>,
    webauthn_rp_name: Option<String>,
    harness_command: Option<String>,
    harness_max_concurrency: Option<String>,
    run_max_duration_secs: Option<String>,
    run_workspace_root: Option<String>,
    run_graceful_interrupt_secs: Option<String>,
    web_push_public_key: Option<String>,
    web_push_private_key: Option<String>,
    web_push_subject: Option<String>,
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_values(ConfigValues {
            database_url: std::env::var("DATABASE_URL").ok(),
            db_max_connections: std::env::var("DB_MAX_CONNECTIONS").ok(),
            db_min_connections: std::env::var("DB_MIN_CONNECTIONS").ok(),
            db_acquire_timeout_secs: std::env::var("DB_ACQUIRE_TIMEOUT_SECS").ok(),
            db_connect_timeout_secs: std::env::var("DB_CONNECT_TIMEOUT_SECS").ok(),
            db_idle_timeout_secs: std::env::var("DB_IDLE_TIMEOUT_SECS").ok(),
            db_max_lifetime_secs: std::env::var("DB_MAX_LIFETIME_SECS").ok(),
            db_statement_timeout_ms: std::env::var("DB_STATEMENT_TIMEOUT_MS").ok(),
            auto_migrate: std::env::var("AUTO_MIGRATE").ok(),
            port: std::env::var("PORT").ok(),
            session_ttl_secs: std::env::var("SESSION_TTL_SECS").ok(),
            session_idle_timeout_secs: std::env::var("SESSION_IDLE_TIMEOUT_SECS").ok(),
            persistent_session_ttl_secs: std::env::var("PERSISTENT_SESSION_TTL_SECS").ok(),
            persistent_session_idle_timeout_secs: std::env::var(
                "PERSISTENT_SESSION_IDLE_TIMEOUT_SECS",
            )
            .ok(),
            max_sessions_per_user: std::env::var("MAX_SESSIONS_PER_USER").ok(),
            login_max_failures: std::env::var("LOGIN_MAX_FAILURES").ok(),
            login_ip_max_failures: std::env::var("LOGIN_IP_MAX_FAILURES").ok(),
            login_account_ip_max_failures: std::env::var("LOGIN_ACCOUNT_IP_MAX_FAILURES").ok(),
            login_failure_window_secs: std::env::var("LOGIN_FAILURE_WINDOW_SECS").ok(),
            login_lockout_secs: std::env::var("LOGIN_LOCKOUT_SECS").ok(),
            trusted_proxy_cidrs: std::env::var("TRUSTED_PROXY_CIDRS").ok(),
            app_env: std::env::var("APP_ENV").ok(),
            cors_allowed_origins: std::env::var("CORS_ALLOWED_ORIGINS").ok(),
            log_format: std::env::var("LOG_FORMAT").ok(),
            service_name: std::env::var("SERVICE_NAME").ok(),
            mfa_encryption_key: std::env::var("MFA_ENCRYPTION_KEY").ok(),
            webauthn_rp_id: std::env::var("WEBAUTHN_RP_ID").ok(),
            webauthn_rp_origin: std::env::var("WEBAUTHN_RP_ORIGIN").ok(),
            webauthn_rp_name: std::env::var("WEBAUTHN_RP_NAME").ok(),
            harness_command: std::env::var("DEVRAIL_HARNESS_COMMAND").ok(),
            harness_max_concurrency: std::env::var("DEVRAIL_HARNESS_MAX_CONCURRENCY").ok(),
            run_max_duration_secs: std::env::var("DEVRAIL_RUN_MAX_DURATION_SECS").ok(),
            run_workspace_root: std::env::var("DEVRAIL_RUN_WORKSPACE_ROOT").ok(),
            run_graceful_interrupt_secs: std::env::var("DEVRAIL_RUN_GRACEFUL_INTERRUPT_SECS").ok(),
            web_push_public_key: std::env::var("WEB_PUSH_VAPID_PUBLIC_KEY").ok(),
            web_push_private_key: std::env::var("WEB_PUSH_VAPID_PRIVATE_KEY").ok(),
            web_push_subject: std::env::var("WEB_PUSH_SUBJECT").ok(),
        })
    }

    fn from_values(values: ConfigValues) -> anyhow::Result<Self> {
        let ConfigValues {
            database_url,
            db_max_connections,
            db_min_connections,
            db_acquire_timeout_secs,
            db_connect_timeout_secs,
            db_idle_timeout_secs,
            db_max_lifetime_secs,
            db_statement_timeout_ms,
            auto_migrate,
            port,
            session_ttl_secs,
            session_idle_timeout_secs,
            persistent_session_ttl_secs,
            persistent_session_idle_timeout_secs,
            max_sessions_per_user,
            login_max_failures,
            login_ip_max_failures,
            login_account_ip_max_failures,
            login_failure_window_secs,
            login_lockout_secs,
            trusted_proxy_cidrs,
            app_env,
            cors_allowed_origins,
            log_format,
            service_name,
            mfa_encryption_key,
            webauthn_rp_id,
            webauthn_rp_origin,
            webauthn_rp_name,
            harness_command,
            harness_max_concurrency,
            run_max_duration_secs,
            run_workspace_root,
            run_graceful_interrupt_secs,
            web_push_public_key,
            web_push_private_key,
            web_push_subject,
        } = values;
        let database_url = database_url.context(
            "DATABASE_URL is required; configure it in backend/.env or the process environment",
        )?;
        let environment = match app_env.as_deref().unwrap_or("development") {
            "development" | "dev" => AppEnvironment::Development,
            "test" => AppEnvironment::Test,
            "production" | "prod" => AppEnvironment::Production,
            value => bail!("APP_ENV must be development, test, or production; got {value:?}"),
        };
        let database_pool = DatabasePoolConfig::from_values(DatabasePoolConfigValues {
            max_connections: db_max_connections,
            min_connections: db_min_connections,
            acquire_timeout_secs: db_acquire_timeout_secs,
            connect_timeout_secs: db_connect_timeout_secs,
            idle_timeout_secs: db_idle_timeout_secs,
            max_lifetime_secs: db_max_lifetime_secs,
            statement_timeout_ms: db_statement_timeout_ms,
        })?;
        let auto_migrate = parse_bool(
            "AUTO_MIGRATE",
            auto_migrate,
            environment != AppEnvironment::Production,
        )?;
        let port = port
            .as_deref()
            .unwrap_or("8080")
            .parse::<u16>()
            .context("PORT must be an integer between 1 and 65535")?;
        if port == 0 {
            bail!("PORT must be an integer between 1 and 65535");
        }
        let session_ttl_secs = positive_i64("SESSION_TTL_SECS", session_ttl_secs, 28_800)?;
        let session_idle_timeout_secs = positive_i64(
            "SESSION_IDLE_TIMEOUT_SECS",
            session_idle_timeout_secs,
            1_800,
        )?;
        let persistent_session_ttl_secs = positive_i64(
            "PERSISTENT_SESSION_TTL_SECS",
            persistent_session_ttl_secs,
            2_592_000,
        )?;
        let persistent_session_idle_timeout_secs = positive_i64(
            "PERSISTENT_SESSION_IDLE_TIMEOUT_SECS",
            persistent_session_idle_timeout_secs,
            604_800,
        )?;
        let max_sessions_per_user =
            positive_i64("MAX_SESSIONS_PER_USER", max_sessions_per_user, 10)?;
        let login_max_failures = positive_i32("LOGIN_MAX_FAILURES", login_max_failures, 5)?;
        let login_ip_max_failures =
            positive_i32("LOGIN_IP_MAX_FAILURES", login_ip_max_failures, 50)?;
        let login_account_ip_max_failures = positive_i32(
            "LOGIN_ACCOUNT_IP_MAX_FAILURES",
            login_account_ip_max_failures,
            5,
        )?;
        let login_failure_window_secs =
            positive_i64("LOGIN_FAILURE_WINDOW_SECS", login_failure_window_secs, 900)?;
        let login_lockout_secs = positive_i64("LOGIN_LOCKOUT_SECS", login_lockout_secs, 900)?;
        let trusted_proxy_cidrs = parse_trusted_proxy_cidrs(trusted_proxy_cidrs)?;
        if session_idle_timeout_secs > session_ttl_secs {
            bail!("SESSION_IDLE_TIMEOUT_SECS cannot exceed SESSION_TTL_SECS");
        }
        if persistent_session_idle_timeout_secs > persistent_session_ttl_secs {
            bail!("PERSISTENT_SESSION_IDLE_TIMEOUT_SECS cannot exceed PERSISTENT_SESSION_TTL_SECS");
        }

        let allowed_origins = parse_origins(cors_allowed_origins)?;
        if environment == AppEnvironment::Production && allowed_origins.is_empty() {
            bail!("production requires CORS_ALLOWED_ORIGINS with at least one explicit origin");
        }
        let log_format = match log_format.as_deref() {
            Some("pretty") => LogFormat::Pretty,
            Some("json") => LogFormat::Json,
            Some(value) => bail!("LOG_FORMAT must be pretty or json; got {value:?}"),
            None if environment == AppEnvironment::Production => LogFormat::Json,
            None => LogFormat::Pretty,
        };
        let service_name = service_name.unwrap_or_else(|| "arc-admin-backend".to_string());
        if service_name.is_empty()
            || service_name.len() > 64
            || !service_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            bail!("SERVICE_NAME must be 1-64 ASCII letters, digits, '.', '_' or '-'");
        }

        let mfa_encryption_key = match mfa_encryption_key.filter(|value| !value.is_empty()) {
            Some(value) => STANDARD
                .decode(value)
                .context("MFA_ENCRYPTION_KEY must be base64 encoded")?,
            None if environment == AppEnvironment::Production => {
                bail!("production requires MFA_ENCRYPTION_KEY from the secret manager")
            }
            None => Sha256::digest(b"arc-admin-development-only-mfa-key").to_vec(),
        };
        if mfa_encryption_key.len() != 32 {
            bail!("MFA_ENCRYPTION_KEY must decode to exactly 32 bytes");
        }
        let webauthn_rp_id = webauthn_rp_id.unwrap_or_else(|| "localhost".to_string());
        let webauthn_rp_origin =
            webauthn_rp_origin.unwrap_or_else(|| "http://localhost:4200".to_string());
        let webauthn_rp_name = webauthn_rp_name.unwrap_or_else(|| "Arc Admin".to_string());
        if webauthn_rp_id.is_empty() || webauthn_rp_origin.is_empty() || webauthn_rp_name.is_empty()
        {
            bail!("WEBAUTHN_RP_ID, WEBAUTHN_RP_ORIGIN and WEBAUTHN_RP_NAME must not be empty");
        }

        let harness_command = harness_command.unwrap_or_else(|| "codex".to_string());
        if harness_command.is_empty()
            || harness_command.len() > 256
            || harness_command.contains('/')
        {
            bail!("DEVRAIL_HARNESS_COMMAND must be a non-empty executable name without path separators");
        }
        let harness_max_concurrency = positive_usize(
            "DEVRAIL_HARNESS_MAX_CONCURRENCY",
            harness_max_concurrency,
            2,
        )?;
        let run_max_duration_secs =
            positive_i64("DEVRAIL_RUN_MAX_DURATION_SECS", run_max_duration_secs, 3600)?;
        let run_graceful_interrupt_secs = positive_i64(
            "DEVRAIL_RUN_GRACEFUL_INTERRUPT_SECS",
            run_graceful_interrupt_secs,
            10,
        )?;
        let run_workspace_root =
            run_workspace_root.unwrap_or_else(|| "/tmp/devrail-workspaces".to_string());
        if !run_workspace_root.starts_with('/')
            || run_workspace_root.contains("..")
            || run_workspace_root.len() > 512
        {
            bail!("DEVRAIL_RUN_WORKSPACE_ROOT must be an absolute controlled path");
        }

        let web_push_public_key =
            non_empty_optional(web_push_public_key, "WEB_PUSH_VAPID_PUBLIC_KEY")?;
        let web_push_private_key =
            non_empty_optional(web_push_private_key, "WEB_PUSH_VAPID_PRIVATE_KEY")?;
        let web_push_subject = non_empty_optional(web_push_subject, "WEB_PUSH_SUBJECT")?;
        if web_push_subject
            .as_deref()
            .is_some_and(|value| !value.starts_with("mailto:") && !value.starts_with("https://"))
        {
            bail!("WEB_PUSH_SUBJECT must start with mailto: or https://");
        }

        Ok(Self {
            database_url,
            database_pool,
            auto_migrate,
            port,
            session_ttl_secs,
            session_idle_timeout_secs,
            persistent_session_ttl_secs,
            persistent_session_idle_timeout_secs,
            max_sessions_per_user,
            login_max_failures,
            login_ip_max_failures,
            login_account_ip_max_failures,
            login_failure_window_secs,
            login_lockout_secs,
            trusted_proxy_cidrs,
            environment,
            log_format,
            service_name,
            mfa_encryption_key,
            webauthn_rp_id,
            webauthn_rp_origin,
            webauthn_rp_name,
            harness_command,
            harness_max_concurrency,
            run_max_duration_secs,
            run_workspace_root,
            run_graceful_interrupt_secs,
            web_push_public_key,
            web_push_private_key,
            web_push_subject,
            allowed_origins,
        })
    }

    pub fn cors_layer(&self) -> CorsLayer {
        if self.allowed_origins.is_empty() {
            return CorsLayer::permissive();
        }

        CorsLayer::new()
            .allow_origin(AllowOrigin::list(self.allowed_origins.clone()))
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers([CONTENT_TYPE, CSRF_HEADER, REQUEST_ID_HEADER])
            .expose_headers([REQUEST_ID_HEADER])
            .allow_credentials(true)
    }
}

fn positive_i64(name: &str, value: Option<String>, default: i64) -> anyhow::Result<i64> {
    let value = value
        .as_deref()
        .map(str::parse::<i64>)
        .transpose()
        .with_context(|| format!("{name} must be a positive integer"))?
        .unwrap_or(default);
    if value <= 0 {
        bail!("{name} must be a positive integer");
    }
    Ok(value)
}

fn positive_i32(name: &str, value: Option<String>, default: i32) -> anyhow::Result<i32> {
    let value = value
        .as_deref()
        .map(str::parse::<i32>)
        .transpose()
        .with_context(|| format!("{name} must be a positive integer"))?
        .unwrap_or(default);
    if value <= 0 {
        bail!("{name} must be a positive integer");
    }
    Ok(value)
}

fn positive_usize(name: &str, value: Option<String>, default: usize) -> anyhow::Result<usize> {
    let value = value
        .as_deref()
        .map(str::parse::<usize>)
        .transpose()
        .with_context(|| format!("{name} must be a positive integer"))?
        .unwrap_or(default);
    if value == 0 {
        bail!("{name} must be a positive integer");
    }
    Ok(value)
}

fn parse_bool(name: &str, value: Option<String>, default: bool) -> anyhow::Result<bool> {
    match value.as_deref() {
        None => Ok(default),
        Some("true" | "1") => Ok(true),
        Some("false" | "0") => Ok(false),
        Some(value) => bail!("{name} must be true or false; got {value:?}"),
    }
}

fn parse_origins(value: Option<String>) -> anyhow::Result<Vec<HeaderValue>> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(|origin| {
            if origin == "*" {
                bail!("CORS_ALLOWED_ORIGINS must contain explicit origins, not '*'");
            }
            origin
                .parse::<HeaderValue>()
                .with_context(|| format!("invalid CORS origin {origin:?}"))
        })
        .collect()
}

fn non_empty_optional(value: Option<String>, name: &str) -> anyhow::Result<Option<String>> {
    value
        .map(|value| {
            if value.trim().is_empty() {
                Err(anyhow::anyhow!("{name} must not be empty when configured"))
            } else {
                Ok(value)
            }
        })
        .transpose()
}

fn parse_trusted_proxy_cidrs(value: Option<String>) -> anyhow::Result<Vec<IpNet>> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|cidr| !cidr.is_empty())
        .map(|cidr| {
            cidr.parse::<IpNet>()
                .with_context(|| format!("invalid trusted proxy CIDR {cidr:?}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(environment: &str, origins: Option<&str>) -> anyhow::Result<AppConfig> {
        AppConfig::from_values(ConfigValues {
            database_url: Some("postgres://localhost/test".to_string()),
            app_env: Some(environment.to_string()),
            cors_allowed_origins: origins.map(str::to_string),
            mfa_encryption_key: Some(STANDARD.encode([0_u8; 32])),
            ..ConfigValues::default()
        })
    }

    #[test]
    fn development_has_safe_local_defaults() {
        let config = config("development", None).expect("development config");

        assert_eq!(config.environment, AppEnvironment::Development);
        assert_eq!(config.log_format, LogFormat::Pretty);
        assert_eq!(config.session_ttl_secs, 28_800);
        assert_eq!(config.login_max_failures, 5);
        assert_eq!(config.login_ip_max_failures, 50);
        assert!(config.auto_migrate);
        assert!(config.trusted_proxy_cidrs.is_empty());
    }

    #[test]
    fn empty_mfa_encryption_key_uses_non_production_default() {
        let config = AppConfig::from_values(ConfigValues {
            database_url: Some("postgres://localhost/test".to_string()),
            app_env: Some("test".to_string()),
            mfa_encryption_key: Some(String::new()),
            ..ConfigValues::default()
        })
        .expect("empty non-production MFA key should use the default");

        assert_eq!(
            config.mfa_encryption_key,
            Sha256::digest(b"arc-admin-development-only-mfa-key").to_vec()
        );
    }

    #[test]
    fn production_requires_explicit_cors_origins() {
        let error = config("production", None)
            .err()
            .expect("missing origins must fail");

        assert!(error.to_string().contains("CORS_ALLOWED_ORIGINS"));
    }

    #[test]
    fn production_uses_json_logs_by_default() {
        let config =
            config("production", Some("https://admin.example.com")).expect("production config");

        assert_eq!(config.log_format, LogFormat::Json);
        assert!(!config.auto_migrate);
    }

    #[test]
    fn idle_timeout_cannot_exceed_absolute_session_ttl() {
        let error = AppConfig::from_values(ConfigValues {
            database_url: Some("postgres://localhost/test".to_string()),
            session_ttl_secs: Some("60".to_string()),
            session_idle_timeout_secs: Some("61".to_string()),
            ..ConfigValues::default()
        })
        .err()
        .expect("invalid session timeouts must fail");

        assert!(error.to_string().contains("SESSION_IDLE_TIMEOUT_SECS"));
    }

    #[test]
    fn trusted_proxy_cidrs_are_validated() {
        let config = AppConfig::from_values(ConfigValues {
            database_url: Some("postgres://localhost/test".to_string()),
            trusted_proxy_cidrs: Some("10.0.0.0/8, 2001:db8::/32".to_string()),
            ..ConfigValues::default()
        })
        .expect("valid trusted proxy CIDRs");
        assert_eq!(config.trusted_proxy_cidrs.len(), 2);

        let error = AppConfig::from_values(ConfigValues {
            database_url: Some("postgres://localhost/test".to_string()),
            trusted_proxy_cidrs: Some("not-a-cidr".to_string()),
            ..ConfigValues::default()
        })
        .err()
        .expect("invalid trusted proxy CIDR must fail");
        assert!(error.to_string().contains("trusted proxy CIDR"));
    }
}
