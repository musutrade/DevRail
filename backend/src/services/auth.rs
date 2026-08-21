//! Authentication service: password verification, session lifecycle, throttling, and auditing.

use crate::auth::{self, AuthSessionConfig};
use crate::error::{db_error, ApiError};
use crate::mfa::MfaConfig;
use crate::models::{
    user_response, ChangePasswordRequest, LoginRequest, LoginResponse, LoginStatusSchema,
    PermissionCodes, UserResponse, UserRow,
};
use crate::repositories;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgConnection, PgPool};
use std::net::IpAddr;
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;

pub const MIN_PASSWORD_LENGTH: usize = 12;
pub const MAX_PASSWORD_LENGTH: usize = 128;
const BOOTSTRAP_PASSWORD_LENGTH: usize = 16;
const ARGON2_MAX_CONCURRENCY: usize = 4;
const DUMMY_PASSWORD_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$pDDhKh46fVQNqRy3OeXTTw$+5qvGkvmKsilvsRWsskXT4k6fmmE4q35ntz6ME1UNBE";

pub enum LoginOutcome {
    Authenticated(LoginSessionOutcome),
    MfaRequired(LoginResponse),
}

pub struct LoginSessionOutcome {
    pub response: LoginResponse,
    pub session_token: String,
    pub csrf_token: String,
    pub persistent: bool,
    pub ttl_secs: i64,
}

pub(crate) struct LoginThrottleKeys {
    pub(crate) account: String,
    pub(crate) source_ip: String,
    pub(crate) account_ip: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LoginContext {
    pub account: String,
    pub source_ip: String,
    pub account_ip: String,
}

impl LoginThrottleKeys {
    fn new(username: &str, client_ip: IpAddr) -> Self {
        let username = username.to_ascii_lowercase();
        Self {
            account: auth::token_hash(&format!("account:{username}")),
            source_ip: auth::token_hash(&format!("source_ip:{client_ip}")),
            account_ip: auth::token_hash(&format!("account_ip:{username}:{client_ip}")),
        }
    }

    fn all(&self) -> [&str; 3] {
        [&self.account, &self.source_ip, &self.account_ip]
    }
}

pub async fn login(
    pool: &PgPool,
    config: &AuthSessionConfig,
    mfa: &MfaConfig,
    req: &LoginRequest,
    client_ip: IpAddr,
) -> Result<LoginOutcome, ApiError> {
    let username = req.username.trim();
    let throttle_keys = LoginThrottleKeys::new(username, client_ip);
    if let Some(locked_until) = locked_until(pool, &throttle_keys).await? {
        let retry_after = (locked_until - Utc::now()).num_seconds().max(1) as u64;
        return Err(ApiError::rate_limited(retry_after));
    }

    let row = repositories::users::find_by_username(pool, username)
        .await
        .map_err(db_error)?;
    let password_hash = row
        .as_ref()
        .map(|user| user.password_hash.as_str())
        .unwrap_or(DUMMY_PASSWORD_HASH);
    let password_valid = if req.password.chars().count() <= MAX_PASSWORD_LENGTH {
        verify_login_password(&req.password, password_hash).await?
    } else {
        false
    };
    let authenticated = row
        .as_ref()
        .is_some_and(|user| user.status == "active" && password_valid);
    if !authenticated {
        let locked_until = record_failed_login(
            pool,
            config,
            &throttle_keys,
            row.as_ref().map(|user| user.id),
        )
        .await?;
        if let Some(locked_until) = locked_until {
            let retry_after = (locked_until - Utc::now()).num_seconds().max(1) as u64;
            return Err(ApiError::rate_limited(retry_after));
        }
        return Err(ApiError::unauthorized());
    }
    let row = row.expect("authenticated login always has a user row");

    if let Some(response) = crate::services::mfa::begin_login(
        pool,
        mfa,
        &row,
        req.remember,
        LoginContext {
            account: throttle_keys.account.clone(),
            source_ip: throttle_keys.source_ip.clone(),
            account_ip: throttle_keys.account_ip.clone(),
        },
    )
    .await?
    {
        return Ok(LoginOutcome::MfaRequired(response));
    }

    Ok(LoginOutcome::Authenticated(
        create_login_session(pool, config, req.remember, throttle_keys, row).await?,
    ))
}

pub(crate) async fn create_login_session(
    pool: &PgPool,
    config: &AuthSessionConfig,
    persistent: bool,
    throttle_keys: LoginThrottleKeys,
    row: UserRow,
) -> Result<LoginSessionOutcome, ApiError> {
    let (ttl_secs, idle_timeout_secs) = if persistent {
        (
            config.persistent_session_ttl_secs,
            config.persistent_session_idle_timeout_secs,
        )
    } else {
        (config.session_ttl_secs, config.session_idle_timeout_secs)
    };
    let expires_at = Utc::now() + Duration::seconds(ttl_secs);
    let session_token = auth::random_token();
    let csrf_token = auth::random_token();
    let mut transaction = pool.begin().await.map_err(db_error)?;
    repositories::users::update_last_login(&mut transaction, row.id)
        .await
        .map_err(db_error)?;
    repositories::auth_sessions::clear_login_failures(&mut transaction, &throttle_keys.account)
        .await
        .map_err(db_error)?;
    repositories::auth_sessions::clear_login_failures(&mut transaction, &throttle_keys.account_ip)
        .await
        .map_err(db_error)?;
    let session = repositories::auth_sessions::NewSession {
        user_id: row.id,
        session_token_hash: auth::token_hash(&session_token),
        csrf_token_hash: auth::token_hash(&csrf_token),
        token_version: row.token_version,
        idle_timeout_secs,
        persistent,
        expires_at,
    };
    let session_id = repositories::auth_sessions::create(&mut transaction, &session)
        .await
        .map_err(db_error)?;
    let revoked_sessions = repositories::auth_sessions::enforce_user_limit(
        &mut transaction,
        row.id,
        config.max_sessions_per_user,
    )
    .await
    .map_err(db_error)?;
    record_session_revocation(
        &mut transaction,
        Some(row.id),
        row.id,
        "session_limit",
        revoked_sessions,
    )
    .await?;
    repositories::auth_sessions::prune(&mut transaction)
        .await
        .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut transaction,
        Some(row.id),
        "auth.login.success",
        "auth_session",
        Some(session_id),
        json!({
            "persistent": persistent,
            "expiresAt": expires_at,
            "revokedBySessionLimit": revoked_sessions,
            "sourceIpFingerprint": &throttle_keys.source_ip[..12],
        }),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;

    let roles = repositories::users::role_names_by_user(pool, row.id)
        .await
        .map_err(db_error)?;
    Ok(LoginSessionOutcome {
        response: LoginResponse {
            status: LoginStatusSchema::Authenticated,
            expires_at: Some(expires_at),
            user: Some(user_response(row, roles)),
            challenge_token: None,
            methods: Vec::new(),
            totp_secret: None,
            totp_uri: None,
            totp_qr_code: None,
            recovery_codes: Vec::new(),
        },
        session_token,
        csrf_token,
        persistent,
        ttl_secs,
    })
}

async fn record_failed_login(
    pool: &PgPool,
    config: &AuthSessionConfig,
    throttle_keys: &LoginThrottleKeys,
    user_id: Option<i64>,
) -> Result<Option<chrono::DateTime<Utc>>, ApiError> {
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let dimensions = [
        ("account", &throttle_keys.account, config.login_max_failures),
        (
            "source_ip",
            &throttle_keys.source_ip,
            config.login_ip_max_failures,
        ),
        (
            "account_ip",
            &throttle_keys.account_ip,
            config.login_account_ip_max_failures,
        ),
    ];
    let mut locked_until = None;
    let mut locked_dimensions = Vec::new();
    for (dimension, key, max_failures) in dimensions {
        let current_lock = repositories::auth_sessions::record_login_failure(
            &mut transaction,
            key,
            max_failures,
            config.login_failure_window_secs,
            config.login_lockout_secs,
        )
        .await
        .map_err(db_error)?;
        if let Some(current_lock) = current_lock {
            locked_dimensions.push(dimension);
            if locked_until.is_none_or(|existing| current_lock > existing) {
                locked_until = Some(current_lock);
            }
        }
    }
    repositories::audit_logs::record(
        &mut transaction,
        None,
        "auth.login.failure",
        "user",
        user_id,
        json!({
            "identifierFingerprint": &throttle_keys.account[..12],
            "sourceIpFingerprint": &throttle_keys.source_ip[..12],
            "locked": locked_until.is_some(),
            "lockedDimensions": locked_dimensions,
        }),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(locked_until)
}

async fn locked_until(
    pool: &PgPool,
    throttle_keys: &LoginThrottleKeys,
) -> Result<Option<chrono::DateTime<Utc>>, ApiError> {
    let mut latest = None;
    for key in throttle_keys.all() {
        if let Some(current) = repositories::auth_sessions::locked_until(pool, key)
            .await
            .map_err(db_error)?
        {
            if latest.is_none_or(|existing| current > existing) {
                latest = Some(current);
            }
        }
    }
    Ok(latest)
}

pub async fn record_session_revocation(
    connection: &mut PgConnection,
    actor_user_id: Option<i64>,
    target_user_id: i64,
    reason: &str,
    revoked_session_count: u64,
) -> Result<(), ApiError> {
    if revoked_session_count == 0 {
        return Ok(());
    }
    repositories::audit_logs::record(
        connection,
        actor_user_id,
        "auth.session.revoked",
        "user",
        Some(target_user_id),
        json!({
            "reason": reason,
            "revokedSessionCount": revoked_session_count,
        }),
    )
    .await
    .map_err(db_error)
}

pub async fn logout(pool: &PgPool, user_id: i64, session_id: i64) -> Result<(), ApiError> {
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let revoked = repositories::auth_sessions::revoke(&mut transaction, session_id, user_id)
        .await
        .map_err(db_error)?;
    record_session_revocation(
        &mut transaction,
        Some(user_id),
        user_id,
        "logout",
        u64::from(revoked),
    )
    .await?;
    repositories::audit_logs::record(
        &mut transaction,
        Some(user_id),
        "auth.logout",
        "auth_session",
        Some(session_id),
        json!({}),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(())
}

pub async fn me(pool: &PgPool, user_id: i64) -> Result<UserResponse, ApiError> {
    let row = repositories::users::find_by_id(pool, user_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("用户不存在"))?;
    let roles = repositories::users::role_names_by_user(pool, user_id)
        .await
        .map_err(db_error)?;
    Ok(user_response(row, roles))
}

pub async fn change_password(
    pool: &PgPool,
    user_id: i64,
    req: &ChangePasswordRequest,
) -> Result<(), ApiError> {
    if req.current_password.is_empty() {
        return Err(ApiError::validation("当前密码不能为空"));
    }
    validate_password(&req.new_password)?;
    if req.current_password == req.new_password {
        return Err(ApiError::validation("新密码不能与当前密码相同"));
    }

    verify_current_password(pool, user_id, &req.current_password).await?;

    let password_hash = hash_password_async(&req.new_password).await?;
    let mut transaction = pool.begin().await.map_err(db_error)?;
    repositories::users::update_password(&mut transaction, user_id, &password_hash)
        .await
        .map_err(db_error)?
        .ok_or_else(ApiError::unauthorized)?;
    let revoked_sessions =
        repositories::auth_sessions::revoke_all_for_user(&mut transaction, user_id)
            .await
            .map_err(db_error)?;
    record_session_revocation(
        &mut transaction,
        Some(user_id),
        user_id,
        "password_change",
        revoked_sessions,
    )
    .await?;
    repositories::audit_logs::record(
        &mut transaction,
        Some(user_id),
        "user.password.change",
        "user",
        Some(user_id),
        json!({
            "revokedExistingSessions": true,
            "revokedSessionCount": revoked_sessions,
        }),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(())
}

pub async fn verify_current_password(
    pool: &PgPool,
    user_id: i64,
    password: &str,
) -> Result<(), ApiError> {
    let row = repositories::users::find_by_id(pool, user_id)
        .await
        .map_err(db_error)?
        .ok_or_else(ApiError::unauthorized)?;
    let password_matches = verify_password_hash(password, &row.password_hash).await?;
    if password_matches {
        Ok(())
    } else {
        Err(ApiError::validation("当前密码不正确"))
    }
}

pub async fn permission_codes(pool: &PgPool, user_id: i64) -> Result<PermissionCodes, ApiError> {
    let codes = repositories::permissions::permission_codes_by_user(pool, user_id)
        .await
        .map_err(db_error)?;
    Ok(PermissionCodes { codes })
}

pub async fn bootstrap_super_admin(
    pool: &PgPool,
    username: &str,
    password: &str,
    display_name: &str,
    email: Option<String>,
) -> Result<UserResponse, ApiError> {
    let username = username.trim();
    if !(3..=64).contains(&username.len())
        || !username.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
    {
        return Err(ApiError::validation(
            "管理员用户名需为 3-64 位字母、数字、下划线或连字符",
        ));
    }
    validate_password(password)?;
    if password.chars().count() < BOOTSTRAP_PASSWORD_LENGTH {
        return Err(ApiError::validation("引导管理员密码不能少于 16 位"));
    }
    let display_name = display_name.trim();
    if display_name.is_empty() || display_name.len() > 128 {
        return Err(ApiError::validation("显示名称长度需在 1-128 个字符之间"));
    }

    let role_id = repositories::roles::id_by_code(pool, "super_admin")
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::internal("缺少内置超级管理员角色"))?;
    let existing = repositories::users::find_by_username(pool, username)
        .await
        .map_err(db_error)?;
    let (organization_id, department_id) = repositories::organizations::default_assignment(pool)
        .await
        .map_err(db_error)?;
    let password_hash = hash_password_async(password).await?;
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let row = if let Some(existing) = existing {
        repositories::users::activate_bootstrap_account(
            &mut transaction,
            existing.id,
            &password_hash,
            display_name,
            email,
        )
        .await
        .map_err(db_error)?
    } else {
        let user = repositories::users::NewUser {
            username: username.to_string(),
            password_hash,
            display_name: display_name.to_string(),
            email,
            status: "active".to_string(),
            organization_id,
            department_id,
        };
        repositories::users::create(&mut transaction, &user)
            .await
            .map_err(db_error)?
    };
    repositories::users::assign_roles(&mut transaction, row.id, &[role_id])
        .await
        .map_err(db_error)?;
    let revoked_sessions =
        repositories::auth_sessions::revoke_all_for_user(&mut transaction, row.id)
            .await
            .map_err(db_error)?;
    record_session_revocation(
        &mut transaction,
        None,
        row.id,
        "bootstrap_reactivation",
        revoked_sessions,
    )
    .await?;
    repositories::audit_logs::record(
        &mut transaction,
        None,
        "user.bootstrap_super_admin",
        "user",
        Some(row.id),
        json!({ "username": username, "roleId": role_id }),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    let roles = repositories::users::role_names_by_user(pool, row.id)
        .await
        .map_err(db_error)?;
    Ok(user_response(row, roles))
}

pub fn validate_password(password: &str) -> Result<(), ApiError> {
    let length = password.chars().count();
    if !(MIN_PASSWORD_LENGTH..=MAX_PASSWORD_LENGTH).contains(&length) {
        return Err(ApiError::validation("密码长度需在 12-128 个字符之间"));
    }
    Ok(())
}

/// Argon2 password hash in PHC format with a cryptographically random salt.
pub fn hash_password(password: &str) -> Result<String, ApiError> {
    let salt = auth::password_salt();
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| ApiError::internal(error.to_string()))
}

pub async fn hash_password_async(password: &str) -> Result<String, ApiError> {
    let password = password.to_string();
    run_argon2_task(move || hash_password(&password)).await
}

pub(crate) async fn verify_password_hash(
    password: &str,
    encoded_hash: &str,
) -> Result<bool, ApiError> {
    let password = password.to_string();
    let encoded_hash = encoded_hash.to_string();
    run_argon2_task(move || {
        let parsed = PasswordHash::new(&encoded_hash).map_err(|error| {
            ApiError::internal(format!("invalid stored password hash: {error}"))
        })?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    })
    .await
}

async fn verify_login_password(password: &str, encoded_hash: &str) -> Result<bool, ApiError> {
    let password = password.to_string();
    let encoded_hash = encoded_hash.to_string();
    run_argon2_task(move || {
        Ok(PasswordHash::new(&encoded_hash).ok().is_some_and(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        }))
    })
    .await
}

pub(crate) async fn run_argon2_task<T, F>(task: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ApiError> + Send + 'static,
{
    static LIMITER: OnceLock<Arc<Semaphore>> = OnceLock::new();
    let limiter = LIMITER
        .get_or_init(|| {
            let permits = std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1)
                .clamp(1, ARGON2_MAX_CONCURRENCY);
            Arc::new(Semaphore::new(permits))
        })
        .clone();
    let permit = limiter
        .acquire_owned()
        .await
        .map_err(|error| ApiError::internal(format!("Argon2 limiter closed: {error}")))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        task()
    })
    .await
    .map_err(|error| ApiError::internal(format!("Argon2 worker failed: {error}")))?
}
