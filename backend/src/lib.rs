//! arc-admin backend library: application wiring shared by the server and integration tests.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

pub mod access;
pub mod app_metrics;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod handlers;
pub mod mfa;
pub mod models;
pub mod openapi;
pub mod permissions;
pub mod repositories;
pub mod services;
pub mod telemetry;
pub mod workers;

pub use error::ApiError;

pub const API_PREFIX: &str = "/api/v1";
const HEALTHZ_PATH: &str = "/api/v1/healthz";
const READYZ_PATH: &str = "/api/v1/readyz";
const LOGIN_PATH: &str = "/api/v1/auth/login";
const LOGOUT_PATH: &str = "/api/v1/auth/logout";
const CURRENT_USER_PATH: &str = "/api/v1/auth/me";
const CURRENT_USER_PASSWORD_PATH: &str = "/api/v1/auth/me/password";
const STEP_UP_PATH: &str = "/api/v1/auth/me/step-up";
const MODULE_UNLOCKS_PATH: &str = "/api/v1/auth/me/module-unlocks";
const MODULE_UNLOCK_STATUS_PATH: &str = "/api/v1/auth/me/module-unlocks/{module}";
const CURRENT_USER_PERMISSIONS_PATH: &str = "/api/v1/auth/me/permissions";
const MFA_TOTP_VERIFY_PATH: &str = "/api/v1/auth/mfa/totp/verify";
const MFA_RECOVERY_VERIFY_PATH: &str = "/api/v1/auth/mfa/recovery/verify";
const MFA_PASSKEY_AUTH_START_PATH: &str = "/api/v1/auth/mfa/passkey/authenticate/start";
const MFA_PASSKEY_AUTH_FINISH_PATH: &str = "/api/v1/auth/mfa/passkey/authenticate/finish";
const MFA_STATUS_PATH: &str = "/api/v1/auth/me/mfa";
const MFA_PASSKEY_REGISTRATION_START_PATH: &str = "/api/v1/auth/me/mfa/passkey/register/start";
const MFA_PASSKEY_REGISTRATION_FINISH_PATH: &str = "/api/v1/auth/me/mfa/passkey/register/finish";
const MFA_PASSKEY_PATH: &str = "/api/v1/auth/me/mfa/passkey/{id}";
const MFA_RECOVERY_CODES_PATH: &str = "/api/v1/auth/me/mfa/recovery-codes";
const USERS_PATH: &str = "/api/v1/users";
const USER_PATH: &str = "/api/v1/users/{id}";
const USERS_BATCH_DELETE_PATH: &str = "/api/v1/users/batch-delete";
const USERS_BATCH_ROLES_PATH: &str = "/api/v1/users/batch-roles";
const USER_ROLES_PATH: &str = "/api/v1/users/{id}/roles";
const ROLES_PATH: &str = "/api/v1/roles";
const ROLE_PATH: &str = "/api/v1/roles/{id}";
const ROLE_PERMISSIONS_PATH: &str = "/api/v1/roles/{id}/permissions";
const DEPARTMENTS_PATH: &str = "/api/v1/departments";
const DEPARTMENT_PATH: &str = "/api/v1/departments/{id}";
const PERMISSION_GROUPS_PATH: &str = "/api/v1/permissions/groups";
const DASHBOARD_STATS_PATH: &str = "/api/v1/dashboard/stats";
const AUDIT_LOGS_PATH: &str = "/api/v1/audit-logs";
const DEVRAIL_PROJECTS_PATH: &str = "/api/v1/projects";
const DEVRAIL_PROJECT_PATH: &str = "/api/v1/projects/{id}";
const DEVRAIL_PROJECT_ARCHIVE_PATH: &str = "/api/v1/projects/{id}/archive";
const DEVRAIL_PROJECT_POLICY_PATH: &str = "/api/v1/projects/{id}/policy";
const DEVRAIL_PROJECT_MEMBERS_PATH: &str = "/api/v1/projects/{project_id}/members";
const DEVRAIL_PROJECT_MEMBER_PATH: &str = "/api/v1/projects/{project_id}/members/{user_id}";
const DEVRAIL_REPOSITORIES_PATH: &str = "/api/v1/projects/{project_id}/repositories";
const DEVRAIL_REPOSITORY_PATH: &str = "/api/v1/projects/{project_id}/repositories/{id}";
const DEVRAIL_REPOSITORY_PROVIDER_PATH: &str =
    "/api/v1/projects/{project_id}/repositories/{id}/git-provider";
const DEVRAIL_REPOSITORY_SYNC_PATH: &str = "/api/v1/projects/{project_id}/repositories/{id}/sync";
const DEVRAIL_REPOSITORY_WORKTREE_PATH: &str =
    "/api/v1/projects/{project_id}/repositories/{id}/worktree";
const DEVRAIL_ENVIRONMENTS_PATH: &str = "/api/v1/projects/{project_id}/environments";
const DEVRAIL_ENVIRONMENT_PATH: &str = "/api/v1/projects/{project_id}/environments/{id}";
const DEVRAIL_ENVIRONMENT_HEALTH_PATH: &str =
    "/api/v1/projects/{project_id}/environments/{id}/health-check";
const DEVRAIL_TASKS_PATH: &str = "/api/v1/projects/{project_id}/tasks";
const DEVRAIL_TASK_PATH: &str = "/api/v1/projects/{project_id}/tasks/{id}";
const DEVRAIL_TASK_RUNS_PATH: &str = "/api/v1/tasks/{task_id}/runs";
const DEVRAIL_TASK_COMMENTS_PATH: &str = "/api/v1/tasks/{task_id}/comments";
const DEVRAIL_TASK_COMMENT_PATH: &str = "/api/v1/task-comments/{id}";
const DEVRAIL_RUN_PATH: &str = "/api/v1/runs/{id}";
const DEVRAIL_RUN_INTERRUPT_PATH: &str = "/api/v1/runs/{id}/interrupt";
const DEVRAIL_RUN_RETRY_PATH: &str = "/api/v1/runs/{id}/retry";
const DEVRAIL_RUN_EVENTS_PATH: &str = "/api/v1/runs/{id}/events";
const DEVRAIL_RUN_CHANGESET_PATH: &str = "/api/v1/runs/{id}/changeset";
const DEVRAIL_RUN_PATCH_PATH: &str = "/api/v1/runs/{id}/patch";
const DEVRAIL_RUN_QUALITY_GATES_PATH: &str = "/api/v1/runs/{id}/quality-gates";
const DEVRAIL_RUN_QUALITY_GATES_EXECUTE_PATH: &str = "/api/v1/runs/{id}/quality-gates/execute";
const DEVRAIL_RUN_QUALITY_GATE_LOG_PATH: &str = "/api/v1/runs/{id}/quality-gate-log";
const DEVRAIL_RUN_EVENTS_STREAM_PATH: &str = "/api/v1/runs/{id}/events/stream";
const DEVRAIL_PUSH_DEVICES_PATH: &str = "/api/v1/push/devices";
const DEVRAIL_PUSH_CONFIG_PATH: &str = "/api/v1/push/config";
const DEVRAIL_PUSH_DEVICE_PATH: &str = "/api/v1/push/devices/{id}";
const DEVRAIL_APPROVALS_PATH: &str = "/api/v1/approvals";
const DEVRAIL_REVIEWS_PATH: &str = "/api/v1/reviews";
const DEVRAIL_REVIEW_DECIDE_PATH: &str = "/api/v1/reviews/{id}/decide";
const DEVRAIL_REVIEW_COMMENTS_PATH: &str = "/api/v1/reviews/{id}/comments";
const DEVRAIL_REVIEW_COMMENT_PATH: &str = "/api/v1/review-comments/{id}";
const DEVRAIL_APPROVAL_PATH: &str = "/api/v1/approvals/{id}";
const DEVRAIL_APPROVAL_APPROVE_PATH: &str = "/api/v1/approvals/{id}/approve";
const DEVRAIL_APPROVAL_RECOVER_PATH: &str = "/api/v1/approvals/{id}/recover";
const DEVRAIL_APPROVAL_REJECT_PATH: &str = "/api/v1/approvals/{id}/reject";
const DEVRAIL_APPROVAL_WITHDRAW_PATH: &str = "/api/v1/approvals/{id}/withdraw";
const DEVRAIL_NOTIFICATIONS_PATH: &str = "/api/v1/notifications";
const DEVRAIL_NOTIFICATION_READ_PATH: &str = "/api/v1/notifications/{id}/read";
const DEVRAIL_NOTIFICATIONS_READ_ALL_PATH: &str = "/api/v1/notifications/read-all";
const DEVRAIL_NOTIFICATION_PREFERENCES_PATH: &str = "/api/v1/notification-preferences";
const METRICS_PATH: &str = "/metrics";

/// Public HTTP operations generated into `docs/openapi.json`.
pub const API_ROUTE_CONTRACT: &[(&str, &[&str])] = &[
    (HEALTHZ_PATH, &["get"]),
    (READYZ_PATH, &["get"]),
    (LOGIN_PATH, &["post"]),
    (LOGOUT_PATH, &["post"]),
    (CURRENT_USER_PATH, &["get"]),
    (CURRENT_USER_PASSWORD_PATH, &["put"]),
    (STEP_UP_PATH, &["post"]),
    (MODULE_UNLOCKS_PATH, &["post"]),
    (MODULE_UNLOCK_STATUS_PATH, &["get"]),
    (CURRENT_USER_PERMISSIONS_PATH, &["get"]),
    (MFA_TOTP_VERIFY_PATH, &["post"]),
    (MFA_RECOVERY_VERIFY_PATH, &["post"]),
    (MFA_PASSKEY_AUTH_START_PATH, &["post"]),
    (MFA_PASSKEY_AUTH_FINISH_PATH, &["post"]),
    (MFA_STATUS_PATH, &["get"]),
    (MFA_PASSKEY_REGISTRATION_START_PATH, &["post"]),
    (MFA_PASSKEY_REGISTRATION_FINISH_PATH, &["post"]),
    (MFA_PASSKEY_PATH, &["delete"]),
    (MFA_RECOVERY_CODES_PATH, &["post"]),
    (USERS_PATH, &["get", "post"]),
    (USER_PATH, &["get", "put", "delete"]),
    (USERS_BATCH_DELETE_PATH, &["post"]),
    (USERS_BATCH_ROLES_PATH, &["put"]),
    (USER_ROLES_PATH, &["put"]),
    (ROLES_PATH, &["get", "post"]),
    (ROLE_PATH, &["get", "put", "delete"]),
    (ROLE_PERMISSIONS_PATH, &["get", "put"]),
    (DEPARTMENTS_PATH, &["get", "post"]),
    (DEPARTMENT_PATH, &["get", "put", "delete"]),
    (PERMISSION_GROUPS_PATH, &["get"]),
    (DASHBOARD_STATS_PATH, &["get"]),
    (AUDIT_LOGS_PATH, &["get"]),
    (DEVRAIL_PROJECTS_PATH, &["get", "post"]),
    (DEVRAIL_PROJECT_PATH, &["get", "patch"]),
    (DEVRAIL_PROJECT_ARCHIVE_PATH, &["post"]),
    (DEVRAIL_PROJECT_POLICY_PATH, &["get", "patch"]),
    (DEVRAIL_PROJECT_MEMBERS_PATH, &["get", "post"]),
    (DEVRAIL_PROJECT_MEMBER_PATH, &["delete"]),
    (DEVRAIL_REPOSITORIES_PATH, &["get", "post"]),
    (DEVRAIL_REPOSITORY_PATH, &["get", "patch"]),
    (DEVRAIL_REPOSITORY_PROVIDER_PATH, &["get"]),
    (DEVRAIL_REPOSITORY_SYNC_PATH, &["get", "post"]),
    (DEVRAIL_REPOSITORY_WORKTREE_PATH, &["get"]),
    (DEVRAIL_ENVIRONMENTS_PATH, &["get", "post"]),
    (DEVRAIL_ENVIRONMENT_PATH, &["get", "patch"]),
    (DEVRAIL_ENVIRONMENT_HEALTH_PATH, &["post"]),
    (DEVRAIL_TASKS_PATH, &["get", "post"]),
    (DEVRAIL_TASK_PATH, &["get", "patch"]),
    (DEVRAIL_TASK_RUNS_PATH, &["get", "post"]),
    (DEVRAIL_TASK_COMMENTS_PATH, &["get", "post"]),
    (DEVRAIL_TASK_COMMENT_PATH, &["patch", "delete"]),
    (DEVRAIL_RUN_PATH, &["get"]),
    (DEVRAIL_RUN_INTERRUPT_PATH, &["post"]),
    (DEVRAIL_RUN_RETRY_PATH, &["post"]),
    (DEVRAIL_RUN_EVENTS_PATH, &["get"]),
    (DEVRAIL_RUN_CHANGESET_PATH, &["get"]),
    (DEVRAIL_RUN_PATCH_PATH, &["get"]),
    (DEVRAIL_RUN_QUALITY_GATES_PATH, &["get"]),
    (DEVRAIL_RUN_QUALITY_GATES_EXECUTE_PATH, &["post"]),
    (DEVRAIL_RUN_QUALITY_GATE_LOG_PATH, &["get"]),
    (DEVRAIL_RUN_EVENTS_STREAM_PATH, &["get"]),
    (DEVRAIL_APPROVALS_PATH, &["get"]),
    (DEVRAIL_REVIEWS_PATH, &["get", "post"]),
    (DEVRAIL_REVIEW_DECIDE_PATH, &["post"]),
    (DEVRAIL_REVIEW_COMMENTS_PATH, &["get", "post"]),
    (DEVRAIL_REVIEW_COMMENT_PATH, &["patch"]),
    (DEVRAIL_APPROVAL_PATH, &["get"]),
    (DEVRAIL_APPROVAL_APPROVE_PATH, &["post"]),
    (DEVRAIL_APPROVAL_RECOVER_PATH, &["post"]),
    (DEVRAIL_APPROVAL_REJECT_PATH, &["post"]),
    (DEVRAIL_APPROVAL_WITHDRAW_PATH, &["post"]),
    (DEVRAIL_NOTIFICATIONS_PATH, &["get"]),
    (DEVRAIL_PUSH_DEVICES_PATH, &["get", "post"]),
    (DEVRAIL_PUSH_CONFIG_PATH, &["get"]),
    (DEVRAIL_PUSH_DEVICE_PATH, &["delete"]),
    (DEVRAIL_NOTIFICATION_PREFERENCES_PATH, &["get", "patch"]),
    (DEVRAIL_NOTIFICATION_READ_PATH, &["post"]),
    (DEVRAIL_NOTIFICATIONS_READ_ALL_PATH, &["post"]),
];

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub auth: Arc<auth::AuthSessionConfig>,
    pub mfa: Arc<mfa::MfaConfig>,
    pub supervisor: Arc<workers::harness_supervisor::HarnessSupervisor>,
    pub run_workspace_root: Arc<PathBuf>,
    pub web_push_public_key: Option<Arc<str>>,
}

async fn healthz() -> Json<models::HealthResponse> {
    Json(models::HealthResponse {
        status: "ok".to_string(),
    })
}

async fn readyz(State(state): State<AppState>) -> (StatusCode, Json<models::ReadinessResponse>) {
    let db_ok = db::ping(&state.pool).await;
    let status = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(models::ReadinessResponse {
            status: if db_ok { "ok" } else { "degraded" }.to_string(),
            db: db_ok,
        }),
    )
}

fn base_router(state: AppState) -> Router {
    app_metrics::initialize();
    Router::new()
        .route(METRICS_PATH, get(app_metrics::render))
        .route(HEALTHZ_PATH, get(healthz))
        .route(READYZ_PATH, get(readyz))
        .route(LOGIN_PATH, post(handlers::auth::login))
        .route(LOGOUT_PATH, post(handlers::auth::logout))
        .route(CURRENT_USER_PATH, get(handlers::auth::me))
        .route(
            CURRENT_USER_PASSWORD_PATH,
            put(handlers::auth::change_password),
        )
        .route(STEP_UP_PATH, post(handlers::auth::step_up))
        .route(MODULE_UNLOCKS_PATH, post(handlers::auth::module_unlock))
        .route(
            MODULE_UNLOCK_STATUS_PATH,
            get(handlers::auth::module_unlock_status),
        )
        .route(
            CURRENT_USER_PERMISSIONS_PATH,
            get(handlers::auth::me_permissions),
        )
        .route(MFA_TOTP_VERIFY_PATH, post(handlers::auth::verify_totp))
        .route(
            MFA_RECOVERY_VERIFY_PATH,
            post(handlers::auth::verify_recovery_code),
        )
        .route(
            MFA_PASSKEY_AUTH_START_PATH,
            post(handlers::auth::start_passkey_authentication),
        )
        .route(
            MFA_PASSKEY_AUTH_FINISH_PATH,
            post(handlers::auth::finish_passkey_authentication),
        )
        .route(MFA_STATUS_PATH, get(handlers::auth::mfa_status))
        .route(
            MFA_PASSKEY_REGISTRATION_START_PATH,
            post(handlers::auth::start_passkey_registration),
        )
        .route(
            MFA_PASSKEY_REGISTRATION_FINISH_PATH,
            post(handlers::auth::finish_passkey_registration),
        )
        .route(MFA_PASSKEY_PATH, delete(handlers::auth::revoke_passkey))
        .route(
            MFA_RECOVERY_CODES_PATH,
            post(handlers::auth::regenerate_recovery_codes),
        )
        .route(
            USERS_PATH,
            get(handlers::users::list).post(handlers::users::create),
        )
        .route(
            USER_PATH,
            get(handlers::users::get)
                .put(handlers::users::update)
                .delete(handlers::users::delete),
        )
        .route(USERS_BATCH_DELETE_PATH, post(handlers::users::batch_delete))
        .route(
            USERS_BATCH_ROLES_PATH,
            put(handlers::users::batch_assign_roles),
        )
        .route(USER_ROLES_PATH, put(handlers::users::assign_roles))
        .route(
            ROLES_PATH,
            get(handlers::roles::list).post(handlers::roles::create),
        )
        .route(
            ROLE_PATH,
            get(handlers::roles::get)
                .put(handlers::roles::update)
                .delete(handlers::roles::delete),
        )
        .route(
            ROLE_PERMISSIONS_PATH,
            get(handlers::roles::get_permissions).put(handlers::roles::put_permissions),
        )
        .route(
            DEPARTMENTS_PATH,
            get(handlers::departments::list).post(handlers::departments::create),
        )
        .route(
            DEPARTMENT_PATH,
            get(handlers::departments::get)
                .put(handlers::departments::update)
                .delete(handlers::departments::delete),
        )
        .route(PERMISSION_GROUPS_PATH, get(handlers::permissions::groups))
        .route(DASHBOARD_STATS_PATH, get(handlers::dashboard::stats))
        .route(AUDIT_LOGS_PATH, get(handlers::audit_logs::list))
        .route(
            DEVRAIL_PROJECTS_PATH,
            get(handlers::devrail::list_projects).post(handlers::devrail::create_project),
        )
        .route(
            DEVRAIL_PROJECT_PATH,
            get(handlers::devrail::get_project).patch(handlers::devrail::update_project),
        )
        .route(
            DEVRAIL_PROJECT_ARCHIVE_PATH,
            post(handlers::devrail::archive_project),
        )
        .route(
            DEVRAIL_PROJECT_POLICY_PATH,
            get(handlers::devrail::get_project_policy)
                .patch(handlers::devrail::update_project_policy),
        )
        .route(
            DEVRAIL_PROJECT_MEMBERS_PATH,
            get(handlers::devrail::list_members).post(handlers::devrail::add_member),
        )
        .route(
            DEVRAIL_PROJECT_MEMBER_PATH,
            axum::routing::delete(handlers::devrail::remove_member),
        )
        .route(
            DEVRAIL_REPOSITORIES_PATH,
            get(handlers::devrail::list_repositories).post(handlers::devrail::create_repository),
        )
        .route(
            DEVRAIL_REPOSITORY_PATH,
            get(handlers::devrail::get_repository).patch(handlers::devrail::update_repository),
        )
        .route(
            DEVRAIL_REPOSITORY_PROVIDER_PATH,
            get(handlers::devrail::get_git_provider),
        )
        .route(
            DEVRAIL_REPOSITORY_SYNC_PATH,
            get(handlers::devrail::get_repository_sync).post(handlers::devrail::sync_repository),
        )
        .route(
            DEVRAIL_REPOSITORY_WORKTREE_PATH,
            get(handlers::devrail::inspect_repository_worktree),
        )
        .route(
            DEVRAIL_ENVIRONMENTS_PATH,
            get(handlers::devrail::list_environments).post(handlers::devrail::create_environment),
        )
        .route(
            DEVRAIL_ENVIRONMENT_PATH,
            get(handlers::devrail::get_environment).patch(handlers::devrail::update_environment),
        )
        .route(
            DEVRAIL_ENVIRONMENT_HEALTH_PATH,
            post(handlers::devrail::health_check_environment),
        )
        .route(
            DEVRAIL_TASKS_PATH,
            get(handlers::devrail::list_tasks).post(handlers::devrail::create_task),
        )
        .route(
            DEVRAIL_TASK_PATH,
            get(handlers::devrail::get_task).patch(handlers::devrail::update_task),
        )
        .route(
            DEVRAIL_TASK_RUNS_PATH,
            get(handlers::devrail::list_runs).post(handlers::devrail::create_run),
        )
        .route(
            DEVRAIL_TASK_COMMENTS_PATH,
            get(handlers::devrail::list_task_comments).post(handlers::devrail::create_task_comment),
        )
        .route(
            DEVRAIL_TASK_COMMENT_PATH,
            patch(handlers::devrail::update_task_comment)
                .delete(handlers::devrail::delete_task_comment),
        )
        .route(DEVRAIL_RUN_PATH, get(handlers::devrail::get_run))
        .route(
            DEVRAIL_RUN_INTERRUPT_PATH,
            post(handlers::devrail::interrupt_run),
        )
        .route(DEVRAIL_RUN_RETRY_PATH, post(handlers::devrail::retry_run))
        .route(
            DEVRAIL_RUN_EVENTS_PATH,
            get(handlers::devrail::list_run_events),
        )
        .route(
            DEVRAIL_RUN_CHANGESET_PATH,
            get(handlers::devrail::get_run_changeset),
        )
        .route(
            DEVRAIL_RUN_PATCH_PATH,
            get(handlers::devrail::export_run_patch),
        )
        .route(
            DEVRAIL_RUN_QUALITY_GATES_PATH,
            get(handlers::devrail::get_run_quality_gates),
        )
        .route(
            DEVRAIL_RUN_QUALITY_GATES_EXECUTE_PATH,
            post(handlers::devrail::execute_run_quality_gates),
        )
        .route(
            DEVRAIL_RUN_QUALITY_GATE_LOG_PATH,
            get(handlers::devrail::get_run_quality_gate_log),
        )
        .route(
            DEVRAIL_RUN_EVENTS_STREAM_PATH,
            get(handlers::devrail::stream_run_events),
        )
        .route(
            DEVRAIL_APPROVALS_PATH,
            get(handlers::devrail::list_approvals),
        )
        .route(
            DEVRAIL_NOTIFICATIONS_PATH,
            get(handlers::devrail::list_notifications),
        )
        .route(
            DEVRAIL_PUSH_DEVICES_PATH,
            get(handlers::devrail::list_push_devices).post(handlers::devrail::register_push_device),
        )
        .route(
            DEVRAIL_PUSH_CONFIG_PATH,
            get(handlers::devrail::get_push_config),
        )
        .route(
            DEVRAIL_PUSH_DEVICE_PATH,
            delete(handlers::devrail::revoke_push_device),
        )
        .route(
            DEVRAIL_NOTIFICATION_READ_PATH,
            post(handlers::devrail::mark_notification_read),
        )
        .route(
            DEVRAIL_NOTIFICATIONS_READ_ALL_PATH,
            post(handlers::devrail::mark_all_notifications_read),
        )
        .route(
            DEVRAIL_NOTIFICATION_PREFERENCES_PATH,
            get(handlers::devrail::get_notification_preferences)
                .patch(handlers::devrail::update_notification_preferences),
        )
        .route(DEVRAIL_APPROVAL_PATH, get(handlers::devrail::get_approval))
        .route(
            DEVRAIL_REVIEWS_PATH,
            get(handlers::devrail::list_reviews).post(handlers::devrail::create_review),
        )
        .route(
            DEVRAIL_REVIEW_DECIDE_PATH,
            post(handlers::devrail::decide_review),
        )
        .route(
            DEVRAIL_REVIEW_COMMENTS_PATH,
            get(handlers::devrail::list_review_comments)
                .post(handlers::devrail::create_review_comment),
        )
        .route(
            DEVRAIL_REVIEW_COMMENT_PATH,
            patch(handlers::devrail::update_review_comment),
        )
        .route(
            DEVRAIL_APPROVAL_APPROVE_PATH,
            post(handlers::devrail::approve_approval),
        )
        .route(
            DEVRAIL_APPROVAL_RECOVER_PATH,
            post(handlers::devrail::recover_approval),
        )
        .route(
            DEVRAIL_APPROVAL_REJECT_PATH,
            post(handlers::devrail::reject_approval),
        )
        .route(
            DEVRAIL_APPROVAL_WITHDRAW_PATH,
            post(handlers::devrail::withdraw_approval),
        )
        .with_state(state)
}

pub fn build_router(state: AppState) -> Router {
    telemetry::default_http_observability(base_router(state))
}

pub fn build_router_with_metadata(
    state: AppState,
    metadata: telemetry::TelemetryMetadata,
) -> Router {
    telemetry::with_http_observability(base_router(state), metadata)
}

pub fn build_router_with_metadata_and_cors(
    state: AppState,
    metadata: telemetry::TelemetryMetadata,
    cors: CorsLayer,
) -> Router {
    telemetry::with_http_observability(base_router(state).layer(cors), metadata)
}
