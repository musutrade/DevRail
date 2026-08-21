//! 认证 Handler：登录 / 当前用户 / 权限码

use crate::access::ActorContext;
use crate::auth;
use crate::error::ApiError;
use crate::models::{
    ChangePasswordRequest, LoginRequest, LoginResponse, MfaCodeRequest, MfaFactorRevokeRequest,
    MfaPasskeyAuthenticationFinishRequest, MfaPasskeyAuthenticationStartRequest,
    MfaPasskeyRegistrationFinishRequest, MfaPasskeyRegistrationStartRequest, ModuleUnlockRequest,
    ModuleUnlockStatusResponse, PermissionCodes, RecoveryCodesResponse, StepUpRequest,
    StepUpResponse, UserResponse,
};
use crate::services;
use crate::AppState;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::header::{CACHE_CONTROL, PRAGMA};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use std::net::SocketAddr;

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let client_ip = auth::resolve_client_ip(peer.ip(), &headers, &state.auth.trusted_proxy_cidrs);
    let outcome =
        services::auth::login(&state.pool, &state.auth, &state.mfa, &req, client_ip).await?;
    let no_store = [
        (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        (PRAGMA, HeaderValue::from_static("no-cache")),
    ];
    match outcome {
        services::auth::LoginOutcome::Authenticated(outcome) => {
            let jar = auth::set_session_cookies(
                jar,
                outcome.session_token,
                outcome.csrf_token,
                outcome.persistent,
                outcome.ttl_secs,
                state.auth.secure_cookies,
            );
            Ok((jar, no_store, Json(outcome.response)))
        }
        services::auth::LoginOutcome::MfaRequired(response) => Ok((jar, no_store, Json(response))),
    }
}

pub async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
    auth_user: ActorContext,
) -> Result<impl IntoResponse, ApiError> {
    services::auth::logout(&state.pool, auth_user.user_id, auth_user.session_id).await?;
    Ok((
        auth::clear_session_cookies(jar, state.auth.secure_cookies),
        StatusCode::NO_CONTENT,
    ))
}

pub async fn me(
    State(state): State<AppState>,
    auth: ActorContext,
) -> Result<Json<UserResponse>, ApiError> {
    services::auth::me(&state.pool, auth.user_id)
        .await
        .map(Json)
}

pub async fn change_password(
    State(state): State<AppState>,
    jar: CookieJar,
    auth: ActorContext,
    headers: HeaderMap,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<impl IntoResponse, ApiError> {
    services::step_up::consume(
        &state.pool,
        auth.session_id,
        auth.user_id,
        &headers,
        services::step_up::PASSWORD_CHANGE_SCOPE,
    )
    .await?;
    services::auth::change_password(&state.pool, auth.user_id, &req).await?;
    Ok((
        auth::clear_session_cookies(jar, state.auth.secure_cookies),
        StatusCode::NO_CONTENT,
    ))
}

pub async fn step_up(
    State(state): State<AppState>,
    auth: ActorContext,
    Json(req): Json<StepUpRequest>,
) -> Result<Json<StepUpResponse>, ApiError> {
    services::step_up::issue(&state.pool, &state.mfa, auth.session_id, auth.user_id, &req)
        .await
        .map(Json)
}

pub async fn module_unlock_status(
    State(state): State<AppState>,
    auth: ActorContext,
    Path(module): Path<String>,
) -> Result<Json<ModuleUnlockStatusResponse>, ApiError> {
    services::module_unlock::status(&state.pool, auth.session_id, auth.user_id, &module)
        .await
        .map(Json)
}

pub async fn module_unlock(
    State(state): State<AppState>,
    auth: ActorContext,
    Json(req): Json<ModuleUnlockRequest>,
) -> Result<Json<ModuleUnlockStatusResponse>, ApiError> {
    services::module_unlock::issue(&state.pool, &state.mfa, auth.session_id, auth.user_id, &req)
        .await
        .map(Json)
}

pub async fn me_permissions(auth: ActorContext) -> Result<Json<PermissionCodes>, ApiError> {
    Ok(Json(PermissionCodes {
        codes: auth.permission_codes.into_iter().collect(),
    }))
}

pub async fn verify_totp(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<MfaCodeRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let outcome = services::mfa::verify_totp(
        &state.pool,
        &state.auth,
        &state.mfa,
        &req.challenge_token,
        &req.code,
    )
    .await?;
    Ok(set_login_response(jar, state, outcome))
}

pub async fn verify_recovery_code(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<MfaCodeRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let outcome = services::mfa::verify_recovery_login(
        &state.pool,
        &state.auth,
        &req.challenge_token,
        &req.code,
    )
    .await?;
    Ok(set_login_response(jar, state, outcome))
}

pub async fn start_passkey_authentication(
    State(state): State<AppState>,
    Json(req): Json<MfaPasskeyAuthenticationStartRequest>,
) -> Result<Json<crate::models::MfaWebauthnChallengeResponse>, ApiError> {
    services::mfa::start_passkey_authentication(&state.pool, &state.mfa, &req.challenge_token)
        .await
        .map(Json)
}

pub async fn finish_passkey_authentication(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<MfaPasskeyAuthenticationFinishRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let outcome = services::mfa::finish_passkey_authentication(
        &state.pool,
        &state.auth,
        &state.mfa,
        &req.challenge_token,
        req.credential,
    )
    .await?;
    Ok(set_login_response(jar, state, outcome))
}

pub async fn mfa_status(
    State(state): State<AppState>,
    auth: ActorContext,
) -> Result<Json<crate::models::MfaStatusResponse>, ApiError> {
    services::mfa::status(&state.pool, auth.user_id)
        .await
        .map(Json)
}

pub async fn start_passkey_registration(
    State(state): State<AppState>,
    auth: ActorContext,
    Json(req): Json<MfaPasskeyRegistrationStartRequest>,
) -> Result<Json<crate::models::MfaWebauthnChallengeResponse>, ApiError> {
    let user = services::auth::me(&state.pool, auth.user_id).await?;
    services::mfa::start_passkey_registration(&state.pool, &state.mfa, &user, &req)
        .await
        .map(Json)
}

pub async fn finish_passkey_registration(
    State(state): State<AppState>,
    auth: ActorContext,
    Json(req): Json<MfaPasskeyRegistrationFinishRequest>,
) -> Result<Json<crate::models::MfaStatusResponse>, ApiError> {
    services::mfa::finish_passkey_registration(
        &state.pool,
        &state.mfa,
        auth.user_id,
        &req.challenge_token,
        req.credential,
    )
    .await
    .map(Json)
}

pub async fn revoke_passkey(
    State(state): State<AppState>,
    jar: CookieJar,
    auth: ActorContext,
    axum::extract::Path(passkey_id): axum::extract::Path<i64>,
    Json(req): Json<MfaFactorRevokeRequest>,
) -> Result<impl IntoResponse, ApiError> {
    services::mfa::revoke_passkey(&state.pool, &state.mfa, auth.user_id, passkey_id, &req).await?;
    Ok((
        auth::clear_session_cookies(jar, state.auth.secure_cookies),
        StatusCode::NO_CONTENT,
    ))
}

pub async fn regenerate_recovery_codes(
    State(state): State<AppState>,
    jar: CookieJar,
    auth: ActorContext,
    Json(req): Json<MfaFactorRevokeRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let response =
        services::mfa::regenerate_recovery_codes(&state.pool, &state.mfa, auth.user_id, &req)
            .await?;
    let no_store = [
        (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        (PRAGMA, HeaderValue::from_static("no-cache")),
    ];
    Ok((
        auth::clear_session_cookies(jar, state.auth.secure_cookies),
        no_store,
        Json::<RecoveryCodesResponse>(response),
    ))
}

fn set_login_response(
    jar: CookieJar,
    state: AppState,
    outcome: crate::services::auth::LoginSessionOutcome,
) -> (
    CookieJar,
    [(axum::http::HeaderName, HeaderValue); 2],
    Json<LoginResponse>,
) {
    let jar = auth::set_session_cookies(
        jar,
        outcome.session_token,
        outcome.csrf_token,
        outcome.persistent,
        outcome.ttl_secs,
        state.auth.secure_cookies,
    );
    let no_store = [
        (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        (PRAGMA, HeaderValue::from_static("no-cache")),
    ];
    (jar, no_store, Json(outcome.response))
}
