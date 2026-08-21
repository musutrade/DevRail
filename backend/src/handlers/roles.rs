//! 角色 Handler：列表 / 详情 / 创建 / 更新 / 删除 / 权限分配

use crate::auth::RequirePermission;
use crate::error::ApiError;
use crate::models::{
    CreateRoleRequest, RolePermissions, RoleResponse, UpdateRolePermissionsRequest,
    UpdateRoleRequest,
};
use crate::permissions::{RolePermissionWrite, RoleRead, RoleWrite};
use crate::services;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;

pub async fn list(
    State(state): State<AppState>,
    _auth: RequirePermission<RoleRead>,
) -> Result<Json<Vec<RoleResponse>>, ApiError> {
    services::roles::list(&state.pool).await.map(Json)
}

pub async fn get(
    State(state): State<AppState>,
    _auth: RequirePermission<RoleRead>,
    Path(id): Path<i64>,
) -> Result<Json<RoleResponse>, ApiError> {
    services::roles::get(&state.pool, id).await.map(Json)
}

pub async fn create(
    State(state): State<AppState>,
    auth: RequirePermission<RoleWrite>,
    headers: HeaderMap,
    Json(req): Json<CreateRoleRequest>,
) -> Result<(StatusCode, Json<RoleResponse>), ApiError> {
    let needs_step_up = req
        .permission_ids
        .as_ref()
        .is_some_and(|permission_ids| !permission_ids.is_empty());
    if needs_step_up {
        auth.require("role:permissions:write")?;
        services::step_up::consume(
            &state.pool,
            auth.session_id,
            auth.user_id,
            &headers,
            services::step_up::ROLES_PERMISSIONS_SCOPE,
        )
        .await?;
    } else {
        services::module_unlock::require(
            &state.pool,
            auth.session_id,
            auth.user_id,
            services::module_unlock::ROLES_MODULE,
        )
        .await?;
    }
    services::roles::create(
        &state.pool,
        Some(auth.user_id),
        auth.data_scope,
        &req,
        auth.has("role:permissions:write"),
    )
    .await
    .map(|role| (StatusCode::CREATED, Json(role)))
}

pub async fn update(
    State(state): State<AppState>,
    auth: RequirePermission<RoleWrite>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateRoleRequest>,
) -> Result<Json<RoleResponse>, ApiError> {
    if req.data_scope.is_some() || req.is_active.is_some() {
        services::step_up::consume(
            &state.pool,
            auth.session_id,
            auth.user_id,
            &headers,
            services::step_up::ROLES_SENSITIVE_SCOPE,
        )
        .await?;
    } else {
        services::module_unlock::require(
            &state.pool,
            auth.session_id,
            auth.user_id,
            services::module_unlock::ROLES_MODULE,
        )
        .await?;
    }
    services::roles::update(&state.pool, Some(auth.user_id), auth.data_scope, id, &req)
        .await
        .map(Json)
}

pub async fn delete(
    State(state): State<AppState>,
    auth: RequirePermission<RoleWrite>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    services::step_up::consume(
        &state.pool,
        auth.session_id,
        auth.user_id,
        &headers,
        services::step_up::ROLES_DELETE_SCOPE,
    )
    .await?;
    services::roles::delete(&state.pool, Some(auth.user_id), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_permissions(
    State(state): State<AppState>,
    _auth: RequirePermission<RoleRead>,
    Path(id): Path<i64>,
) -> Result<Json<RolePermissions>, ApiError> {
    services::roles::get_permissions(&state.pool, id)
        .await
        .map(Json)
}

pub async fn put_permissions(
    State(state): State<AppState>,
    auth: RequirePermission<RolePermissionWrite>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateRolePermissionsRequest>,
) -> Result<StatusCode, ApiError> {
    services::step_up::consume(
        &state.pool,
        auth.session_id,
        auth.user_id,
        &headers,
        services::step_up::ROLES_PERMISSIONS_SCOPE,
    )
    .await?;
    services::roles::assign_permissions(&state.pool, Some(auth.user_id), id, &req).await?;
    Ok(StatusCode::NO_CONTENT)
}
