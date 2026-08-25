//! DevRail Phase 0 CRUD HTTP handlers.

use crate::auth::RequirePermission;
use crate::error::ApiError;
use crate::models::*;
use crate::permissions::devrail::*;
use crate::services;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures_core::Stream;
use std::convert::Infallible;
use std::time::Duration;

pub async fn list_projects(
    State(s): State<AppState>,
    _auth: RequirePermission<ProjectRead>,
    Query(q): Query<DevRailListQuery>,
) -> Result<Json<DevRailProjectPage>, ApiError> {
    services::devrail::list_projects(&s.pool, &_auth, &q)
        .await
        .map(Json)
}
pub async fn list_notifications(
    State(s): State<AppState>,
    auth: RequirePermission<NotificationRead>,
    Query(q): Query<DevRailListQuery>,
) -> Result<Json<DevRailNotificationPage>, ApiError> {
    services::devrail_notifications::list(
        &s.pool,
        &auth,
        q.page.unwrap_or(1).max(1),
        q.page_size.unwrap_or(20).clamp(1, 100),
    )
    .await
    .map(Json)
}
pub async fn mark_notification_read(
    State(s): State<AppState>,
    auth: RequirePermission<NotificationRead>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    services::devrail_notifications::mark_read(&s.pool, &auth, id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
}
pub async fn mark_all_notifications_read(
    State(s): State<AppState>,
    auth: RequirePermission<NotificationRead>,
) -> Result<StatusCode, ApiError> {
    services::devrail_notifications::mark_all_read(&s.pool, &auth)
        .await
        .map(|_| StatusCode::NO_CONTENT)
}
pub async fn get_notification_preferences(
    State(s): State<AppState>,
    auth: RequirePermission<NotificationRead>,
) -> Result<Json<DevRailNotificationPreferencesResponse>, ApiError> {
    services::devrail_notifications::get_preferences(&s.pool, &auth)
        .await
        .map(Json)
}
pub async fn update_notification_preferences(
    State(s): State<AppState>,
    auth: RequirePermission<NotificationWrite>,
    Json(request): Json<UpdateDevRailNotificationPreferencesRequest>,
) -> Result<Json<DevRailNotificationPreferencesResponse>, ApiError> {
    services::devrail_notifications::update_preferences(&s.pool, &auth, &request)
        .await
        .map(Json)
}
pub async fn list_push_devices(
    State(s): State<AppState>,
    auth: RequirePermission<PushDeviceRead>,
) -> Result<Json<Vec<DevRailPushDeviceResponse>>, ApiError> {
    services::devrail_push::list(&s.pool, &auth).await.map(Json)
}

pub async fn get_push_config(
    State(s): State<AppState>,
    _auth: RequirePermission<PushDeviceRead>,
) -> Result<Json<DevRailPushConfigResponse>, ApiError> {
    Ok(Json(DevRailPushConfigResponse {
        enabled: s.web_push_public_key.is_some(),
        public_key: s.web_push_public_key.as_deref().map(str::to_string),
    }))
}
pub async fn register_push_device(
    State(s): State<AppState>,
    auth: RequirePermission<PushDeviceWrite>,
    Json(request): Json<RegisterDevRailPushDeviceRequest>,
) -> Result<Json<DevRailPushDeviceResponse>, ApiError> {
    services::devrail_push::register(&s.pool, &auth, &s.mfa, &request)
        .await
        .map(Json)
}
pub async fn revoke_push_device(
    State(s): State<AppState>,
    auth: RequirePermission<PushDeviceRevoke>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    services::devrail_push::revoke(&s.pool, &auth, id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
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

pub async fn get_project_policy(
    State(s): State<AppState>,
    auth: RequirePermission<ProjectRead>,
    Path(id): Path<i64>,
) -> Result<Json<DevRailProjectPolicyResponse>, ApiError> {
    services::devrail::get_project_policy(&s.pool, &auth, id)
        .await
        .map(Json)
}

pub async fn update_project_policy(
    State(s): State<AppState>,
    auth: RequirePermission<ProjectWrite>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateDevRailProjectPolicyRequest>,
) -> Result<Json<DevRailProjectPolicyResponse>, ApiError> {
    services::devrail::update_project_policy(&s.pool, &auth, id, &req)
        .await
        .map(Json)
}

pub async fn list_members(
    State(s): State<AppState>,
    auth: RequirePermission<MemberRead>,
    Path(project_id): Path<i64>,
) -> Result<Json<DevRailProjectMemberPage>, ApiError> {
    services::devrail_members::list(&s.pool, &auth, project_id)
        .await
        .map(Json)
}

pub async fn add_member(
    State(s): State<AppState>,
    auth: RequirePermission<MemberWrite>,
    Path(project_id): Path<i64>,
    Json(req): Json<AddDevRailProjectMemberRequest>,
) -> Result<(StatusCode, Json<DevRailProjectMemberResponse>), ApiError> {
    services::devrail_members::add(&s.pool, &auth, project_id, &req)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
}

pub async fn remove_member(
    State(s): State<AppState>,
    auth: RequirePermission<MemberWrite>,
    Path((project_id, user_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    services::devrail_members::remove(&s.pool, &auth, project_id, user_id)
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
pub async fn sync_repository(
    State(s): State<AppState>,
    auth: RequirePermission<RepositoryWrite>,
    Path((project_id, id)): Path<(i64, i64)>,
) -> Result<Json<DevRailRepositoryResponse>, ApiError> {
    services::devrail::sync_repository(&s.pool, &auth, project_id, id)
        .await
        .map(Json)
}
pub async fn get_repository_sync(
    State(s): State<AppState>,
    auth: RequirePermission<RepositoryRead>,
    Path((project_id, id)): Path<(i64, i64)>,
    Query(query): Query<DevRailRepositorySyncQuery>,
) -> Result<Json<DevRailRepositorySyncResponse>, ApiError> {
    services::devrail::get_repository_sync(
        &s.pool,
        &auth,
        project_id,
        id,
        query.environment_id,
        s.run_workspace_root.as_ref(),
    )
    .await
    .map(Json)
}
pub async fn inspect_repository_worktree(
    State(s): State<AppState>,
    auth: RequirePermission<RepositoryRead>,
    Path((project_id, repository_id)): Path<(i64, i64)>,
    Query(query): Query<DevRailWorktreeQuery>,
) -> Result<Json<DevRailWorktreeResponse>, ApiError> {
    services::devrail::inspect_repository_worktree(
        &s.pool,
        &auth,
        project_id,
        repository_id,
        query.environment_id,
        s.run_workspace_root.as_ref(),
    )
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
pub async fn health_check_environment(
    State(s): State<AppState>,
    auth: RequirePermission<EnvironmentRead>,
    Path((project_id, id)): Path<(i64, i64)>,
) -> Result<Json<DevRailEnvironmentHealthResponse>, ApiError> {
    services::devrail::health_check_environment(&s.pool, &auth, project_id, id)
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
pub async fn list_task_comments(
    State(s): State<AppState>,
    auth: RequirePermission<CommentRead>,
    Path(task_id): Path<i64>,
    Query(q): Query<DevRailListQuery>,
) -> Result<Json<DevRailTaskCommentPage>, ApiError> {
    services::devrail_comments::list(
        &s.pool,
        &auth,
        task_id,
        q.page.unwrap_or(1).max(1),
        q.page_size.unwrap_or(20).clamp(1, 100),
    )
    .await
    .map(Json)
}
pub async fn create_task_comment(
    State(s): State<AppState>,
    auth: RequirePermission<CommentWrite>,
    Path(task_id): Path<i64>,
    Json(request): Json<CreateDevRailTaskCommentRequest>,
) -> Result<(StatusCode, Json<DevRailTaskCommentResponse>), ApiError> {
    services::devrail_comments::create(&s.pool, &auth, task_id, &request)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
}
pub async fn update_task_comment(
    State(s): State<AppState>,
    auth: RequirePermission<CommentWrite>,
    Path(id): Path<i64>,
    Json(request): Json<UpdateDevRailTaskCommentRequest>,
) -> Result<Json<DevRailTaskCommentResponse>, ApiError> {
    services::devrail_comments::update(&s.pool, &auth, id, &request)
        .await
        .map(Json)
}
pub async fn delete_task_comment(
    State(s): State<AppState>,
    auth: RequirePermission<CommentWrite>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    services::devrail_comments::delete(&s.pool, &auth, id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
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

pub async fn create_run(
    State(s): State<AppState>,
    auth: RequirePermission<RunExecute>,
    Path(task_id): Path<i64>,
    Json(req): Json<CreateDevRailRunRequest>,
) -> Result<(StatusCode, Json<DevRailRunResponse>), ApiError> {
    services::devrail_runs::create_run(&s.pool, &auth, &s.supervisor, task_id, &req)
        .await
        .map(|v| (StatusCode::ACCEPTED, Json(v)))
}

pub async fn list_runs(
    State(s): State<AppState>,
    auth: RequirePermission<RunRead>,
    Path(task_id): Path<i64>,
    Query(q): Query<DevRailListQuery>,
) -> Result<Json<DevRailRunPage>, ApiError> {
    let page = q.page.unwrap_or(1).max(1);
    let size = q.page_size.unwrap_or(20).clamp(1, 100);
    services::devrail_runs::list_runs(&s.pool, &auth, task_id, page, size)
        .await
        .map(Json)
}

pub async fn get_run(
    State(s): State<AppState>,
    auth: RequirePermission<RunRead>,
    Path(id): Path<i64>,
) -> Result<Json<DevRailRunResponse>, ApiError> {
    services::devrail_runs::get_run(&s.pool, &auth, id)
        .await
        .map(Json)
}

pub async fn interrupt_run(
    State(s): State<AppState>,
    auth: RequirePermission<RunInterrupt>,
    Path(id): Path<i64>,
) -> Result<Json<DevRailRunResponse>, ApiError> {
    services::devrail_runs::interrupt_run(&s.pool, &auth, &s.supervisor, id)
        .await
        .map(Json)
}

pub async fn retry_run(
    State(s): State<AppState>,
    auth: RequirePermission<RunRetry>,
    Path(id): Path<i64>,
    Json(req): Json<RetryDevRailRunRequest>,
) -> Result<(StatusCode, Json<DevRailRunResponse>), ApiError> {
    services::devrail_runs::retry_run(&s.pool, &auth, &s.supervisor, id, &req)
        .await
        .map(|v| (StatusCode::ACCEPTED, Json(v)))
}

pub async fn list_approvals(
    State(s): State<AppState>,
    auth: RequirePermission<ApprovalRead>,
    Query(q): Query<DevRailListQuery>,
) -> Result<Json<DevRailApprovalPage>, ApiError> {
    services::devrail_approvals::list(
        &s.pool,
        &auth,
        q.page.unwrap_or(1).max(1),
        q.page_size.unwrap_or(20).clamp(1, 100),
    )
    .await
    .map(Json)
}
pub async fn get_approval(
    State(s): State<AppState>,
    auth: RequirePermission<ApprovalRead>,
    Path(id): Path<i64>,
) -> Result<Json<DevRailApprovalResponse>, ApiError> {
    services::devrail_approvals::get(&s.pool, &auth, id)
        .await
        .map(Json)
}
pub async fn list_reviews(
    State(s): State<AppState>,
    auth: RequirePermission<ReviewRead>,
    Query(q): Query<DevRailListQuery>,
) -> Result<Json<DevRailReviewPage>, ApiError> {
    services::devrail_reviews::list(
        &s.pool,
        &auth,
        q.page.unwrap_or(1).max(1),
        q.page_size.unwrap_or(20).clamp(1, 100),
    )
    .await
    .map(Json)
}
pub async fn create_review(
    State(s): State<AppState>,
    auth: RequirePermission<ReviewWrite>,
    Json(req): Json<CreateDevRailReviewRequest>,
) -> Result<(StatusCode, Json<DevRailReviewResponse>), ApiError> {
    services::devrail_reviews::create(&s.pool, &auth, &req)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
}
pub async fn decide_review(
    State(s): State<AppState>,
    auth: RequirePermission<ReviewWrite>,
    Path(id): Path<i64>,
    Json(req): Json<DecideDevRailReviewRequest>,
) -> Result<Json<DevRailReviewResponse>, ApiError> {
    services::devrail_reviews::decide(&s.pool, &auth, id, &req)
        .await
        .map(Json)
}
pub async fn list_review_comments(
    State(s): State<AppState>,
    auth: RequirePermission<ReviewRead>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<DevRailReviewCommentResponse>>, ApiError> {
    services::devrail_reviews::list_comments(&s.pool, &auth, id)
        .await
        .map(Json)
}
pub async fn create_review_comment(
    State(s): State<AppState>,
    auth: RequirePermission<ReviewWrite>,
    Path(id): Path<i64>,
    Json(req): Json<CreateDevRailReviewCommentRequest>,
) -> Result<(StatusCode, Json<DevRailReviewCommentResponse>), ApiError> {
    services::devrail_reviews::create_comment(&s.pool, &auth, id, &req)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
}
pub async fn update_review_comment(
    State(s): State<AppState>,
    auth: RequirePermission<ReviewWrite>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateDevRailReviewCommentRequest>,
) -> Result<Json<DevRailReviewCommentResponse>, ApiError> {
    services::devrail_reviews::update_comment(&s.pool, &auth, id, &req)
        .await
        .map(Json)
}
pub async fn approve_approval(
    State(s): State<AppState>,
    auth: RequirePermission<ApprovalApprove>,
    Path(id): Path<i64>,
    Json(req): Json<DevRailApprovalDecisionRequest>,
) -> Result<Json<DevRailApprovalResponse>, ApiError> {
    services::devrail_approvals::approve(&s.pool, &auth, &s.supervisor, id, req.reason.as_deref())
        .await
        .map(Json)
}
pub async fn recover_approval(
    State(s): State<AppState>,
    auth: RequirePermission<ApprovalApprove>,
    Path(id): Path<i64>,
) -> Result<Json<DevRailApprovalResponse>, ApiError> {
    services::devrail_approvals::recover(&s.pool, &auth, &s.supervisor, id)
        .await
        .map(Json)
}
pub async fn reject_approval(
    State(s): State<AppState>,
    auth: RequirePermission<ApprovalReject>,
    Path(id): Path<i64>,
    Json(req): Json<DevRailApprovalDecisionRequest>,
) -> Result<Json<DevRailApprovalResponse>, ApiError> {
    services::devrail_approvals::reject(&s.pool, &auth, &s.supervisor, id, req.reason.as_deref())
        .await
        .map(Json)
}
pub async fn withdraw_approval(
    State(s): State<AppState>,
    auth: RequirePermission<ApprovalReject>,
    Path(id): Path<i64>,
    Json(req): Json<DevRailApprovalDecisionRequest>,
) -> Result<Json<DevRailApprovalResponse>, ApiError> {
    services::devrail_approvals::withdraw(&s.pool, &auth, &s.supervisor, id, req.reason.as_deref())
        .await
        .map(Json)
}

pub async fn list_run_events(
    State(s): State<AppState>,
    auth: RequirePermission<RunRead>,
    Path(id): Path<i64>,
    Query(query): Query<RunEventQuery>,
) -> Result<Json<DevRailRunEventPage>, ApiError> {
    services::devrail_runs::list_events(
        &s.pool,
        &auth,
        id,
        query.after_cursor.unwrap_or(0).max(0),
        query.limit.unwrap_or(100),
    )
    .await
    .map(Json)
}
pub async fn get_run_changeset(
    State(s): State<AppState>,
    auth: RequirePermission<RunRead>,
    Path(id): Path<i64>,
) -> Result<Json<DevRailChangesetResponse>, ApiError> {
    services::devrail_runs::get_changeset(&s.pool, &auth, id)
        .await
        .map(Json)
}
pub async fn export_run_patch(
    State(s): State<AppState>,
    auth: RequirePermission<RunRead>,
    Path(id): Path<i64>,
) -> Result<Json<DevRailPatchExportResponse>, ApiError> {
    services::devrail_runs::export_patch(&s.pool, &auth, id, s.run_workspace_root.as_ref())
        .await
        .map(Json)
}
pub async fn get_run_quality_gates(
    State(s): State<AppState>,
    auth: RequirePermission<RunRead>,
    Path(id): Path<i64>,
) -> Result<Json<DevRailQualityGatePage>, ApiError> {
    services::devrail_runs::get_quality_gates(&s.pool, &auth, id)
        .await
        .map(Json)
}

pub async fn get_run_quality_gate_log(
    State(s): State<AppState>,
    auth: RequirePermission<RunRead>,
    Path(id): Path<i64>,
    Query(query): Query<QualityGateLogQuery>,
) -> Result<Json<DevRailQualityGateLogPage>, ApiError> {
    services::devrail_runs::get_quality_gate_log(
        &s.pool,
        &auth,
        id,
        &query.log_ref,
        query.after_cursor.unwrap_or(0),
        query.limit.unwrap_or(100),
    )
    .await
    .map(Json)
}
pub async fn execute_run_quality_gates(
    State(s): State<AppState>,
    auth: RequirePermission<RunExecute>,
    Path(id): Path<i64>,
) -> Result<Json<DevRailQualityGatePage>, ApiError> {
    services::devrail_runs::execute_quality_gates(&s.pool, &auth, id)
        .await
        .map(Json)
}

#[derive(Debug, serde::Deserialize)]
pub struct RunEventQuery {
    pub after_cursor: Option<i64>,
    pub limit: Option<i64>,
}

pub async fn stream_run_events(
    State(s): State<AppState>,
    auth: RequirePermission<RunRead>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Query(query): Query<RunEventQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let _ = services::devrail_runs::get_run(&s.pool, &auth, id).await?;
    let pool = s.pool.clone();
    let actor = auth.actor.clone();
    let header_cursor = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    let initial_cursor = query.after_cursor.unwrap_or(header_cursor).max(0);
    let stream = async_stream::stream! {
        let mut cursor = initial_cursor;
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            match services::devrail_runs::list_events(&pool, &actor, id, cursor, 100).await {
                Ok(page) => {
                    for event in page.items { cursor = event.cursor; if let Ok(data) = serde_json::to_string(&event) { yield Ok(Event::default().id(cursor.to_string()).event(event.event_type).data(data)); } }
                }
                Err(_) => break,
            }
            if let Ok(run) = services::devrail_runs::get_run(&pool, &actor, id).await { if matches!(run.status.as_str(), "completed"|"failed"|"cancelled") { break; } }
        }
    };
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("heartbeat"),
    ))
}
