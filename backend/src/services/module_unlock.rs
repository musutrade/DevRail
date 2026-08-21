//! Session-bound, short-lived authorization for routine writes inside protected modules.

use crate::error::{db_error, ApiError};
use crate::mfa::MfaConfig;
use crate::models::{ModuleUnlockRequest, ModuleUnlockStatusResponse};
use crate::repositories;
use crate::services::mfa;
use chrono::{Duration, Utc};
use serde_json::json;
use sqlx::PgPool;

pub const USERS_MODULE: &str = "users";
pub const ROLES_MODULE: &str = "roles";
const UNLOCK_TTL_SECS: i64 = 300;
const VALID_MODULES: &[&str] = &[USERS_MODULE, ROLES_MODULE];

pub async fn status(
    pool: &PgPool,
    session_id: i64,
    user_id: i64,
    module: &str,
) -> Result<ModuleUnlockStatusResponse, ApiError> {
    validate_module(module)?;
    let expires_at =
        repositories::module_unlock::active_expires_at(pool, session_id, user_id, module)
            .await
            .map_err(db_error)?;
    Ok(ModuleUnlockStatusResponse {
        module: module.to_string(),
        unlocked: expires_at.is_some(),
        expires_at,
    })
}

pub async fn issue(
    pool: &PgPool,
    mfa_config: &MfaConfig,
    session_id: i64,
    user_id: i64,
    req: &ModuleUnlockRequest,
) -> Result<ModuleUnlockStatusResponse, ApiError> {
    validate_module(&req.module)?;
    mfa::verify_reauthentication(
        pool,
        mfa_config,
        user_id,
        &req.current_password,
        req.totp_code.as_deref(),
    )
    .await?;

    let expires_at = Utc::now() + Duration::seconds(UNLOCK_TTL_SECS);
    let mut transaction = pool.begin().await.map_err(db_error)?;
    repositories::module_unlock::prune(&mut transaction)
        .await
        .map_err(db_error)?;
    repositories::module_unlock::upsert(
        &mut transaction,
        session_id,
        user_id,
        &req.module,
        expires_at,
    )
    .await
    .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut transaction,
        Some(user_id),
        "auth.module_unlock.issued",
        "user",
        Some(user_id),
        json!({"module": req.module, "expiresAt": expires_at}),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;

    Ok(ModuleUnlockStatusResponse {
        module: req.module.clone(),
        unlocked: true,
        expires_at: Some(expires_at),
    })
}

pub async fn require(
    pool: &PgPool,
    session_id: i64,
    user_id: i64,
    module: &str,
) -> Result<(), ApiError> {
    validate_module(module)?;
    let unlocked =
        repositories::module_unlock::active_expires_at(pool, session_id, user_id, module)
            .await
            .map_err(db_error)?
            .is_some();
    if unlocked {
        Ok(())
    } else {
        Err(ApiError::forbidden("当前模块需要先完成身份验证"))
    }
}

fn validate_module(module: &str) -> Result<(), ApiError> {
    if VALID_MODULES.contains(&module) {
        Ok(())
    } else {
        Err(ApiError::validation("无效的模块解锁范围"))
    }
}
