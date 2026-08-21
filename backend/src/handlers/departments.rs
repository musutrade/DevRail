//! 部门 Handler：层级列表、详情、创建、更新和删除。

use crate::auth::RequirePermission;
use crate::error::ApiError;
use crate::models::{CreateDepartmentRequest, DepartmentResponse, UpdateDepartmentRequest};
use crate::permissions::departments::{DepartmentRead, DepartmentWrite};
use crate::services;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;

pub async fn list(
    State(state): State<AppState>,
    auth: RequirePermission<DepartmentRead>,
) -> Result<Json<Vec<DepartmentResponse>>, ApiError> {
    services::departments::list(&state.pool, &auth)
        .await
        .map(Json)
}

pub async fn get(
    State(state): State<AppState>,
    auth: RequirePermission<DepartmentRead>,
    Path(id): Path<i64>,
) -> Result<Json<DepartmentResponse>, ApiError> {
    services::departments::get(&state.pool, &auth, id)
        .await
        .map(Json)
}

pub async fn create(
    State(state): State<AppState>,
    auth: RequirePermission<DepartmentWrite>,
    headers: HeaderMap,
    Json(req): Json<CreateDepartmentRequest>,
) -> Result<(StatusCode, Json<DepartmentResponse>), ApiError> {
    services::step_up::consume(
        &state.pool,
        auth.session_id,
        auth.user_id,
        &headers,
        services::step_up::DEPARTMENTS_WRITE_SCOPE,
    )
    .await?;
    services::departments::create(&state.pool, &auth, &req)
        .await
        .map(|department| (StatusCode::CREATED, Json(department)))
}

pub async fn update(
    State(state): State<AppState>,
    auth: RequirePermission<DepartmentWrite>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateDepartmentRequest>,
) -> Result<Json<DepartmentResponse>, ApiError> {
    services::step_up::consume(
        &state.pool,
        auth.session_id,
        auth.user_id,
        &headers,
        services::step_up::DEPARTMENTS_WRITE_SCOPE,
    )
    .await?;
    services::departments::update(&state.pool, &auth, id, &req)
        .await
        .map(Json)
}

pub async fn delete(
    State(state): State<AppState>,
    auth: RequirePermission<DepartmentWrite>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    services::step_up::consume(
        &state.pool,
        auth.session_id,
        auth.user_id,
        &headers,
        services::step_up::DEPARTMENTS_DELETE_SCOPE,
    )
    .await?;
    services::departments::delete(&state.pool, &auth, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
