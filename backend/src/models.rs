//! 数据模型：数据库行（FromRow）+ API DTO（serde）
//! 约定：本文件只放纯数据结构与派生宏；转换函数用自由函数，不放 impl 业务逻辑

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use std::collections::BTreeSet;
use uuid::Uuid;

// ===== 数据库行 =====

#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub display_name: String,
    pub email: Option<String>,
    pub status: String,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub token_version: i64,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct UserWithRolesRow {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub status: String,
    pub department_id: Option<i64>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct RoleRow {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub category: String,
    pub icon: Option<String>,
    pub color: String,
    pub description: Option<String>,
    pub data_scope: String,
    pub is_active: bool,
    pub members: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct RoleWithPermissionsRow {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub category: String,
    pub icon: Option<String>,
    pub color: String,
    pub description: Option<String>,
    pub data_scope: String,
    pub is_active: bool,
    pub members: i64,
    pub permission_group_ids: Vec<i64>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DepartmentRow {
    pub id: i64,
    pub organization_id: i64,
    pub parent_id: Option<i64>,
    pub code: String,
    pub name: String,
    pub status: String,
    pub depth: i32,
    pub member_count: i64,
    pub child_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct PermissionGroupRow {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct PermissionRow {
    pub id: i64,
    pub group_id: i64,
    pub code: String,
    pub name: String,
    #[sqlx(rename = "type")]
    pub r#type: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DashboardStatsRow {
    pub total_users: i64,
    pub active_users: i64,
    pub total_roles: i64,
    pub total_permissions: i64,
    pub suspended_users: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct AuditLogRow {
    pub id: i64,
    pub actor_user_id: Option<i64>,
    pub actor_username: Option<String>,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<i64>,
    pub details: Value,
    pub trace_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditArchiveRow {
    pub id: i64,
    pub actor_user_id: Option<i64>,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<i64>,
    pub details: Value,
    pub trace_id: Option<String>,
    pub organization_id: Option<i64>,
    pub department_id: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DevRailProjectRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub default_repository_id: Option<i64>,
    pub default_environment_id: Option<i64>,
    pub notification_policy: Value,
    pub quality_gate_template: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DevRailRepositoryRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub project_id: i64,
    pub name: String,
    pub remote_url: String,
    pub protocol: String,
    pub default_branch: String,
    pub credential_ref: Option<String>,
    pub last_sync_status: String,
    pub last_head_sha: Option<String>,
    pub last_remote_branch: Option<String>,
    pub last_remote_branch_count: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DevRailEnvironmentRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub project_id: i64,
    pub name: String,
    pub workspace_root: String,
    pub network_mode: String,
    pub tool_policy: Value,
    pub secret_refs: Value,
    pub max_duration_secs: i64,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DevRailTaskRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub project_id: i64,
    pub repository_id: Option<i64>,
    pub environment_id: Option<i64>,
    pub assignee_user_id: Option<i64>,
    pub title: String,
    pub goal: String,
    pub background: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub constraints: Option<String>,
    pub priority: String,
    pub status: String,
    pub revision: i64,
    pub dispatch_snapshot: Value,
    pub dispatch_snapshot_digest: String,
    pub workflow_source: String,
    pub workflow_version: String,
    pub workflow_digest: String,
    pub scheduler_attempt: i32,
    pub scheduler_retry_count: i32,
    pub scheduler_max_attempts: i32,
    pub scheduler_retry_at: Option<DateTime<Utc>>,
    pub scheduler_last_error: Option<String>,
    pub creation_source: String,
    pub source_task_id: Option<i64>,
    pub source_run_id: Option<i64>,
    pub followup_depth: i16,
    pub labels: Value,
    pub due_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DevRailTaskDependencyRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub task_id: i64,
    pub prerequisite_task_id: i64,
    pub prerequisite_title: String,
    pub prerequisite_status: String,
    pub failure_action: String,
    pub cancelled_action: String,
    pub timeout_action: String,
    pub creation_source: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DevRailTaskDependentRow {
    pub id: i64,
    pub task_id: i64,
    pub task_title: String,
    pub task_status: String,
    pub failure_action: String,
    pub cancelled_action: String,
    pub timeout_action: String,
    pub creation_source: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DevRailFollowupRequestRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub source_task_id: i64,
    pub source_run_id: i64,
    pub idempotency_key: String,
    pub request_digest: String,
    pub status: String,
    pub result_task_id: Option<i64>,
    pub error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DevRailTaskEventRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub task_id: i64,
    pub cursor: i64,
    pub event_type: String,
    pub idempotency_key: String,
    pub payload: Value,
    pub summary: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DevRailTaskCommentRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub task_id: i64,
    pub author_user_id: i64,
    pub author_username: String,
    pub author_display_name: String,
    pub body: String,
    pub mentions: Value,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DevRailProjectMemberRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub project_id: i64,
    pub user_id: i64,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub joined_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DevRailRunRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub task_id: i64,
    pub snapshot_id: i64,
    pub idempotency_key: String,
    pub attempt: i32,
    pub task_revision: i64,
    pub workflow_source: String,
    pub workflow_version: String,
    pub workflow_digest: String,
    pub workflow_snapshot: Value,
    pub actor_type: String,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub last_event_at: Option<DateTime<Utc>>,
    pub retry_reason: Option<String>,
    pub parent_run_id: Option<i64>,
    pub parent_turn_id: Option<String>,
    pub run_kind: String,
    pub root_run_id: Option<i64>,
    pub continuation_sequence: Option<i16>,
    pub continuation_request_id: Option<i64>,
    pub harness_start_key: Option<String>,
    pub harness_start_claim_owner: Option<String>,
    pub harness_start_claim_token: Option<Uuid>,
    pub harness_start_claim_expires_at: Option<DateTime<Utc>>,
    pub harness_started_token: Option<Uuid>,
    pub cleanup_status: String,
    pub branch_name: Option<String>,
    pub branch_expires_at: Option<DateTime<Utc>>,
    pub status: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub harness_version: Option<String>,
    pub model_id: Option<String>,
    pub cwd: String,
    pub policy: Value,
    pub startup_args_summary: Value,
    pub exit_reason: Option<String>,
    pub exit_code: Option<i32>,
    pub stderr_summary: Option<String>,
    pub trace_id: Option<String>,
    pub recovery_suggestion: Option<String>,
    pub recovery_attempts: i32,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DevRailContinuationRequestRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub project_id: i64,
    pub task_id: i64,
    pub source_run_id: i64,
    pub root_run_id: i64,
    pub source_turn_id: String,
    pub requested_by_user_id: Option<i64>,
    pub trigger_type: String,
    pub evidence_ref: String,
    pub evidence_digest: String,
    pub evidence_observed_at: DateTime<Utc>,
    pub evidence_expires_at: Option<DateTime<Utc>>,
    pub changeset_digest: Option<String>,
    pub redacted_context: String,
    pub context_summary: String,
    pub input_digest: String,
    pub idempotency_key: String,
    pub continuation_sequence: i16,
    pub chain_depth: i16,
    pub prior_task_status: String,
    pub policy_version: String,
    pub policy_snapshot: Value,
    pub status: String,
    pub status_version: i64,
    pub claim_owner: Option<String>,
    pub claim_token: Option<Uuid>,
    pub claim_expires_at: Option<DateTime<Utc>>,
    pub dispatch_attempts: i32,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub child_run_id: Option<i64>,
    pub result_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub dispatched_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub rejected_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DevRailRunHandoffRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub project_id: i64,
    pub task_id: i64,
    pub source_run_id: i64,
    pub task_snapshot_id: i64,
    pub repository_id: i64,
    pub environment_id: Option<i64>,
    pub task_snapshot_digest: String,
    pub workflow_snapshot_digest: String,
    pub environment_snapshot_digest: Option<String>,
    pub repository_identity: String,
    pub repository_identity_digest: String,
    pub base_commit: String,
    pub head_commit: Option<String>,
    pub branch_ref: Option<String>,
    pub changeset_ref: Option<String>,
    pub changeset_digest: String,
    pub tool_versions: Value,
    pub evidence_status: String,
    pub error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub validated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DevRailTaskWorkspaceRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub task_id: i64,
    pub run_id: Option<i64>,
    pub attempt: i32,
    pub workspace_key: String,
    pub relative_path: String,
    pub path_digest: String,
    pub repository_id: Option<i64>,
    pub environment_id: Option<i64>,
    pub base_commit: Option<String>,
    pub branch_name: Option<String>,
    pub workflow_version: Option<String>,
    pub workflow_digest: Option<String>,
    pub environment_version: Option<String>,
    pub tool_versions: Value,
    pub snapshot_digest: Option<String>,
    pub lifecycle_status: String,
    pub cleanup_status: String,
    pub cleanup_attempts: i32,
    pub next_cleanup_at: Option<DateTime<Utc>>,
    pub last_hook: Option<String>,
    pub diagnostic_ref: Option<String>,
    pub error_summary: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub cleaned_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DevRailRunEventRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub run_id: i64,
    pub cursor: i64,
    pub event_type: String,
    pub source_event_id: Option<String>,
    pub idempotency_key: String,
    pub payload: Value,
    pub summary: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DevRailApprovalRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub run_id: i64,
    pub event_id: Option<i64>,
    pub idempotency_key: String,
    pub tool_name: String,
    pub args_summary: Value,
    pub cwd: String,
    pub impact_scope: Option<String>,
    pub risk_level: String,
    pub requested_by: i64,
    pub decided_by: Option<i64>,
    pub status: String,
    pub decision_reason: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub policy_version: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ===== API DTO =====

pub const DEFAULT_CONTINUATION_MAX_COUNT: u16 = 3;
pub const DEFAULT_CONTINUATION_MAX_CHAIN_DEPTH: u16 = 3;
pub const DEFAULT_CONTINUATION_MAX_CONTEXT_BYTES: usize = 16 * 1024;
pub const DEFAULT_CONTINUATION_CLAIM_LEASE_SECONDS: i64 = 60;
pub const DEFAULT_CONTINUATION_MAX_DISPATCH_ATTEMPTS: i32 = 3;
pub const DEFAULT_CONTINUATION_RETRY_BASE_DELAY_SECONDS: i64 = 5;
pub const DEFAULT_CONTINUATION_RETRY_MAX_DELAY_SECONDS: i64 = 300;

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum DevRailContinuationTrigger {
    UserContext,
    QualityGate,
    ReviewChanges,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DevRailContinuationStatus {
    Pending,
    Claimed,
    Dispatched,
    Completed,
    Cancelled,
    Rejected,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DevRailRunKind {
    Primary,
    Retry,
    Continuation,
    FollowUp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DevRailHandoffEvidenceStatus {
    Available,
    Missing,
    Invalid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DevRailContinuationErrorCode {
    PolicyDisabled,
    InvalidSource,
    ActivityConflict,
    TaskCancelled,
    EvidenceMissing,
    EvidenceExpired,
    EvidenceMismatch,
    InputTooLarge,
    InputRejected,
    ChainLimit,
    CountLimit,
    ClaimConflict,
    DispatchUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ContinuationPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_continuation_triggers")]
    pub allowed_triggers: BTreeSet<DevRailContinuationTrigger>,
    #[serde(default = "default_continuation_max_count")]
    pub max_continuations: u16,
    #[serde(default = "default_continuation_max_chain_depth")]
    pub max_chain_depth: u16,
    #[serde(default = "default_continuation_max_context_bytes")]
    pub max_context_bytes: usize,
    #[serde(default = "default_continuation_claim_lease_seconds")]
    pub claim_lease_seconds: i64,
    #[serde(default = "default_continuation_max_dispatch_attempts")]
    pub max_dispatch_attempts: i32,
    #[serde(default = "default_continuation_retry_base_delay_seconds")]
    pub retry_base_delay_seconds: i64,
    #[serde(default = "default_continuation_retry_max_delay_seconds")]
    pub retry_max_delay_seconds: i64,
}

const fn default_continuation_max_count() -> u16 {
    DEFAULT_CONTINUATION_MAX_COUNT
}

const fn default_continuation_max_chain_depth() -> u16 {
    DEFAULT_CONTINUATION_MAX_CHAIN_DEPTH
}

const fn default_continuation_max_context_bytes() -> usize {
    DEFAULT_CONTINUATION_MAX_CONTEXT_BYTES
}

const fn default_continuation_claim_lease_seconds() -> i64 {
    DEFAULT_CONTINUATION_CLAIM_LEASE_SECONDS
}

const fn default_continuation_max_dispatch_attempts() -> i32 {
    DEFAULT_CONTINUATION_MAX_DISPATCH_ATTEMPTS
}

const fn default_continuation_retry_base_delay_seconds() -> i64 {
    DEFAULT_CONTINUATION_RETRY_BASE_DELAY_SECONDS
}

const fn default_continuation_retry_max_delay_seconds() -> i64 {
    DEFAULT_CONTINUATION_RETRY_MAX_DELAY_SECONDS
}

fn default_continuation_triggers() -> BTreeSet<DevRailContinuationTrigger> {
    [
        DevRailContinuationTrigger::UserContext,
        DevRailContinuationTrigger::QualityGate,
        DevRailContinuationTrigger::ReviewChanges,
    ]
    .into_iter()
    .collect()
}

impl Default for ContinuationPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_triggers: default_continuation_triggers(),
            max_continuations: DEFAULT_CONTINUATION_MAX_COUNT,
            max_chain_depth: DEFAULT_CONTINUATION_MAX_CHAIN_DEPTH,
            max_context_bytes: DEFAULT_CONTINUATION_MAX_CONTEXT_BYTES,
            claim_lease_seconds: DEFAULT_CONTINUATION_CLAIM_LEASE_SECONDS,
            max_dispatch_attempts: DEFAULT_CONTINUATION_MAX_DISPATCH_ATTEMPTS,
            retry_base_delay_seconds: DEFAULT_CONTINUATION_RETRY_BASE_DELAY_SECONDS,
            retry_max_delay_seconds: DEFAULT_CONTINUATION_RETRY_MAX_DELAY_SECONDS,
        }
    }
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailContinuationResponse {
    pub id: i64,
    pub task_id: i64,
    pub source_run_id: i64,
    pub root_run_id: i64,
    pub source_turn_id: String,
    #[schema(value_type = DevRailContinuationTrigger)]
    pub trigger_type: String,
    pub context_summary: String,
    pub continuation_sequence: i16,
    pub chain_depth: i16,
    #[schema(value_type = DevRailContinuationStatus)]
    pub status: String,
    pub child_run_id: Option<i64>,
    pub result_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub dispatched_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailContinuationPage {
    pub items: Vec<DevRailContinuationResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailContinuationCapabilities {
    pub can_read: bool,
    pub can_create: bool,
    pub can_cancel: bool,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDevRailContinuationRequest {
    pub idempotency_key: String,
    pub input: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailRunHandoffResponse {
    pub source_run_id: i64,
    #[schema(value_type = DevRailHandoffEvidenceStatus)]
    pub evidence_status: String,
    pub task_snapshot_digest: String,
    pub workflow_snapshot_digest: String,
    pub environment_snapshot_digest: Option<String>,
    pub base_commit: String,
    pub head_commit: Option<String>,
    pub branch_ref: Option<String>,
    pub changeset_ref: Option<String>,
    pub changeset_digest: String,
    pub error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub validated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(as = UserStatus)]
pub enum UserStatusSchema {
    Active,
    Inactive,
    Suspended,
}

#[derive(Debug, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(as = DepartmentStatus)]
pub enum DepartmentStatusSchema {
    Active,
    Inactive,
}

#[derive(Debug, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = DataScope)]
pub enum DataScopeSchema {
    All,
    Organization,
    DepartmentAndChildren,
    Department,
    #[serde(rename = "self")]
    SelfOnly,
}

#[derive(Debug, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(as = RoleColor)]
pub enum RoleColorSchema {
    Primary,
    Warning,
    Success,
    Danger,
    Neutral,
}

#[derive(Debug, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(as = PermissionType)]
pub enum PermissionTypeSchema {
    Menu,
    Button,
    Api,
}

#[derive(Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = UserSortBy)]
pub enum UserSortBySchema {
    Username,
    DisplayName,
    Email,
    Status,
    LastLoginAt,
    CreatedAt,
}

#[derive(Debug, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(as = SortDirection)]
pub enum SortDirectionSchema {
    Asc,
    Desc,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct HealthResponse {
    pub status: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ReadinessResponse {
    pub status: String,
    pub db: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserResponse {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    #[schema(required = true, nullable = true)]
    pub email: Option<String>,
    #[schema(value_type = UserStatusSchema)]
    pub status: String,
    #[schema(required = true, nullable = true)]
    pub department_id: Option<i64>,
    pub roles: Vec<String>,
    #[schema(required = true, nullable = true)]
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

pub fn user_response(row: UserRow, roles: Vec<String>) -> UserResponse {
    UserResponse {
        id: row.id,
        username: row.username,
        display_name: row.display_name,
        email: row.email,
        status: row.status,
        department_id: row.department_id,
        roles,
        last_login_at: row.last_login_at,
        created_at: row.created_at,
    }
}

pub fn user_with_roles_response(row: UserWithRolesRow) -> UserResponse {
    UserResponse {
        id: row.id,
        username: row.username,
        display_name: row.display_name,
        email: row.email,
        status: row.status,
        department_id: row.department_id,
        roles: row.roles,
        last_login_at: row.last_login_at,
        created_at: row.created_at,
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DepartmentResponse {
    pub id: i64,
    pub organization_id: i64,
    #[schema(required = true, nullable = true)]
    pub parent_id: Option<i64>,
    pub code: String,
    pub name: String,
    #[schema(value_type = DepartmentStatusSchema)]
    pub status: String,
    pub depth: i32,
    pub member_count: i64,
    pub child_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<DepartmentRow> for DepartmentResponse {
    fn from(row: DepartmentRow) -> Self {
        Self {
            id: row.id,
            organization_id: row.organization_id,
            parent_id: row.parent_id,
            code: row.code,
            name: row.name,
            status: row.status,
            depth: row.depth,
            member_count: row.member_count,
            child_count: row.child_count,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleResponse {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub category: String,
    #[schema(required = true, nullable = true)]
    pub icon: Option<String>,
    #[schema(value_type = RoleColorSchema)]
    pub color: String,
    #[schema(required = true, nullable = true)]
    pub description: Option<String>,
    #[schema(value_type = DataScopeSchema)]
    pub data_scope: String,
    pub is_active: bool,
    pub members: i64,
    pub permission_group_ids: Vec<i64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponse {
    pub id: i64,
    pub code: String,
    pub name: String,
    #[schema(value_type = PermissionTypeSchema)]
    pub r#type: String,
    #[schema(required = true, nullable = true)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionGroupResponse {
    pub id: i64,
    pub code: String,
    pub name: String,
    #[schema(required = true, nullable = true)]
    pub icon: Option<String>,
    pub permissions: Vec<PermissionResponse>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogResponse {
    pub id: i64,
    #[schema(required = true, nullable = true)]
    pub actor_user_id: Option<i64>,
    #[schema(required = true, nullable = true)]
    pub actor_username: Option<String>,
    pub action: String,
    pub target_type: String,
    #[schema(required = true, nullable = true)]
    pub target_id: Option<i64>,
    #[schema(value_type = Object)]
    pub details: Value,
    #[schema(required = true, nullable = true)]
    pub trace_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub fn audit_log_response(row: AuditLogRow) -> AuditLogResponse {
    AuditLogResponse {
        id: row.id,
        actor_user_id: row.actor_user_id,
        actor_username: row.actor_username,
        action: row.action,
        target_type: row.target_type,
        target_id: row.target_id,
        details: row.details,
        trace_id: row.trace_id,
        created_at: row.created_at,
    }
}

// ===== 请求 / 响应 =====

#[derive(Debug, Default)]
pub enum NullablePatch<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}

impl<'de, T> Deserialize<'de> for NullablePatch<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match Option::<T>::deserialize(deserializer)? {
            Some(value) => Self::Value(value),
            None => Self::Null,
        })
    }
}

pub fn nullable_patch<T: Clone>(patch: &NullablePatch<T>) -> (bool, Option<T>) {
    match patch {
        NullablePatch::Missing => (false, None),
        NullablePatch::Null => (true, None),
        NullablePatch::Value(value) => (true, Some(value.clone())),
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub remember: bool,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StepUpRequest {
    pub current_password: String,
    pub totp_code: Option<String>,
    pub scope: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StepUpResponse {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ModuleUnlockScopeSchema {
    Users,
    Roles,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModuleUnlockRequest {
    #[schema(value_type = ModuleUnlockScopeSchema)]
    pub module: String,
    pub current_password: String,
    pub totp_code: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModuleUnlockStatusResponse {
    #[schema(value_type = ModuleUnlockScopeSchema)]
    pub module: String,
    pub unlocked: bool,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum LoginStatusSchema {
    Authenticated,
    MfaRequired,
    MfaEnrollmentRequired,
}

#[derive(Debug, Clone, Copy, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum MfaMethodSchema {
    Totp,
    Passkey,
    RecoveryCode,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    pub status: LoginStatusSchema,
    pub expires_at: Option<DateTime<Utc>>,
    pub user: Option<UserResponse>,
    pub challenge_token: Option<String>,
    pub methods: Vec<MfaMethodSchema>,
    pub totp_secret: Option<String>,
    pub totp_uri: Option<String>,
    pub totp_qr_code: Option<String>,
    pub recovery_codes: Vec<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaCodeRequest {
    pub challenge_token: String,
    pub code: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaPasskeyAuthenticationStartRequest {
    pub challenge_token: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaPasskeyAuthenticationFinishRequest {
    pub challenge_token: String,
    pub credential: Value,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaTotpEnrollmentStartRequest {
    pub current_password: String,
    pub current_totp_code: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaPasskeyRegistrationStartRequest {
    pub current_password: String,
    pub totp_code: String,
    pub name: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaPasskeyRegistrationFinishRequest {
    pub challenge_token: String,
    pub credential: Value,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaFactorRevokeRequest {
    pub current_password: String,
    pub totp_code: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaEnrollmentResponse {
    pub challenge_token: String,
    pub totp_secret: String,
    pub totp_uri: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaWebauthnChallengeResponse {
    pub challenge_token: String,
    #[schema(value_type = Object)]
    pub public_key: Value,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaPasskeyResponse {
    pub id: i64,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaStatusResponse {
    pub required: bool,
    pub totp_enabled: bool,
    pub recovery_codes_remaining: i64,
    pub passkeys: Vec<MfaPasskeyResponse>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCodesResponse {
    pub codes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub keyword: Option<String>,
    #[param(value_type = Option<UserStatusSchema>)]
    pub status: Option<String>,
    pub role: Option<String>,
    #[param(value_type = Option<UserSortBySchema>)]
    pub sort_by: Option<String>,
    #[param(value_type = Option<SortDirectionSchema>)]
    pub sort_direction: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PageUser {
    pub items: Vec<UserResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub role_options: Vec<String>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct AuditLogQuery {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub keyword: Option<String>,
    pub action: Option<String>,
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PageAuditLog {
    pub items: Vec<AuditLogResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct DevRailListQuery {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub keyword: Option<String>,
    pub status: Option<String>,
    pub project_id: Option<i64>,
    pub assignee_user_id: Option<i64>,
    pub label: Option<String>,
    pub task_id: Option<i64>,
    pub run_id: Option<i64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailProjectResponse {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub default_repository_id: Option<i64>,
    pub default_environment_id: Option<i64>,
    pub notification_policy: Value,
    pub quality_gate_template: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailProjectPolicyResponse {
    pub project_id: i64,
    pub notification_policy: Value,
    pub quality_gate_template: Value,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDevRailProjectPolicyRequest {
    pub notification_policy: Option<Value>,
    pub quality_gate_template: Option<Value>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailRepositoryResponse {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub project_id: i64,
    pub name: String,
    pub remote_url: String,
    pub protocol: String,
    pub default_branch: String,
    pub credential_configured: bool,
    pub last_sync_status: String,
    pub last_head_sha: Option<String>,
    pub last_remote_branch: Option<String>,
    pub last_remote_branch_count: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailGitProviderResponse {
    pub repository_id: i64,
    pub provider: String,
    pub owner: String,
    pub repository: String,
    pub default_branch: String,
    pub credential_configured: bool,
    pub compare_url: Option<String>,
    pub pull_request_url: Option<String>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDevRailPullRequestRequest {
    pub title: String,
    pub body: Option<String>,
    pub source_branch: String,
    pub target_branch: Option<String>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDevRailBranchRequest {
    pub name: String,
    pub source_sha: String,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailBranchResponse {
    pub repository_id: i64,
    pub provider: String,
    pub name: String,
    pub source_sha: String,
    pub url: String,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteDevRailBranchRequest {
    pub name: String,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailPullRequestResponse {
    pub repository_id: i64,
    pub provider: String,
    pub number: Option<i64>,
    pub url: String,
    pub status: String,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailPullRequestWebhookRequest {
    pub provider: String,
    pub repository_id: i64,
    pub number: i64,
    pub url: String,
    pub status: String,
    pub event_id: Option<String>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncDevRailPullRequestRequest {
    pub number: i64,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct DevRailWorktreeQuery {
    pub environment_id: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailWorktreeFileResponse {
    pub status: String,
    pub path: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailWorktreeResponse {
    pub repository_id: i64,
    pub environment_id: i64,
    pub status: String,
    pub branch: Option<String>,
    pub head_sha: Option<String>,
    pub commit_summary: Option<String>,
    pub changed_files: Vec<DevRailWorktreeFileResponse>,
    pub checked_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct DevRailRepositorySyncQuery {
    pub environment_id: Option<i64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailRepositoryBranchResponse {
    pub name: String,
    pub sha: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailRepositoryCommitResponse {
    pub sha: String,
    pub summary: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailRepositorySyncResponse {
    pub repository_id: i64,
    pub status: String,
    pub default_branch: String,
    pub branches: Vec<DevRailRepositoryBranchResponse>,
    pub commits: Vec<DevRailRepositoryCommitResponse>,
    pub synced_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailEnvironmentResponse {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub project_id: i64,
    pub name: String,
    pub workspace_root: String,
    pub network_mode: String,
    pub tool_policy: Value,
    pub secret_ref_names: Vec<String>,
    pub max_duration_secs: i64,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailEnvironmentHealthResponse {
    pub environment_id: i64,
    pub status: String,
    pub enabled: bool,
    pub workspace_exists: bool,
    pub workspace_is_directory: bool,
    pub workspace_writable: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailTaskResponse {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub project_id: i64,
    pub repository_id: Option<i64>,
    pub environment_id: Option<i64>,
    pub assignee_user_id: Option<i64>,
    pub title: String,
    pub goal: String,
    pub background: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub constraints: Option<String>,
    pub priority: String,
    pub status: String,
    pub revision: i64,
    pub workflow_source: String,
    pub workflow_version: String,
    pub workflow_digest: String,
    pub continuation_policy: ContinuationPolicy,
    pub continuation_capabilities: DevRailContinuationCapabilities,
    pub scheduler_attempt: i32,
    pub scheduler_retry_count: i32,
    pub scheduler_max_attempts: i32,
    pub scheduler_retry_at: Option<DateTime<Utc>>,
    pub scheduler_last_error: Option<String>,
    pub creation_source: String,
    pub source_task_id: Option<i64>,
    pub source_run_id: Option<i64>,
    pub followup_depth: i16,
    pub blocked_reason: Option<String>,
    pub prerequisites: Vec<DevRailTaskDependencyResponse>,
    pub dependents: Vec<DevRailTaskDependentResponse>,
    pub labels: Value,
    pub due_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DevRailDependencyAction {
    Wait,
    Skip,
    Fail,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailTaskDependencyResponse {
    pub id: i64,
    pub task_id: i64,
    pub prerequisite_task_id: i64,
    pub prerequisite_title: String,
    pub prerequisite_status: String,
    pub failure_action: String,
    pub cancelled_action: String,
    pub timeout_action: String,
    pub creation_source: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailTaskDependentResponse {
    pub id: i64,
    pub task_id: i64,
    pub task_title: String,
    pub task_status: String,
    pub failure_action: String,
    pub cancelled_action: String,
    pub timeout_action: String,
    pub creation_source: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailTaskRelationsResponse {
    pub task_id: i64,
    pub revision: i64,
    pub blocked_reason: Option<String>,
    pub prerequisites: Vec<DevRailTaskDependencyResponse>,
    pub dependents: Vec<DevRailTaskDependentResponse>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailTaskEventResponse {
    pub cursor: i64,
    pub event_type: String,
    pub payload: Value,
    pub summary: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailTaskEventPage {
    pub items: Vec<DevRailTaskEventResponse>,
    pub next_cursor: Option<i64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailProjectMemberResponse {
    pub id: i64,
    pub project_id: i64,
    pub user_id: i64,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailProjectMemberPage {
    pub items: Vec<DevRailProjectMemberResponse>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddDevRailProjectMemberRequest {
    pub user_id: i64,
    pub role: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailRunResponse {
    pub id: i64,
    pub task_id: i64,
    pub snapshot_id: i64,
    pub idempotency_key: String,
    pub attempt: i32,
    pub task_revision: i64,
    pub workflow_source: String,
    pub workflow_version: String,
    pub workflow_digest: String,
    pub actor_type: String,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub last_event_at: Option<DateTime<Utc>>,
    pub retry_reason: Option<String>,
    pub parent_run_id: Option<i64>,
    pub parent_turn_id: Option<String>,
    #[schema(value_type = DevRailRunKind)]
    pub run_kind: String,
    pub root_run_id: Option<i64>,
    pub continuation_sequence: Option<i16>,
    pub continuation_request_id: Option<i64>,
    #[schema(value_type = Option<DevRailHandoffEvidenceStatus>)]
    pub handoff_evidence_status: Option<String>,
    pub handoff_error_code: Option<String>,
    pub cleanup_status: String,
    pub branch_name: Option<String>,
    pub branch_expires_at: Option<DateTime<Utc>>,
    pub status: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub harness_version: Option<String>,
    pub model_id: Option<String>,
    pub cwd: String,
    pub policy: Value,
    pub startup_args_summary: Value,
    pub exit_reason: Option<String>,
    pub exit_code: Option<i32>,
    pub stderr_summary: Option<String>,
    pub trace_id: Option<String>,
    pub recovery_suggestion: Option<String>,
    pub recovery_attempts: i32,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DevRailWorkspaceLifecycle {
    Preparing,
    Ready,
    Running,
    CleanupPending,
    CleanupFailed,
    Cleaned,
    Orphaned,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailTaskWorkspaceResponse {
    pub id: i64,
    pub task_id: i64,
    pub run_id: Option<i64>,
    pub attempt: i32,
    pub relative_id: String,
    pub base_commit: Option<String>,
    pub branch_name: Option<String>,
    pub workflow_version: Option<String>,
    pub workflow_digest: Option<String>,
    pub environment_version: Option<String>,
    pub tool_versions: Value,
    pub snapshot_digest: Option<String>,
    pub lifecycle_status: String,
    pub cleanup_status: String,
    pub cleanup_attempts: i32,
    pub next_cleanup_at: Option<DateTime<Utc>>,
    pub last_hook: Option<String>,
    pub diagnostic_ref: Option<String>,
    pub error_summary: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub cleaned_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RebuildDevRailTaskWorkspaceRequest {
    pub snapshot_digest: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailRunEventResponse {
    pub cursor: i64,
    pub event_type: String,
    pub source_event_id: Option<String>,
    pub payload: Value,
    pub summary: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailRunPage {
    pub items: Vec<DevRailRunResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailRunEventPage {
    pub items: Vec<DevRailRunEventResponse>,
    pub next_cursor: Option<i64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailChangesetResponse {
    pub run_id: i64,
    pub files: Vec<DevRailChangeFileResponse>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailPatchExportResponse {
    pub run_id: i64,
    pub file_name: String,
    pub content: String,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailChangeFileResponse {
    pub path: String,
    pub status: String,
    pub additions: Option<i64>,
    pub deletions: Option<i64>,
    pub summary: Option<String>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailQualityGatePage {
    pub run_id: i64,
    pub items: Vec<DevRailQualityGateResponse>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailQualityGateResponse {
    pub name: String,
    pub status: String,
    pub command_summary: Option<String>,
    pub executor_version: Option<String>,
    pub log_ref: Option<String>,
    pub exit_code: Option<i64>,
    pub duration_ms: Option<i64>,
    pub summary: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailQualityGateLogPage {
    pub run_id: i64,
    pub log_ref: String,
    pub name: String,
    pub status: String,
    pub lines: Vec<String>,
    pub next_cursor: Option<i64>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct QualityGateLogQuery {
    pub log_ref: String,
    pub after_cursor: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DevRailNotificationRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub recipient_user_id: i64,
    pub event_type: String,
    pub level: String,
    pub title: String,
    pub summary: String,
    pub resource_type: Option<String>,
    pub resource_id: Option<i64>,
    pub deep_link: Option<String>,
    pub source_key: String,
    pub read_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailNotificationResponse {
    pub id: i64,
    pub event_type: String,
    pub level: String,
    pub title: String,
    pub summary: String,
    pub resource_type: Option<String>,
    pub resource_id: Option<i64>,
    pub deep_link: Option<String>,
    pub read_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailNotificationPage {
    pub items: Vec<DevRailNotificationResponse>,
    pub total: i64,
    pub unread: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DevRailNotificationPreferencesRow {
    pub organization_id: i64,
    pub user_id: i64,
    pub in_app_enabled: bool,
    pub push_enabled: bool,
    pub event_types: Value,
    pub quiet_hours: Value,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailNotificationPreferencesResponse {
    pub in_app_enabled: bool,
    pub push_enabled: bool,
    pub push_supported: bool,
    pub event_types: Vec<String>,
    pub quiet_hours: Value,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDevRailNotificationPreferencesRequest {
    pub in_app_enabled: Option<bool>,
    pub push_enabled: Option<bool>,
    pub event_types: Option<Vec<String>>,
    pub quiet_hours: Option<Value>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DevRailPushDeviceRow {
    pub id: i64,
    pub device_name: String,
    pub platform: String,
    pub browser: Option<String>,
    pub timezone: Option<String>,
    pub client_version: Option<String>,
    pub endpoint_fingerprint: String,
    pub status: String,
    pub last_active_at: DateTime<Utc>,
    pub last_error: Option<String>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DevRailPushDispatchRow {
    pub delivery_id: i64,
    pub outbox_event_id: i64,
    pub device_id: i64,
    pub user_id: i64,
    pub endpoint_ciphertext: Vec<u8>,
    pub keys_ciphertext: Vec<u8>,
    pub title: String,
    pub summary: String,
    pub deep_link: Option<String>,
    pub notification_id: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailPushDeviceResponse {
    pub id: i64,
    pub device_name: String,
    pub platform: String,
    pub browser: Option<String>,
    pub timezone: Option<String>,
    pub client_version: Option<String>,
    pub endpoint_fingerprint: String,
    pub status: String,
    pub last_active_at: DateTime<Utc>,
    pub last_error: Option<String>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailPushConfigResponse {
    pub enabled: bool,
    pub public_key: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegisterDevRailPushDeviceRequest {
    pub device_name: String,
    pub platform: String,
    pub browser: Option<String>,
    pub timezone: Option<String>,
    pub client_version: Option<String>,
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailApprovalResponse {
    pub id: i64,
    pub run_id: i64,
    pub event_id: Option<i64>,
    pub idempotency_key: String,
    pub tool_name: String,
    pub args_summary: Value,
    pub cwd: String,
    pub impact_scope: Option<String>,
    pub risk_level: String,
    pub requested_by: i64,
    pub decided_by: Option<i64>,
    pub status: String,
    pub decision_reason: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub policy_version: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailApprovalPage {
    pub items: Vec<DevRailApprovalResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailApprovalDecisionRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailProjectPage {
    pub items: Vec<DevRailProjectResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailRepositoryPage {
    pub items: Vec<DevRailRepositoryResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailEnvironmentPage {
    pub items: Vec<DevRailEnvironmentResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailTaskPage {
    pub items: Vec<DevRailTaskResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DevRailReviewRow {
    pub id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub task_id: i64,
    pub run_id: i64,
    pub requested_by: i64,
    pub reviewer_user_id: i64,
    pub status: String,
    pub summary: Option<String>,
    pub decision_reason: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailReviewResponse {
    pub id: i64,
    pub task_id: i64,
    pub run_id: i64,
    pub requested_by: i64,
    pub reviewer_user_id: i64,
    pub status: String,
    pub summary: Option<String>,
    pub decision_reason: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailReviewPage {
    pub items: Vec<DevRailReviewResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDevRailReviewRequest {
    pub run_id: i64,
    pub reviewer_user_id: i64,
    pub summary: Option<String>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecideDevRailReviewRequest {
    pub decision: String,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DevRailReviewCommentRow {
    pub id: i64,
    pub organization_id: i64,
    pub review_id: i64,
    pub author_user_id: i64,
    pub file_path: String,
    pub line_start: Option<i32>,
    pub line_end: Option<i32>,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailReviewCommentResponse {
    pub id: i64,
    pub review_id: i64,
    pub author_user_id: i64,
    pub file_path: String,
    pub line_start: Option<i32>,
    pub line_end: Option<i32>,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDevRailReviewCommentRequest {
    pub file_path: String,
    pub line_start: Option<i32>,
    pub line_end: Option<i32>,
    pub body: String,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDevRailReviewCommentRequest {
    pub body: String,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncDevRailExternalReviewRequest {
    pub project_id: i64,
    pub repository_id: i64,
    pub number: i64,
}
#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailExternalReviewCommentResponse {
    pub id: i64,
    pub review_id: i64,
    pub provider: String,
    pub external_id: String,
    pub file_path: String,
    pub line_start: Option<i32>,
    pub line_end: Option<i32>,
    pub body: String,
    pub author_name: String,
    pub external_created_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub resolved: bool,
    pub deleted_at: Option<DateTime<Utc>>,
    pub changeset_matched: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailTaskCommentResponse {
    pub id: i64,
    pub task_id: i64,
    pub author_user_id: i64,
    pub author_username: String,
    pub author_display_name: String,
    pub body: String,
    pub mentions: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
    pub deleted: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailTaskCommentPage {
    pub items: Vec<DevRailTaskCommentResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDevRailTaskCommentRequest {
    pub body: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDevRailTaskCommentRequest {
    pub body: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDevRailProjectRequest {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub department_id: Option<i64>,
    pub notification_policy: Option<Value>,
    pub quality_gate_template: Option<Value>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDevRailProjectRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<String>, nullable = true)]
    pub description: NullablePatch<String>,
    #[serde(default)]
    #[schema(value_type = Option<i64>, nullable = true)]
    pub department_id: NullablePatch<i64>,
    pub status: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<i64>, nullable = true)]
    pub default_repository_id: NullablePatch<i64>,
    #[serde(default)]
    #[schema(value_type = Option<i64>, nullable = true)]
    pub default_environment_id: NullablePatch<i64>,
    pub notification_policy: Option<Value>,
    pub quality_gate_template: Option<Value>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDevRailRepositoryRequest {
    pub name: String,
    pub remote_url: String,
    pub default_branch: Option<String>,
    pub credential_ref: Option<String>,
    pub department_id: Option<i64>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDevRailRepositoryRequest {
    pub name: Option<String>,
    pub remote_url: Option<String>,
    pub default_branch: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<String>, nullable = true)]
    pub credential_ref: NullablePatch<String>,
    pub status: Option<String>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDevRailEnvironmentRequest {
    pub name: String,
    pub workspace_root: String,
    pub network_mode: Option<String>,
    pub tool_policy: Option<Value>,
    pub secret_ref_names: Option<Vec<String>>,
    pub max_duration_secs: Option<i64>,
    pub enabled: Option<bool>,
    pub department_id: Option<i64>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDevRailEnvironmentRequest {
    pub name: Option<String>,
    pub workspace_root: Option<String>,
    pub network_mode: Option<String>,
    pub tool_policy: Option<Value>,
    pub secret_ref_names: Option<Vec<String>>,
    pub max_duration_secs: Option<i64>,
    pub enabled: Option<bool>,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDevRailTaskRequest {
    pub title: String,
    pub goal: String,
    pub background: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub constraints: Option<String>,
    pub priority: Option<String>,
    pub assignee_user_id: Option<i64>,
    pub labels: Option<Vec<String>>,
    pub due_at: Option<DateTime<Utc>>,
    pub department_id: Option<i64>,
    pub repository_id: Option<i64>,
    pub environment_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DevRailTaskDependencyInput {
    pub prerequisite_task_id: i64,
    pub failure_action: Option<DevRailDependencyAction>,
    pub cancelled_action: Option<DevRailDependencyAction>,
    pub timeout_action: Option<DevRailDependencyAction>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceDevRailTaskDependenciesRequest {
    pub revision: i64,
    pub idempotency_key: String,
    pub dependencies: Vec<DevRailTaskDependencyInput>,
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDevRailFollowupTaskRequest {
    pub idempotency_key: String,
    pub title: String,
    pub goal: String,
    pub background: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub constraints: Option<String>,
    pub priority: Option<String>,
    pub labels: Option<Vec<String>>,
    pub due_at: Option<DateTime<Utc>>,
    pub dependencies: Option<Vec<DevRailTaskDependencyInput>>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevRailFollowupTaskResponse {
    pub request_id: i64,
    pub source_task_id: i64,
    pub source_run_id: i64,
    pub task: DevRailTaskResponse,
    pub replayed: bool,
}
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDevRailTaskRequest {
    pub title: Option<String>,
    pub goal: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<String>, nullable = true)]
    pub background: NullablePatch<String>,
    #[serde(default)]
    #[schema(value_type = Option<String>, nullable = true)]
    pub acceptance_criteria: NullablePatch<String>,
    #[serde(default)]
    #[schema(value_type = Option<String>, nullable = true)]
    pub constraints: NullablePatch<String>,
    #[serde(default)]
    pub priority: Option<String>,
    pub status: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<i64>, nullable = true)]
    pub assignee_user_id: NullablePatch<i64>,
    #[serde(default)]
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    #[schema(value_type = Option<String>, nullable = true)]
    pub due_at: NullablePatch<DateTime<Utc>>,
    #[serde(default)]
    #[schema(value_type = Option<i64>, nullable = true)]
    pub repository_id: NullablePatch<i64>,
    #[serde(default)]
    #[schema(value_type = Option<i64>, nullable = true)]
    pub environment_id: NullablePatch<i64>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDevRailRunRequest {
    pub environment_id: i64,
    pub idempotency_key: String,
    pub model_id: Option<String>,
    pub input: Option<String>,
    pub branch_name: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RetryDevRailRunRequest {
    pub idempotency_key: String,
    pub input: Option<String>,
    pub resume_from_turn_id: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub display_name: String,
    pub email: Option<String>,
    #[schema(value_type = Option<UserStatusSchema>)]
    pub status: Option<String>,
    pub department_id: Option<i64>,
    pub role_ids: Option<Vec<i64>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserRequest {
    pub display_name: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<String>, nullable = true)]
    pub email: NullablePatch<String>,
    #[schema(value_type = Option<UserStatusSchema>)]
    pub status: Option<String>,
    pub department_id: Option<i64>,
    pub password: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDepartmentRequest {
    pub parent_id: i64,
    pub code: String,
    pub name: String,
    #[schema(value_type = Option<DepartmentStatusSchema>)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDepartmentRequest {
    pub parent_id: Option<i64>,
    pub code: Option<String>,
    pub name: Option<String>,
    #[schema(value_type = Option<DepartmentStatusSchema>)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignRolesRequest {
    pub role_ids: Vec<i64>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchUserIdsRequest {
    pub user_ids: Vec<i64>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchAssignRolesRequest {
    pub user_ids: Vec<i64>,
    pub role_ids: Vec<i64>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoleRequest {
    pub code: String,
    pub name: String,
    pub category: Option<String>,
    pub icon: Option<String>,
    #[schema(value_type = Option<RoleColorSchema>)]
    pub color: Option<String>,
    pub description: Option<String>,
    #[schema(value_type = Option<DataScopeSchema>)]
    pub data_scope: Option<String>,
    pub permission_ids: Option<Vec<i64>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub category: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<String>, nullable = true)]
    pub icon: NullablePatch<String>,
    #[schema(value_type = Option<RoleColorSchema>)]
    pub color: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<String>, nullable = true)]
    pub description: NullablePatch<String>,
    #[schema(value_type = Option<DataScopeSchema>)]
    pub data_scope: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RolePermissions {
    pub permission_ids: Vec<i64>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRolePermissionsRequest {
    pub permission_ids: Vec<i64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionCodes {
    pub codes: Vec<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DashboardStats {
    pub total_users: i64,
    pub active_users: i64,
    pub total_roles: i64,
    pub total_permissions: i64,
    pub suspended_users: i64,
}

impl From<DashboardStatsRow> for DashboardStats {
    fn from(row: DashboardStatsRow) -> Self {
        Self {
            total_users: row.total_users,
            active_users: row.active_users,
            total_roles: row.total_roles,
            total_permissions: row.total_permissions,
            suspended_users: row.suspended_users,
        }
    }
}

#[cfg(test)]
mod dependency_contract_tests {
    use super::*;

    #[test]
    fn dependency_actions_use_stable_wire_values() {
        assert_eq!(
            serde_json::to_string(&DevRailDependencyAction::Wait).unwrap(),
            "\"wait\""
        );
        assert_eq!(
            serde_json::to_string(&DevRailDependencyAction::Skip).unwrap(),
            "\"skip\""
        );
        assert_eq!(
            serde_json::to_string(&DevRailDependencyAction::Fail).unwrap(),
            "\"fail\""
        );
        assert!(serde_json::from_str::<DevRailDependencyAction>("\"blocked\"").is_err());
    }

    #[test]
    fn followup_input_rejects_scope_and_authority_fields() {
        let result =
            serde_json::from_value::<CreateDevRailFollowupTaskRequest>(serde_json::json!({
                "idempotencyKey": "event-1",
                "title": "后续任务",
                "goal": "验证",
                "organizationId": 42
            }));
        assert!(result.is_err());
    }

    #[test]
    fn continuation_defaults_are_closed_and_bounded() {
        let policy = ContinuationPolicy::default();
        assert!(!policy.enabled);
        assert_eq!(policy.max_continuations, DEFAULT_CONTINUATION_MAX_COUNT);
        assert_eq!(policy.max_chain_depth, DEFAULT_CONTINUATION_MAX_CHAIN_DEPTH);
        assert_eq!(
            policy.max_context_bytes,
            DEFAULT_CONTINUATION_MAX_CONTEXT_BYTES
        );
        assert_eq!(policy.allowed_triggers.len(), 3);
    }

    #[test]
    fn continuation_wire_enums_reject_unknown_values() {
        assert_eq!(
            serde_json::to_string(&DevRailContinuationStatus::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(
            serde_json::to_string(&DevRailRunKind::Continuation).unwrap(),
            "\"continuation\""
        );
        assert!(serde_json::from_str::<DevRailContinuationStatus>("\"started\"").is_err());
        assert!(serde_json::from_str::<DevRailContinuationTrigger>("\"manual\"").is_err());
    }
}
