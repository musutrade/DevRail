//! DevRail Phase 0 CRUD HTTP handlers.

use crate::auth::RequirePermission;
use crate::error::ApiError;
use crate::models::*;
use crate::permissions::devrail::*;
use crate::services;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;

pub async fn list_projects(
    State(s): State<AppState>,
    _auth: RequirePermission<ProjectRead>,
    Query(q): Query<DevRailListQuery>,
) -> Result<Json<DevRailProjectPage>, ApiError> {
    services::devrail::list_projects(&s.pool, &_auth, &q)
        .await
        .map(Json)
}
pub async fn get_project(
    State(s): State<AppState>,
    auth: RequirePermission<ProjectRead>,
    Path(id): Path<i64>,
) -> Result<Json<DevRailProjectResponse>, ApiError> {
    services::devrail::get_project(&s.pool, &auth, id)
        .await
        .map(Json)
}
pub async fn create_project(
    State(s): State<AppState>,
    auth: RequirePermission<ProjectWrite>,
    Json(req): Json<CreateDevRailProjectRequest>,
) -> Result<(StatusCode, Json<DevRailProjectResponse>), ApiError> {
    services::devrail::create_project(&s.pool, &auth, &req)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
}
pub async fn update_project(
    State(s): State<AppState>,
    auth: RequirePermission<ProjectWrite>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateDevRailProjectRequest>,
) -> Result<Json<DevRailProjectResponse>, ApiError> {
    services::devrail::update_project(&s.pool, &auth, id, &req)
        .await
        .map(Json)
}
pub async fn archive_project(
    State(s): State<AppState>,
    auth: RequirePermission<ProjectWrite>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    services::devrail::archive_project(&s.pool, &auth, id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
}

pub async fn list_repositories(
    State(s): State<AppState>,
    auth: RequirePermission<RepositoryRead>,
    Path(project_id): Path<i64>,
    Query(mut q): Query<DevRailListQuery>,
) -> Result<Json<DevRailRepositoryPage>, ApiError> {
    q.project_id = Some(project_id);
    services::devrail::list_repositories(&s.pool, &auth, &q)
        .await
        .map(Json)
}
pub async fn get_repository(
    State(s): State<AppState>,
    auth: RequirePermission<RepositoryRead>,
    Path((project_id, id)): Path<(i64, i64)>,
) -> Result<Json<DevRailRepositoryResponse>, ApiError> {
    services::devrail::get_repository(&s.pool, &auth, project_id, id)
        .await
        .map(Json)
}
pub async fn create_repository(
    State(s): State<AppState>,
    auth: RequirePermission<RepositoryWrite>,
    Path(project_id): Path<i64>,
    Json(req): Json<CreateDevRailRepositoryRequest>,
) -> Result<(StatusCode, Json<DevRailRepositoryResponse>), ApiError> {
    services::devrail::create_repository(&s.pool, &auth, project_id, &req)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
}
pub async fn update_repository(
    State(s): State<AppState>,
    auth: RequirePermission<RepositoryWrite>,
    Path((project_id, id)): Path<(i64, i64)>,
    Json(req): Json<UpdateDevRailRepositoryRequest>,
) -> Result<Json<DevRailRepositoryResponse>, ApiError> {
    services::devrail::update_repository(&s.pool, &auth, project_id, id, &req)
        .await
        .map(Json)
}

pub async fn list_environments(
    State(s): State<AppState>,
    auth: RequirePermission<EnvironmentRead>,
    Path(project_id): Path<i64>,
    Query(mut q): Query<DevRailListQuery>,
) -> Result<Json<DevRailEnvironmentPage>, ApiError> {
    q.project_id = Some(project_id);
    services::devrail::list_environments(&s.pool, &auth, &q)
        .await
        .map(Json)
}
pub async fn get_environment(
    State(s): State<AppState>,
    auth: RequirePermission<EnvironmentRead>,
    Path((project_id, id)): Path<(i64, i64)>,
) -> Result<Json<DevRailEnvironmentResponse>, ApiError> {
    services::devrail::get_environment(&s.pool, &auth, project_id, id)
        .await
        .map(Json)
}
pub async fn create_environment(
    State(s): State<AppState>,
    auth: RequirePermission<EnvironmentWrite>,
    Path(project_id): Path<i64>,
    Json(req): Json<CreateDevRailEnvironmentRequest>,
) -> Result<(StatusCode, Json<DevRailEnvironmentResponse>), ApiError> {
    services::devrail::create_environment(&s.pool, &auth, project_id, &req)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
}
pub async fn update_environment(
    State(s): State<AppState>,
    auth: RequirePermission<EnvironmentWrite>,
    Path((project_id, id)): Path<(i64, i64)>,
    Json(req): Json<UpdateDevRailEnvironmentRequest>,
) -> Result<Json<DevRailEnvironmentResponse>, ApiError> {
    services::devrail::update_environment(&s.pool, &auth, project_id, id, &req)
        .await
        .map(Json)
}

pub async fn list_tasks(
    State(s): State<AppState>,
    auth: RequirePermission<TaskRead>,
    Path(project_id): Path<i64>,
    Query(mut q): Query<DevRailListQuery>,
) -> Result<Json<DevRailTaskPage>, ApiError> {
    q.project_id = Some(project_id);
    services::devrail::list_tasks(&s.pool, &auth, &q)
        .await
        .map(Json)
}
pub async fn get_task(
    State(s): State<AppState>,
    auth: RequirePermission<TaskRead>,
    Path((project_id, id)): Path<(i64, i64)>,
) -> Result<Json<DevRailTaskResponse>, ApiError> {
    services::devrail::get_task(&s.pool, &auth, project_id, id)
        .await
        .map(Json)
}
pub async fn create_task(
    State(s): State<AppState>,
    auth: RequirePermission<TaskWrite>,
    Path(project_id): Path<i64>,
    Json(req): Json<CreateDevRailTaskRequest>,
) -> Result<(StatusCode, Json<DevRailTaskResponse>), ApiError> {
    services::devrail::create_task(&s.pool, &auth, project_id, &req)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
}
pub async fn update_task(
    State(s): State<AppState>,
    auth: RequirePermission<TaskWrite>,
    Path((project_id, id)): Path<(i64, i64)>,
    Json(req): Json<UpdateDevRailTaskRequest>,
) -> Result<Json<DevRailTaskResponse>, ApiError> {
    services::devrail::update_task(&s.pool, &auth, project_id, id, &req)
        .await
        .map(Json)
}
