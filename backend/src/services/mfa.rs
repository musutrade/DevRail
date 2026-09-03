//! MFA service: policy enforcement, TOTP, recovery codes, and passkeys.

use crate::auth::{self as session_auth, AuthSessionConfig};
use crate::error::{db_error, ApiError};
use crate::mfa::MfaConfig;
use crate::models::{
    LoginResponse, LoginStatusSchema, MfaFactorRevokeRequest, MfaMethodSchema,
    MfaPasskeyRegistrationStartRequest, MfaPasskeyResponse, MfaStatusResponse,
    RecoveryCodesResponse, UserResponse,
};
use crate::repositories;
use crate::services::auth::{self as auth_service, LoginContext, LoginSessionOutcome};
use argon2::{Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use totp_rs::{Secret, Totp};
use uuid::Uuid;
use webauthn_rs::prelude::{PasskeyAuthentication, PasskeyRegistration, PublicKeyCredential};

const CHALLENGE_TTL_SECS: i64 = 300;
const RECOVERY_CODE_COUNT: usize = 10;

#[derive(Debug, Serialize, Deserialize)]
struct EnrollmentState {
    login: LoginContext,
    encrypted_secret: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PasskeyAuthenticationState {
    login: LoginContext,
    authentication: PasskeyAuthentication,
}

#[derive(Debug, Serialize, Deserialize)]
struct PasskeyRegistrationState {
    name: String,
    registration: PasskeyRegistration,
}

pub async fn begin_login(
    pool: &PgPool,
    mfa: &MfaConfig,
    user: &crate::models::UserRow,
    persistent: bool,
    login: LoginContext,
) -> Result<Option<LoginResponse>, ApiError> {
    let summary = repositories::mfa::summary(pool, user.id)
        .await
        .map_err(db_error)?;
    if !summary.required && !summary.totp_enabled && summary.passkey_count == 0 {
        return Ok(None);
    }

    let token = session_auth::random_token();
    let expires_at = Utc::now() + Duration::seconds(CHALLENGE_TTL_SECS);
    let mut transaction = pool.begin().await.map_err(db_error)?;
    repositories::mfa::prune_challenges(&mut transaction)
        .await
        .map_err(db_error)?;
    repositories::mfa::consume_login_challenges_for_user(&mut transaction, user.id)
        .await
        .map_err(db_error)?;
    if summary.required && !summary.totp_enabled {
        let secret = Secret::generate();
        let secret_bytes = secret.as_ref().to_vec();
        let encrypted = mfa
            .encrypt_totp_secret(user.id, &secret_bytes)
            .map_err(ApiError::internal)?;
        repositories::mfa::ensure_settings(&mut transaction, user.id, Uuid::new_v4())
            .await
            .map_err(db_error)?;
        repositories::mfa::create_challenge(
            &mut transaction,
            &session_auth::token_hash(&token),
            user.id,
            "totp_enrollment",
            persistent,
            &serde_json::to_value(EnrollmentState {
                login,
                encrypted_secret: STANDARD.encode(encrypted),
            })
            .map_err(ApiError::internal)?,
            expires_at,
        )
        .await
        .map_err(db_error)?;
        repositories::audit_logs::record(
            &mut transaction,
            Some(user.id),
            "auth.mfa.enrollment.started",
            "user",
            Some(user.id),
            json!({"factor": "totp"}),
        )
        .await
        .map_err(db_error)?;
        transaction.commit().await.map_err(db_error)?;
        let totp = mfa
            .totp(&user.username, secret_bytes)
            .map_err(ApiError::internal)?;
        return Ok(Some(LoginResponse {
            status: LoginStatusSchema::MfaEnrollmentRequired,
            expires_at: None,
            user: None,
            challenge_token: Some(token),
            methods: vec![MfaMethodSchema::Totp],
            totp_secret: Some(totp.secret().to_base32()),
            totp_uri: Some(totp.to_url().map_err(ApiError::internal)?),
            totp_qr_code: Some(totp.to_qr_base64().map_err(ApiError::internal)?),
            recovery_codes: Vec::new(),
        }));
    }

    let mut methods = Vec::new();
    if summary.totp_enabled {
        methods.push(MfaMethodSchema::Totp);
    }
    if summary.passkey_count > 0 {
        methods.push(MfaMethodSchema::Passkey);
    }
    if summary.recovery_codes_remaining > 0 {
        methods.push(MfaMethodSchema::RecoveryCode);
    }
    repositories::mfa::create_challenge(
        &mut transaction,
        &session_auth::token_hash(&token),
        user.id,
        "login",
        persistent,
        &serde_json::to_value(&login).map_err(ApiError::internal)?,
        expires_at,
    )
    .await
    .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut transaction,
        Some(user.id),
        "auth.mfa.challenge.issued",
        "user",
        Some(user.id),
        json!({"methods": methods.iter().map(|method| format!("{method:?}")).collect::<Vec<_>>() }),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(Some(LoginResponse {
        status: LoginStatusSchema::MfaRequired,
        expires_at: None,
        user: None,
        challenge_token: Some(token),
        methods,
        totp_secret: None,
        totp_uri: None,
        totp_qr_code: None,
        recovery_codes: Vec::new(),
    }))
}

pub async fn verify_totp_login(
    pool: &PgPool,
    config: &AuthSessionConfig,
    mfa: &MfaConfig,
    challenge_token: &str,
    code: &str,
) -> Result<LoginSessionOutcome, ApiError> {
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let challenge = repositories::mfa::challenge_for_update(
        &mut transaction,
        &session_auth::token_hash(challenge_token),
        "login",
    )
    .await
    .map_err(db_error)?
    .ok_or_else(ApiError::unauthorized)?;
    let login: LoginContext =
        serde_json::from_value(challenge.state.0.clone()).map_err(ApiError::internal)?;
    let encrypted = repositories::mfa::totp_secret_for_update(&mut transaction, challenge.user_id)
        .await
        .map_err(db_error)?
        .ok_or_else(ApiError::unauthorized)?;
    let secret = mfa
        .decrypt_totp_secret(challenge.user_id, &encrypted)
        .map_err(ApiError::internal)?;
    let user = repositories::users::find_by_id_in_connection(&mut transaction, challenge.user_id)
        .await
        .map_err(db_error)?
        .ok_or_else(ApiError::unauthorized)?;
    let valid = mfa
        .totp(&user.username, secret)
        .map_err(ApiError::internal)?
        .check_current(code.trim())
        .is_some();
    if !valid {
        let locked = repositories::mfa::record_challenge_failure(&mut transaction, challenge.id)
            .await
            .map_err(db_error)?;
        repositories::audit_logs::record(
            &mut transaction,
            Some(challenge.user_id),
            "auth.mfa.verify.failure",
            "user",
            Some(challenge.user_id),
            json!({"factor": "totp", "locked": locked}),
        )
        .await
        .map_err(db_error)?;
        transaction.commit().await.map_err(db_error)?;
        return Err(if locked {
            ApiError::rate_limited(CHALLENGE_TTL_SECS as u64)
        } else {
            ApiError::unauthorized()
        });
    }
    repositories::mfa::consume_challenge(&mut transaction, challenge.id)
        .await
        .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut transaction,
        Some(challenge.user_id),
        "auth.mfa.verify.success",
        "user",
        Some(challenge.user_id),
        json!({"factor": "totp"}),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    auth_service::create_login_session(
        pool,
        config,
        challenge.persistent,
        auth_service::LoginThrottleKeys {
            account: login.account,
            source_ip: login.source_ip,
            account_ip: login.account_ip,
        },
        user,
    )
    .await
}

pub async fn verify_totp(
    pool: &PgPool,
    config: &AuthSessionConfig,
    mfa: &MfaConfig,
    challenge_token: &str,
    code: &str,
) -> Result<LoginSessionOutcome, ApiError> {
    let kind = repositories::mfa::challenge_kind(pool, &session_auth::token_hash(challenge_token))
        .await
        .map_err(db_error)?
        .ok_or_else(ApiError::unauthorized)?;
    if kind == "totp_enrollment" {
        return verify_totp_enrollment(pool, config, mfa, challenge_token, code).await;
    }
    verify_totp_login(pool, config, mfa, challenge_token, code).await
}

async fn verify_totp_enrollment(
    pool: &PgPool,
    config: &AuthSessionConfig,
    mfa: &MfaConfig,
    challenge_token: &str,
    code: &str,
) -> Result<LoginSessionOutcome, ApiError> {
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let challenge = repositories::mfa::challenge_for_update(
        &mut transaction,
        &session_auth::token_hash(challenge_token),
        "totp_enrollment",
    )
    .await
    .map_err(db_error)?
    .ok_or_else(ApiError::unauthorized)?;
    let state: EnrollmentState =
        serde_json::from_value(challenge.state.0.clone()).map_err(ApiError::internal)?;
    let encrypted = STANDARD
        .decode(state.encrypted_secret)
        .map_err(|_| ApiError::unauthorized())?;
    let secret = mfa
        .decrypt_totp_secret(challenge.user_id, &encrypted)
        .map_err(ApiError::internal)?;
    let user = repositories::users::find_by_id_in_connection(&mut transaction, challenge.user_id)
        .await
        .map_err(db_error)?
        .ok_or_else(ApiError::unauthorized)?;
    let valid = mfa
        .totp(&user.username, secret)
        .map_err(ApiError::internal)?
        .check_current(code.trim())
        .is_some();
    if !valid {
        let locked = repositories::mfa::record_challenge_failure(&mut transaction, challenge.id)
            .await
            .map_err(db_error)?;
        repositories::audit_logs::record(
            &mut transaction,
            Some(challenge.user_id),
            "auth.mfa.verify.failure",
            "user",
            Some(challenge.user_id),
            json!({"factor": "totp_enrollment", "locked": locked}),
        )
        .await
        .map_err(db_error)?;
        transaction.commit().await.map_err(db_error)?;
        return Err(if locked {
            ApiError::rate_limited(CHALLENGE_TTL_SECS as u64)
        } else {
            ApiError::unauthorized()
        });
    }
    transaction.rollback().await.map_err(db_error)?;
    let recovery_codes = generate_recovery_codes();
    let hashes = hash_recovery_codes(&recovery_codes).await?;

    let mut transaction = pool.begin().await.map_err(db_error)?;
    let challenge = repositories::mfa::challenge_for_update(
        &mut transaction,
        &session_auth::token_hash(challenge_token),
        "totp_enrollment",
    )
    .await
    .map_err(db_error)?
    .ok_or_else(ApiError::unauthorized)?;
    let state: EnrollmentState =
        serde_json::from_value(challenge.state.0.clone()).map_err(ApiError::internal)?;
    repositories::mfa::enable_totp(&mut transaction, challenge.user_id, &encrypted)
        .await
        .map_err(db_error)?;
    repositories::mfa::replace_recovery_codes(&mut transaction, challenge.user_id, &hashes)
        .await
        .map_err(db_error)?;
    repositories::mfa::consume_challenge(&mut transaction, challenge.id)
        .await
        .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut transaction,
        Some(challenge.user_id),
        "auth.mfa.enrollment.completed",
        "user",
        Some(challenge.user_id),
        json!({"factor": "totp", "recoveryCodeCount": recovery_codes.len()}),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    let mut outcome = auth_service::create_login_session(
        pool,
        config,
        challenge.persistent,
        auth_service::LoginThrottleKeys {
            account: state.login.account,
            source_ip: state.login.source_ip,
            account_ip: state.login.account_ip,
        },
        user,
    )
    .await?;
    outcome.response.recovery_codes = recovery_codes;
    Ok(outcome)
}

pub async fn verify_recovery_login(
    pool: &PgPool,
    config: &AuthSessionConfig,
    challenge_token: &str,
    recovery_code: &str,
) -> Result<LoginSessionOutcome, ApiError> {
    let token_hash = session_auth::token_hash(challenge_token);
    let challenge = repositories::mfa::challenge(pool, &token_hash, "login")
        .await
        .map_err(db_error)?
        .ok_or_else(ApiError::unauthorized)?;
    let codes = repositories::mfa::recovery_codes(pool, challenge.user_id)
        .await
        .map_err(db_error)?;
    let matching_id = matching_recovery_code(recovery_code, codes).await?;

    let mut transaction = pool.begin().await.map_err(db_error)?;
    let challenge = repositories::mfa::challenge_for_update(&mut transaction, &token_hash, "login")
        .await
        .map_err(db_error)?
        .ok_or_else(ApiError::unauthorized)?;
    let consumed = if let Some(matching_id) = matching_id {
        repositories::mfa::consume_recovery_code(&mut transaction, challenge.user_id, matching_id)
            .await
            .map_err(db_error)?
    } else {
        false
    };
    if !consumed {
        let locked = repositories::mfa::record_challenge_failure(&mut transaction, challenge.id)
            .await
            .map_err(db_error)?;
        repositories::audit_logs::record(
            &mut transaction,
            Some(challenge.user_id),
            "auth.mfa.verify.failure",
            "user",
            Some(challenge.user_id),
            json!({"factor": "recovery_code", "locked": locked}),
        )
        .await
        .map_err(db_error)?;
        transaction.commit().await.map_err(db_error)?;
        return Err(if locked {
            ApiError::rate_limited(CHALLENGE_TTL_SECS as u64)
        } else {
            ApiError::unauthorized()
        });
    }
    let login: LoginContext =
        serde_json::from_value(challenge.state.0.clone()).map_err(ApiError::internal)?;
    repositories::mfa::consume_challenge(&mut transaction, challenge.id)
        .await
        .map_err(db_error)?;
    let revoked =
        repositories::auth_sessions::revoke_all_for_user(&mut transaction, challenge.user_id)
            .await
            .map_err(db_error)?;
    auth_service::record_session_revocation(
        &mut transaction,
        Some(challenge.user_id),
        challenge.user_id,
        "mfa_recovery_code",
        revoked,
    )
    .await?;
    repositories::audit_logs::record(
        &mut transaction,
        Some(challenge.user_id),
        "auth.mfa.recovery_code.used",
        "user",
        Some(challenge.user_id),
        json!({"revokedSessionCount": revoked}),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    let user = repositories::users::find_by_id(pool, challenge.user_id)
        .await
        .map_err(db_error)?
        .ok_or_else(ApiError::unauthorized)?;
    auth_service::create_login_session(
        pool,
        config,
        challenge.persistent,
        auth_service::LoginThrottleKeys {
            account: login.account,
            source_ip: login.source_ip,
            account_ip: login.account_ip,
        },
        user,
    )
    .await
}

pub async fn status(pool: &PgPool, user_id: i64) -> Result<MfaStatusResponse, ApiError> {
    let summary = repositories::mfa::summary(pool, user_id)
        .await
        .map_err(db_error)?;
    let passkeys = repositories::mfa::list_passkeys(pool, user_id)
        .await
        .map_err(db_error)?
        .into_iter()
        .map(|key| MfaPasskeyResponse {
            id: key.id,
            name: key.name,
            created_at: key.created_at,
            last_used_at: key.last_used_at,
        })
        .collect();
    Ok(MfaStatusResponse {
        required: summary.required,
        totp_enabled: summary.totp_enabled,
        recovery_codes_remaining: summary.recovery_codes_remaining,
        passkeys,
    })
}

pub async fn start_passkey_authentication(
    pool: &PgPool,
    mfa: &MfaConfig,
    challenge_token: &str,
) -> Result<crate::models::MfaWebauthnChallengeResponse, ApiError> {
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let challenge = repositories::mfa::challenge_for_update(
        &mut transaction,
        &session_auth::token_hash(challenge_token),
        "login",
    )
    .await
    .map_err(db_error)?
    .ok_or_else(ApiError::unauthorized)?;
    let login: LoginContext =
        serde_json::from_value(challenge.state.0.clone()).map_err(ApiError::internal)?;
    let passkeys =
        repositories::mfa::list_passkeys_in_connection(&mut transaction, challenge.user_id)
            .await
            .map_err(db_error)?;
    let credentials = passkeys
        .iter()
        .map(|key| key.credential.0.clone())
        .collect::<Vec<_>>();
    let (public_key, authentication) = mfa
        .webauthn()
        .start_passkey_authentication(&credentials)
        .map_err(|error| {
            ApiError::internal(format!("failed to start passkey authentication: {error}"))
        })?;
    let token = session_auth::random_token();
    repositories::mfa::consume_challenge(&mut transaction, challenge.id)
        .await
        .map_err(db_error)?;
    repositories::mfa::create_challenge(
        &mut transaction,
        &session_auth::token_hash(&token),
        challenge.user_id,
        "passkey_authentication",
        challenge.persistent,
        &serde_json::to_value(PasskeyAuthenticationState {
            login,
            authentication,
        })
        .map_err(ApiError::internal)?,
        Utc::now() + Duration::seconds(CHALLENGE_TTL_SECS),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(crate::models::MfaWebauthnChallengeResponse {
        challenge_token: token,
        public_key: serde_json::to_value(public_key).map_err(ApiError::internal)?,
    })
}

pub async fn finish_passkey_authentication(
    pool: &PgPool,
    config: &AuthSessionConfig,
    mfa: &MfaConfig,
    challenge_token: &str,
    credential: Value,
) -> Result<LoginSessionOutcome, ApiError> {
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let challenge = repositories::mfa::challenge_for_update(
        &mut transaction,
        &session_auth::token_hash(challenge_token),
        "passkey_authentication",
    )
    .await
    .map_err(db_error)?
    .ok_or_else(ApiError::unauthorized)?;
    let state: PasskeyAuthenticationState =
        serde_json::from_value(challenge.state.0.clone()).map_err(ApiError::internal)?;
    let passkeys =
        repositories::mfa::list_passkeys_in_connection(&mut transaction, challenge.user_id)
            .await
            .map_err(db_error)?;
    let mut credentials = passkeys
        .iter()
        .map(|key| key.credential.0.clone())
        .collect::<Vec<_>>();
    let result = serde_json::from_value::<PublicKeyCredential>(credential)
        .ok()
        .and_then(|credential| {
            mfa.webauthn()
                .finish_passkey_authentication(&credential, &state.authentication)
                .ok()
        });
    let Some(result) = result else {
        let locked = repositories::mfa::record_challenge_failure(&mut transaction, challenge.id)
            .await
            .map_err(db_error)?;
        repositories::audit_logs::record(
            &mut transaction,
            Some(challenge.user_id),
            "auth.mfa.verify.failure",
            "user",
            Some(challenge.user_id),
            json!({"factor": "passkey", "locked": locked}),
        )
        .await
        .map_err(db_error)?;
        transaction.commit().await.map_err(db_error)?;
        return Err(if locked {
            ApiError::rate_limited(CHALLENGE_TTL_SECS as u64)
        } else {
            ApiError::unauthorized()
        });
    };
    let credential_id = encode_credential_id(result.cred_id());
    if let Some((index, row)) = passkeys
        .iter()
        .enumerate()
        .find(|(_, row)| encode_credential_id(row.credential.0.cred_id()) == credential_id)
    {
        if result.needs_update() {
            credentials[index].update_credential(&result);
        }
        repositories::mfa::update_passkey_after_use(&mut transaction, row.id, &credentials[index])
            .await
            .map_err(db_error)?;
    } else {
        return Err(ApiError::unauthorized());
    }
    repositories::mfa::consume_challenge(&mut transaction, challenge.id)
        .await
        .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut transaction,
        Some(challenge.user_id),
        "auth.mfa.verify.success",
        "user",
        Some(challenge.user_id),
        json!({"factor": "passkey"}),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    let user = repositories::users::find_by_id(pool, challenge.user_id)
        .await
        .map_err(db_error)?
        .ok_or_else(ApiError::unauthorized)?;
    auth_service::create_login_session(
        pool,
        config,
        challenge.persistent,
        auth_service::LoginThrottleKeys {
            account: state.login.account,
            source_ip: state.login.source_ip,
            account_ip: state.login.account_ip,
        },
        user,
    )
    .await
}

pub async fn start_passkey_registration(
    pool: &PgPool,
    mfa: &MfaConfig,
    user: &UserResponse,
    req: &MfaPasskeyRegistrationStartRequest,
) -> Result<crate::models::MfaWebauthnChallengeResponse, ApiError> {
    let name = req.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(ApiError::validation("通行密钥名称长度必须为 1-80 个字符"));
    }
    verify_password_and_totp(pool, mfa, user.id, &req.current_password, &req.totp_code).await?;
    let existing = repositories::mfa::list_passkeys(pool, user.id)
        .await
        .map_err(db_error)?;
    let exclude = existing
        .iter()
        .map(|key| key.credential.0.cred_id().clone())
        .collect::<Vec<_>>();
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let webauthn_user_id =
        repositories::mfa::ensure_settings(&mut transaction, user.id, Uuid::new_v4())
            .await
            .map_err(db_error)?;
    let (public_key, registration) = mfa
        .webauthn()
        .start_passkey_registration(
            webauthn_user_id,
            &user.username,
            &user.display_name,
            Some(exclude),
        )
        .map_err(|error| {
            ApiError::internal(format!("failed to start passkey registration: {error}"))
        })?;
    let token = session_auth::random_token();
    repositories::mfa::create_challenge(
        &mut transaction,
        &session_auth::token_hash(&token),
        user.id,
        "passkey_registration",
        false,
        &serde_json::to_value(PasskeyRegistrationState {
            name: name.to_string(),
            registration,
        })
        .map_err(ApiError::internal)?,
        Utc::now() + Duration::seconds(CHALLENGE_TTL_SECS),
    )
    .await
    .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut transaction,
        Some(user.id),
        "auth.mfa.enrollment.started",
        "user",
        Some(user.id),
        json!({"factor": "passkey"}),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(crate::models::MfaWebauthnChallengeResponse {
        challenge_token: token,
        public_key: serde_json::to_value(public_key).map_err(ApiError::internal)?,
    })
}

pub async fn finish_passkey_registration(
    pool: &PgPool,
    mfa: &MfaConfig,
    user_id: i64,
    challenge_token: &str,
    credential: Value,
) -> Result<MfaStatusResponse, ApiError> {
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let challenge = repositories::mfa::challenge_for_update(
        &mut transaction,
        &session_auth::token_hash(challenge_token),
        "passkey_registration",
    )
    .await
    .map_err(db_error)?
    .filter(|challenge| challenge.user_id == user_id)
    .ok_or_else(ApiError::unauthorized)?;
    let state: PasskeyRegistrationState =
        serde_json::from_value(challenge.state.0.clone()).map_err(ApiError::internal)?;
    let credential = serde_json::from_value(credential).map_err(ApiError::internal)?;
    let passkey = mfa
        .webauthn()
        .finish_passkey_registration(&credential, &state.registration)
        .map_err(|_| ApiError::validation("通行密钥注册失败，请重试"))?;
    let credential_id = encode_credential_id(passkey.cred_id());
    repositories::mfa::create_passkey(
        &mut transaction,
        user_id,
        &state.name,
        &credential_id,
        &passkey,
    )
    .await
    .map_err(db_error)?;
    repositories::mfa::consume_challenge(&mut transaction, challenge.id)
        .await
        .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut transaction,
        Some(user_id),
        "auth.mfa.enrollment.completed",
        "user",
        Some(user_id),
        json!({"factor": "passkey"}),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    status(pool, user_id).await
}

pub async fn revoke_passkey(
    pool: &PgPool,
    mfa: &MfaConfig,
    user_id: i64,
    passkey_id: i64,
    req: &MfaFactorRevokeRequest,
) -> Result<(), ApiError> {
    verify_password_and_totp(pool, mfa, user_id, &req.current_password, &req.totp_code).await?;
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let deleted = repositories::mfa::delete_passkey(&mut transaction, user_id, passkey_id)
        .await
        .map_err(db_error)?;
    if !deleted {
        return Err(ApiError::not_found("通行密钥不存在"));
    }
    let revoked = repositories::auth_sessions::revoke_all_for_user(&mut transaction, user_id)
        .await
        .map_err(db_error)?;
    auth_service::record_session_revocation(
        &mut transaction,
        Some(user_id),
        user_id,
        "mfa_factor_revoked",
        revoked,
    )
    .await?;
    repositories::audit_logs::record(
        &mut transaction,
        Some(user_id),
        "auth.mfa.factor.revoked",
        "user",
        Some(user_id),
        json!({"factor": "passkey", "revokedSessionCount": revoked}),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(())
}

pub fn encode_credential_id(id: &webauthn_rs::prelude::CredentialID) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(id.as_ref())
}

fn generate_recovery_codes() -> Vec<String> {
    (0..RECOVERY_CODE_COUNT)
        .map(|_| {
            let raw = session_auth::random_token()[..20].to_ascii_uppercase();
            raw.as_bytes()
                .chunks(4)
                .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
                .collect::<Vec<_>>()
                .join("-")
        })
        .collect()
}

fn hash_recovery_code(code: &str) -> Result<String, ApiError> {
    // Codes have about 120 bits of generated entropy; this bounded profile keeps
    // ten salted hashes responsive without treating them as low-entropy passwords.
    let params = Params::new(4 * 1024, 1, 1, None)
        .map_err(|error| ApiError::internal(format!("invalid recovery hash profile: {error}")))?;
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password(code.as_bytes(), &session_auth::password_salt())
        .map(|hash| hash.to_string())
        .map_err(|error| ApiError::internal(format!("failed to hash recovery code: {error}")))
}

async fn hash_recovery_codes(codes: &[String]) -> Result<Vec<String>, ApiError> {
    let codes = codes.to_vec();
    auth_service::run_argon2_task(move || {
        codes.iter().map(|code| hash_recovery_code(code)).collect()
    })
    .await
}

async fn matching_recovery_code(
    recovery_code: &str,
    codes: Vec<repositories::mfa::RecoveryCodeRow>,
) -> Result<Option<i64>, ApiError> {
    let normalized = recovery_code.trim().to_ascii_uppercase();
    auth_service::run_argon2_task(move || {
        for code in codes {
            let parsed = PasswordHash::new(&code.code_hash).map_err(ApiError::internal)?;
            if Argon2::default()
                .verify_password(normalized.as_bytes(), &parsed)
                .is_ok()
            {
                return Ok(Some(code.id));
            }
        }
        Ok(None)
    })
    .await
}

pub(crate) async fn verify_totp_code(
    pool: &PgPool,
    mfa: &MfaConfig,
    user_id: i64,
    code: &str,
) -> Result<(), ApiError> {
    let user = repositories::users::find_by_id(pool, user_id)
        .await
        .map_err(db_error)?
        .ok_or_else(ApiError::unauthorized)?;
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let encrypted = repositories::mfa::totp_secret_for_update(&mut transaction, user_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::forbidden("该账号尚未完成 TOTP 注册"))?;
    let secret = mfa
        .decrypt_totp_secret(user_id, &encrypted)
        .map_err(ApiError::internal)?;
    let totp = mfa
        .totp(&user.username, secret)
        .map_err(ApiError::internal)?;
    let Some(counter) = matching_totp_counter(&totp, code)? else {
        return Err(ApiError::validation("身份验证器验证码不正确"));
    };
    let consumed =
        repositories::mfa::consume_reauth_totp_counter(&mut transaction, user_id, counter)
            .await
            .map_err(db_error)?;
    if !consumed {
        return Err(ApiError::validation(
            "验证码已使用或已过期，请输入最新验证码",
        ));
    }
    transaction.commit().await.map_err(db_error)?;
    Ok(())
}

fn matching_totp_counter(totp: &Totp, code: &str) -> Result<Option<i64>, ApiError> {
    totp.check_current(code.trim())
        .map(|counter| i64::try_from(counter).map_err(ApiError::internal))
        .transpose()
}

pub(crate) async fn verify_reauthentication(
    pool: &PgPool,
    mfa: &MfaConfig,
    user_id: i64,
    password: &str,
    code: Option<&str>,
) -> Result<(), ApiError> {
    auth_service::verify_current_password(pool, user_id, password).await?;
    let summary = repositories::mfa::summary(pool, user_id)
        .await
        .map_err(db_error)?;
    if summary.totp_enabled || summary.required {
        let code = code
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| ApiError::forbidden("该操作需要身份验证器验证码"))?;
        verify_totp_code(pool, mfa, user_id, code).await?;
    }
    Ok(())
}

pub(crate) async fn verify_password_and_totp(
    pool: &PgPool,
    mfa: &MfaConfig,
    user_id: i64,
    password: &str,
    code: &str,
) -> Result<(), ApiError> {
    auth_service::verify_current_password(pool, user_id, password).await?;
    verify_totp_code(pool, mfa, user_id, code).await
}

pub async fn regenerate_recovery_codes(
    pool: &PgPool,
    mfa: &MfaConfig,
    user_id: i64,
    req: &MfaFactorRevokeRequest,
) -> Result<RecoveryCodesResponse, ApiError> {
    verify_password_and_totp(pool, mfa, user_id, &req.current_password, &req.totp_code).await?;
    let codes = generate_recovery_codes();
    let hashes = hash_recovery_codes(&codes).await?;
    let mut transaction = pool.begin().await.map_err(db_error)?;
    repositories::mfa::replace_recovery_codes(&mut transaction, user_id, &hashes)
        .await
        .map_err(db_error)?;
    let revoked = repositories::auth_sessions::revoke_all_for_user(&mut transaction, user_id)
        .await
        .map_err(db_error)?;
    auth_service::record_session_revocation(
        &mut transaction,
        Some(user_id),
        user_id,
        "mfa_recovery_regenerated",
        revoked,
    )
    .await?;
    repositories::audit_logs::record(
        &mut transaction,
        Some(user_id),
        "auth.mfa.recovery_codes.generated",
        "user",
        Some(user_id),
        json!({"count": codes.len()}),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(RecoveryCodesResponse { codes })
}
