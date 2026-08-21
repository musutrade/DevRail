//! Short-lived, session-bound re-authentication for high-risk mutations.

use crate::auth as session_auth;
use crate::error::{db_error, ApiError};
use crate::mfa::MfaConfig;
use crate::models::{StepUpRequest, StepUpResponse};
use crate::repositories;
use crate::services::mfa;
use axum::http::HeaderMap;
use chrono::{Duration, Utc};
use serde_json::json;
use sqlx::PgPool;

pub const HEADER: &str = "X-Step-Up-Token";
pub const PASSWORD_CHANGE_SCOPE: &str = "auth.password.change";
pub const USERS_SENSITIVE_SCOPE: &str = "users.sensitive";
pub const USERS_ROLES_SCOPE: &str = "users.roles.write";
pub const USERS_DELETE_SCOPE: &str = "users.delete";
pub const ROLES_SENSITIVE_SCOPE: &str = "roles.sensitive";
pub const ROLES_PERMISSIONS_SCOPE: &str = "roles.permissions.write";
pub const ROLES_DELETE_SCOPE: &str = "roles.delete";
pub const DEPARTMENTS_WRITE_SCOPE: &str = "departments.write";
pub const DEPARTMENTS_DELETE_SCOPE: &str = "departments.delete";
const TOKEN_TTL_SECS: i64 = 300;

const VALID_SCOPES: &[&str] = &[
    PASSWORD_CHANGE_SCOPE,
    USERS_SENSITIVE_SCOPE,
    USERS_ROLES_SCOPE,
    USERS_DELETE_SCOPE,
    ROLES_SENSITIVE_SCOPE,
    ROLES_PERMISSIONS_SCOPE,
    ROLES_DELETE_SCOPE,
    DEPARTMENTS_WRITE_SCOPE,
    DEPARTMENTS_DELETE_SCOPE,
];

pub fn is_valid_scope(scope: &str) -> bool {
    VALID_SCOPES.contains(&scope)
}

pub async fn issue(
    pool: &PgPool,
    mfa_config: &MfaConfig,
    session_id: i64,
    user_id: i64,
    req: &StepUpRequest,
) -> Result<StepUpResponse, ApiError> {
    if !is_valid_scope(&req.scope) {
        return Err(ApiError::validation("无效的再认证操作范围"));
    }
    mfa::verify_reauthentication(
        pool,
        mfa_config,
        user_id,
        &req.current_password,
        req.totp_code.as_deref(),
    )
    .await?;

    let token = session_auth::random_token();
    let expires_at = Utc::now() + Duration::seconds(TOKEN_TTL_SECS);
    let mut transaction = pool.begin().await.map_err(db_error)?;
    repositories::step_up::prune(&mut transaction)
        .await
        .map_err(db_error)?;
    repositories::step_up::create(
        &mut transaction,
        &session_auth::token_hash(&token),
        session_id,
        user_id,
        &req.scope,
        expires_at,
    )
    .await
    .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut transaction,
        Some(user_id),
        "auth.step_up.success",
        "user",
        Some(user_id),
        json!({"scope": req.scope}),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(StepUpResponse { token, expires_at })
}

pub async fn consume(
    pool: &PgPool,
    session_id: i64,
    user_id: i64,
    headers: &HeaderMap,
    scope: &str,
) -> Result<(), ApiError> {
    let token = headers
        .get(HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() == 64)
        .ok_or_else(|| ApiError::forbidden("需要先完成身份再认证"))?;
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let consumed = repositories::step_up::consume(
        &mut transaction,
        &session_auth::token_hash(token),
        session_id,
        user_id,
        scope,
    )
    .await
    .map_err(db_error)?;
    if !consumed {
        return Err(ApiError::forbidden("再认证凭据已失效或已使用"));
    }
    repositories::audit_logs::record(
        &mut transaction,
        Some(user_id),
        "auth.step_up.consumed",
        "user",
        Some(user_id),
        json!({"scope": scope}),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(())
}
