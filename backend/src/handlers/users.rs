//! 用户 Handler：列表 / 详情 / 创建 / 更新 / 软删除 / 分配角色

use crate::auth::RequirePermission;
use crate::error::ApiError;
use crate::models::{
    AssignRolesRequest, BatchAssignRolesRequest, BatchUserIdsRequest, CreateUserRequest, PageQuery,
    PageUser, UpdateUserRequest, UserResponse,
};
use crate::permissions::departments::READ_PERMISSION_CODE as DEPARTMENT_READ_PERMISSION_CODE;
use crate::permissions::{UserDeactivate, UserRead, UserRoleWrite, UserWrite};
use crate::services;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;

pub async fn list(
    State(state): State<AppState>,
    auth: RequirePermission<UserRead>,
    Query(query): Query<PageQuery>,
) -> Result<Json<PageUser>, ApiError> {
    services::users::list(&state.pool, &auth, &query)
        .await
        .map(Json)
}

pub async fn get(
    State(state): State<AppState>,
    auth: RequirePermission<UserRead>,
    Path(id): Path<i64>,
) -> Result<Json<UserResponse>, ApiError> {
    services::users::get(&state.pool, &auth, id).await.map(Json)
}

pub async fn create(
    State(state): State<AppState>,
    auth: RequirePermission<UserWrite>,
    headers: HeaderMap,
    Json(req): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserResponse>), ApiError> {
    if req
        .role_ids
        .as_ref()
        .is_some_and(|role_ids| !role_ids.is_empty())
    {
        auth.require("user:roles:write")?;
    }
    if req
        .status
        .as_deref()
        .is_some_and(|status| status != "active")
    {
        auth.require("user:admin:deactivate")?;
    }
    if req.department_id.is_some() {
        auth.require(DEPARTMENT_READ_PERMISSION_CODE)?;
    }
    let needs_step_up = req.role_ids.as_ref().is_some_and(|ids| !ids.is_empty())
        || req
            .status
            .as_deref()
            .is_some_and(|status| status != "active")
        || req
            .department_id
            .is_some_and(|department_id| auth.department_id != Some(department_id));
    if needs_step_up {
        services::step_up::consume(
            &state.pool,
            auth.session_id,
            auth.user_id,
            &headers,
            services::step_up::USERS_SENSITIVE_SCOPE,
        )
        .await?;
    } else {
        services::module_unlock::require(
            &state.pool,
            auth.session_id,
            auth.user_id,
            services::module_unlock::USERS_MODULE,
        )
        .await?;
    }
    services::users::create(
        &state.pool,
        &auth,
        &req,
        auth.has("user:super_admin:grant"),
        auth.has(DEPARTMENT_READ_PERMISSION_CODE),
    )
    .await
    .map(|user| (StatusCode::CREATED, Json(user)))
}

pub async fn update(
    State(state): State<AppState>,
    auth: RequirePermission<UserWrite>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUserRequest>,
) -> Result<Json<UserResponse>, ApiError> {
    if req.password.is_some() {
        auth.require("user:admin:reset_password")?;
    }
    if req.status.is_some() {
        auth.require("user:admin:deactivate")?;
    }
    if req.department_id.is_some() {
        auth.require(DEPARTMENT_READ_PERMISSION_CODE)?;
    }
    if req.password.is_some() || req.status.is_some() || req.department_id.is_some() {
        services::step_up::consume(
            &state.pool,
            auth.session_id,
            auth.user_id,
            &headers,
            services::step_up::USERS_SENSITIVE_SCOPE,
        )
        .await?;
    } else {
        services::module_unlock::require(
            &state.pool,
            auth.session_id,
            auth.user_id,
            services::module_unlock::USERS_MODULE,
        )
        .await?;
    }
    services::users::update(
        &state.pool,
        id,
        Some(&auth),
        &req,
        auth.has("user:super_admin:grant"),
        auth.has(DEPARTMENT_READ_PERMISSION_CODE),
    )
    .await
    .map(Json)
}

pub async fn delete(
    State(state): State<AppState>,
    auth: RequirePermission<UserDeactivate>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    services::step_up::consume(
        &state.pool,
        auth.session_id,
        auth.user_id,
        &headers,
        services::step_up::USERS_DELETE_SCOPE,
    )
    .await?;
    services::users::delete(&state.pool, id, Some(&auth)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn batch_delete(
    State(state): State<AppState>,
    auth: RequirePermission<UserDeactivate>,
    headers: HeaderMap,
    Json(req): Json<BatchUserIdsRequest>,
) -> Result<StatusCode, ApiError> {
    services::step_up::consume(
        &state.pool,
        auth.session_id,
        auth.user_id,
        &headers,
        services::step_up::USERS_DELETE_SCOPE,
    )
    .await?;
    services::users::delete_many(&state.pool, Some(&auth), &req).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn assign_roles(
    State(state): State<AppState>,
    auth: RequirePermission<UserRoleWrite>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<AssignRolesRequest>,
) -> Result<StatusCode, ApiError> {
    services::step_up::consume(
        &state.pool,
        auth.session_id,
        auth.user_id,
        &headers,
        services::step_up::USERS_ROLES_SCOPE,
    )
    .await?;
    services::users::assign_roles(
        &state.pool,
        Some(&auth),
        id,
        &req,
        auth.has("user:super_admin:grant"),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn batch_assign_roles(
    State(state): State<AppState>,
    auth: RequirePermission<UserRoleWrite>,
    headers: HeaderMap,
    Json(req): Json<BatchAssignRolesRequest>,
) -> Result<StatusCode, ApiError> {
    services::step_up::consume(
        &state.pool,
        auth.session_id,
        auth.user_id,
        &headers,
        services::step_up::USERS_ROLES_SCOPE,
    )
    .await?;
    services::users::assign_roles_many(
        &state.pool,
        Some(&auth),
        &req,
        auth.has("user:super_admin:grant"),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
