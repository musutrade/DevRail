//! Scoped continuation request persistence and claim coordination.

use crate::access::ActorContext;
use crate::models::DevRailContinuationRequestRow;
use crate::repositories::{audit_logs, devrail, devrail_notifications};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{AssertSqlSafe, PgConnection, PgPool, Row};
use uuid::Uuid;

const REQUEST_COLUMNS: &str = "r.id, r.organization_id, r.department_id, r.owner_user_id, r.project_id, r.task_id, r.source_run_id, r.root_run_id, r.source_turn_id, r.requested_by_user_id, r.trigger_type, r.evidence_ref, r.evidence_digest, r.evidence_observed_at, r.evidence_expires_at, r.changeset_digest, r.redacted_context, r.context_summary, r.input_digest, r.idempotency_key, r.continuation_sequence, r.chain_depth, r.prior_task_status, r.policy_version, r.policy_snapshot, r.status, r.status_version, r.claim_owner, r.claim_token, r.claim_expires_at, r.dispatch_attempts, r.next_attempt_at, r.child_run_id, r.result_code, r.created_at, r.updated_at, r.claimed_at, r.dispatched_at, r.completed_at, r.cancelled_at, r.rejected_at";

fn visible_departments_cte() -> &'static str {
    "WITH RECURSIVE visible_departments AS (
         SELECT id FROM departments WHERE id = $4 AND organization_id = $2
         UNION
         SELECT child.id FROM departments child
         JOIN visible_departments parent ON child.parent_id = parent.id
         WHERE child.organization_id = $2
     )"
}

fn scoped_request(alias: &str) -> String {
    format!(
        "{alias}.organization_id = $2 AND ($1 = 'all' OR $1 = 'organization' OR ($1 = 'self' AND {alias}.owner_user_id = $3) OR ($1 = 'department' AND {alias}.department_id = $4) OR ($1 = 'department_and_children' AND {alias}.department_id IN (SELECT id FROM visible_departments)))"
    )
}

pub(crate) struct NewContinuation<'a> {
    pub actor: &'a ActorContext,
    pub project_id: i64,
    pub task_id: i64,
    pub source_run_id: i64,
    pub root_run_id: i64,
    pub source_turn_id: &'a str,
    pub requested_by_user_id: i64,
    pub trigger_type: &'a str,
    pub evidence_ref: &'a str,
    pub evidence_digest: &'a str,
    pub evidence_observed_at: DateTime<Utc>,
    pub evidence_expires_at: Option<DateTime<Utc>>,
    pub changeset_digest: Option<&'a str>,
    pub redacted_context: &'a str,
    pub context_summary: &'a str,
    pub input_digest: &'a str,
    pub idempotency_key: &'a str,
    pub continuation_sequence: i16,
    pub chain_depth: i16,
    pub prior_task_status: &'a str,
    pub expected_task_revision: i64,
    pub policy_version: &'a str,
    pub policy_snapshot: &'a Value,
}

pub struct NewRunHandoff<'a> {
    pub actor: &'a ActorContext,
    pub project_id: i64,
    pub task_id: i64,
    pub source_run_id: i64,
    pub task_snapshot_id: i64,
    pub repository_id: i64,
    pub environment_id: Option<i64>,
    pub task_snapshot_digest: &'a str,
    pub workflow_snapshot_digest: &'a str,
    pub environment_snapshot_digest: Option<&'a str>,
    pub repository_identity: &'a str,
    pub repository_identity_digest: &'a str,
    pub base_commit: &'a str,
    pub head_commit: Option<&'a str>,
    pub branch_ref: Option<&'a str>,
    pub changeset_ref: Option<&'a str>,
    pub changeset_digest: &'a str,
    pub tool_versions: &'a Value,
    pub evidence_status: &'a str,
    pub error_code: Option<&'a str>,
    pub validated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct HandoffSourceRow {
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub project_id: i64,
    pub task_id: i64,
    pub source_run_id: i64,
    pub task_snapshot_id: i64,
    pub repository_id: i64,
    pub repository_name: String,
    pub repository_remote_url: String,
    pub environment_id: Option<i64>,
    pub task_snapshot_digest: String,
    pub workflow_snapshot_digest: String,
    pub environment_snapshot: Value,
    pub workspace_relative_path: String,
    pub workspace_base_commit: Option<String>,
    pub workspace_branch_name: Option<String>,
    pub tool_versions: Value,
}

const HANDOFF_COLUMNS_SCOPED: &str = "h.id, h.organization_id, h.department_id, h.owner_user_id, h.project_id, h.task_id, h.source_run_id, h.task_snapshot_id, h.repository_id, h.environment_id, h.task_snapshot_digest, h.workflow_snapshot_digest, h.environment_snapshot_digest, h.repository_identity, h.repository_identity_digest, h.base_commit, h.head_commit, h.branch_ref, h.changeset_ref, h.changeset_digest, h.tool_versions, h.evidence_status, h.error_code, h.created_at, h.validated_at";
const HANDOFF_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, project_id, task_id, source_run_id, task_snapshot_id, repository_id, environment_id, task_snapshot_digest, workflow_snapshot_digest, environment_snapshot_digest, repository_identity, repository_identity_digest, base_commit, head_commit, branch_ref, changeset_ref, changeset_digest, tool_versions, evidence_status, error_code, created_at, validated_at";

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn safe_ref(value: Option<&str>) -> bool {
    value.is_none_or(|value| {
        !value.is_empty()
            && !value.starts_with('/')
            && !value.starts_with("~/")
            && !value.contains("..")
            && !value.contains('\\')
    })
}

fn sensitive_text(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.contains("begin rsa private key") || lower.contains("begin openssh private key") {
        return true;
    }
    value.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with('/')
            || trimmed.starts_with("~/")
            || trimmed.contains(":\\")
            || [
                "password",
                "passwd",
                "token",
                "authorization",
                "cookie",
                "database_url",
                "private_key",
                "secret",
            ]
            .iter()
            .any(|marker| {
                let lower = trimmed.to_ascii_lowercase();
                lower.contains(marker) && (trimmed.contains('=') || trimmed.contains(':'))
            })
    })
}

fn sensitive_json(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            let key = key.to_ascii_lowercase();
            [
                "password",
                "passwd",
                "token",
                "authorization",
                "cookie",
                "database_url",
                "private_key",
                "secret",
            ]
            .iter()
            .any(|marker| key.contains(marker))
                || sensitive_json(value)
        }),
        Value::Array(values) => values.iter().any(sensitive_json),
        Value::String(value) => sensitive_text(value),
        _ => false,
    }
}

fn valid_commit(value: &str) -> bool {
    (7..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn validate_handoff(input: &NewRunHandoff<'_>) -> Result<(), sqlx::Error> {
    for digest in [
        input.task_snapshot_digest,
        input.workflow_snapshot_digest,
        input.repository_identity_digest,
        input.changeset_digest,
    ] {
        if !valid_digest(digest) {
            return Err(sqlx::Error::Protocol("handoff 摘要格式无效".to_string()));
        }
    }
    if let Some(digest) = input.environment_snapshot_digest {
        if !valid_digest(digest) {
            return Err(sqlx::Error::Protocol(
                "handoff 环境摘要格式无效".to_string(),
            ));
        }
    }
    if !valid_commit(input.base_commit)
        || input.head_commit.is_some_and(|value| !valid_commit(value))
        || !safe_ref(input.head_commit)
        || !safe_ref(input.branch_ref)
        || !safe_ref(input.changeset_ref)
        || input.head_commit.is_none() && input.changeset_ref.is_none()
    {
        return Err(sqlx::Error::Protocol("handoff 证据引用无效".to_string()));
    }
    if matches!(input.evidence_status, "available")
        && (input.validated_at.is_none() || input.error_code.is_some())
    {
        return Err(sqlx::Error::Protocol(
            "handoff 可用状态缺少校验事实".to_string(),
        ));
    }
    if !matches!(input.evidence_status, "available" | "missing" | "invalid") {
        return Err(sqlx::Error::Protocol("handoff 证据状态无效".to_string()));
    }
    if input.repository_identity.is_empty()
        || input.repository_identity.len() > 256
        || sensitive_text(input.repository_identity)
        || sensitive_json(input.tool_versions)
    {
        return Err(sqlx::Error::Protocol(
            "handoff 证据包含敏感信息或受控绝对路径".to_string(),
        ));
    }
    Ok(())
}

pub async fn handoff_source(
    pool: &PgPool,
    source_run_id: i64,
) -> Result<Option<HandoffSourceRow>, sqlx::Error> {
    sqlx::query_as::<_, HandoffSourceRow>(
        "SELECT r.organization_id, r.department_id, r.owner_user_id,
                t.project_id, t.id AS task_id, r.id AS source_run_id,
                r.snapshot_id AS task_snapshot_id, repo.id AS repository_id,
                repo.name AS repository_name,
                repo.remote_url AS repository_remote_url, t.environment_id,
                t.dispatch_snapshot_digest AS task_snapshot_digest,
                r.workflow_digest AS workflow_snapshot_digest,
                COALESCE(t.dispatch_snapshot->'environment','{}'::jsonb)
                    AS environment_snapshot,
                w.relative_path AS workspace_relative_path,
                w.base_commit AS workspace_base_commit,
                w.branch_name AS workspace_branch_name,
                w.tool_versions
         FROM devrail_runs r
         JOIN devrail_tasks t
           ON t.id=r.task_id AND t.organization_id=r.organization_id
         JOIN devrail_repositories repo
           ON repo.id=t.repository_id AND repo.organization_id=t.organization_id
         JOIN devrail_task_workspaces w
           ON w.run_id=r.id AND w.organization_id=r.organization_id
         WHERE r.id=$1",
    )
    .bind(source_run_id)
    .fetch_optional(pool)
    .await
}

pub async fn find_handoff(
    pool: &PgPool,
    actor: &ActorContext,
    source_run_id: i64,
) -> Result<Option<crate::models::DevRailRunHandoffRow>, sqlx::Error> {
    let sql = format!(
        "{} SELECT {HANDOFF_COLUMNS_SCOPED} FROM devrail_run_handoffs h WHERE h.source_run_id=$5 AND {}",
        visible_departments_cte(),
        scoped_request("h")
    );
    sqlx::query_as::<_, crate::models::DevRailRunHandoffRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(source_run_id)
        .fetch_optional(pool)
        .await
}

pub async fn find_handoff_by_request(
    pool: &PgPool,
    request_id: i64,
    claim_token: Uuid,
) -> Result<Option<crate::models::DevRailRunHandoffRow>, sqlx::Error> {
    sqlx::query_as::<_, crate::models::DevRailRunHandoffRow>(AssertSqlSafe(format!(
        "SELECT {HANDOFF_COLUMNS_SCOPED}
         FROM devrail_run_handoffs h
         JOIN devrail_continuation_requests r
           ON r.source_run_id=h.source_run_id
          AND r.organization_id=h.organization_id
         WHERE r.id=$1 AND r.status='claimed' AND r.claim_token=$2
           AND r.claim_expires_at>now()
           AND h.evidence_status='available' AND h.validated_at IS NOT NULL"
    )))
    .bind(request_id)
    .bind(claim_token)
    .fetch_optional(pool)
    .await
}

pub async fn create_handoff(
    connection: &mut PgConnection,
    input: &NewRunHandoff<'_>,
) -> Result<(crate::models::DevRailRunHandoffRow, bool), sqlx::Error> {
    validate_handoff(input)?;
    let existing = sqlx::query_as::<_, crate::models::DevRailRunHandoffRow>(AssertSqlSafe(
        format!(
            "SELECT {HANDOFF_COLUMNS_SCOPED} FROM devrail_run_handoffs h WHERE h.organization_id=$1 AND h.source_run_id=$2 FOR UPDATE"
        ),
    ))
    .bind(input.actor.organization_id)
    .bind(input.source_run_id)
    .fetch_optional(&mut *connection)
    .await?;
    if let Some(existing) = existing {
        let matches = existing.project_id == input.project_id
            && existing.task_id == input.task_id
            && existing.task_snapshot_id == input.task_snapshot_id
            && existing.repository_id == input.repository_id
            && existing.environment_id == input.environment_id
            && existing.task_snapshot_digest == input.task_snapshot_digest
            && existing.workflow_snapshot_digest == input.workflow_snapshot_digest
            && existing.environment_snapshot_digest.as_deref() == input.environment_snapshot_digest
            && existing.repository_identity == input.repository_identity
            && existing.repository_identity_digest == input.repository_identity_digest
            && existing.base_commit == input.base_commit
            && existing.head_commit.as_deref() == input.head_commit
            && existing.branch_ref.as_deref() == input.branch_ref
            && existing.changeset_ref.as_deref() == input.changeset_ref
            && existing.changeset_digest == input.changeset_digest
            && existing.tool_versions == *input.tool_versions
            && existing.evidence_status == input.evidence_status
            && existing.error_code.as_deref() == input.error_code
            && existing.validated_at.is_some() == input.validated_at.is_some();
        if !matches {
            return Err(sqlx::Error::Protocol(
                "来源运行 handoff 证据不可变且摘要不匹配".to_string(),
            ));
        }
        return Ok((existing, false));
    }
    let sql = format!(
        "INSERT INTO devrail_run_handoffs (organization_id,department_id,owner_user_id,project_id,task_id,source_run_id,task_snapshot_id,repository_id,environment_id,task_snapshot_digest,workflow_snapshot_digest,environment_snapshot_digest,repository_identity,repository_identity_digest,base_commit,head_commit,branch_ref,changeset_ref,changeset_digest,tool_versions,evidence_status,error_code,validated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23) RETURNING {HANDOFF_COLUMNS}"
    );
    let row = sqlx::query_as::<_, crate::models::DevRailRunHandoffRow>(AssertSqlSafe(sql))
        .bind(input.actor.organization_id)
        .bind(input.actor.department_id)
        .bind(input.actor.user_id)
        .bind(input.project_id)
        .bind(input.task_id)
        .bind(input.source_run_id)
        .bind(input.task_snapshot_id)
        .bind(input.repository_id)
        .bind(input.environment_id)
        .bind(input.task_snapshot_digest)
        .bind(input.workflow_snapshot_digest)
        .bind(input.environment_snapshot_digest)
        .bind(input.repository_identity)
        .bind(input.repository_identity_digest)
        .bind(input.base_commit)
        .bind(input.head_commit)
        .bind(input.branch_ref)
        .bind(input.changeset_ref)
        .bind(input.changeset_digest)
        .bind(input.tool_versions)
        .bind(input.evidence_status)
        .bind(input.error_code)
        .bind(input.validated_at)
        .fetch_one(&mut *connection)
        .await?;
    Ok((row, true))
}

pub(crate) async fn create(
    connection: &mut PgConnection,
    input: &NewContinuation<'_>,
) -> Result<(DevRailContinuationRequestRow, bool), sqlx::Error> {
    // Idempotent replays must win before validating the source task again. A
    // successful first request projects the task to continuation_pending, so
    // checking the source first would turn an otherwise safe replay into a
    // misleading state conflict.
    let existing_sql = format!(
        "{} SELECT {REQUEST_COLUMNS} FROM devrail_continuation_requests r WHERE r.organization_id=$2 AND r.task_id=$5 AND (r.idempotency_key=$6 OR (r.source_run_id=$7 AND r.trigger_type=$8 AND r.evidence_ref=$9)) AND {} ORDER BY CASE WHEN r.idempotency_key=$6 THEN 0 ELSE 1 END,r.id LIMIT 1 FOR UPDATE",
        visible_departments_cte(),
        scoped_request("r")
    );
    if let Some(existing) =
        sqlx::query_as::<_, DevRailContinuationRequestRow>(AssertSqlSafe(existing_sql))
            .bind(input.actor.data_scope.as_str())
            .bind(input.actor.organization_id)
            .bind(input.actor.user_id)
            .bind(input.actor.department_id)
            .bind(input.task_id)
            .bind(input.idempotency_key)
            .bind(input.source_run_id)
            .bind(input.trigger_type)
            .bind(input.evidence_ref)
            .fetch_optional(&mut *connection)
            .await?
    {
        let same_idempotency = existing.idempotency_key == input.idempotency_key;
        if existing.source_run_id != input.source_run_id
            || existing.input_digest != input.input_digest
            || existing.evidence_digest != input.evidence_digest
            || existing.trigger_type != input.trigger_type
            || same_idempotency && existing.evidence_ref != input.evidence_ref
        {
            return Err(sqlx::Error::Protocol(
                "continuation 幂等键对应不同请求".to_string(),
            ));
        }
        return Ok((existing, false));
    }

    let source_sql = format!(
        "{} SELECT t.status AS task_status, t.revision, t.department_id AS task_department_id, t.owner_user_id AS task_owner_user_id, r.status AS run_status, r.root_run_id, r.turn_id FROM devrail_tasks t JOIN devrail_runs r ON r.task_id = t.id AND r.organization_id = t.organization_id WHERE t.id=$5 AND t.organization_id=$2 AND r.id=$6 AND {} AND {} FOR UPDATE OF t, r",
        visible_departments_cte(),
        scoped_request("t"),
        scoped_request("r")
    );
    let source = sqlx::query(AssertSqlSafe(source_sql))
        .bind(input.actor.data_scope.as_str())
        .bind(input.actor.organization_id)
        .bind(input.actor.user_id)
        .bind(input.actor.department_id)
        .bind(input.task_id)
        .bind(input.source_run_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    let task_status: String = source.try_get("task_status")?;
    let task_revision: i64 = source.try_get("revision")?;
    let task_department_id: Option<i64> = source.try_get("task_department_id")?;
    let task_owner_user_id: i64 = source.try_get("task_owner_user_id")?;
    let run_status: String = source.try_get("run_status")?;
    let stored_root_run_id: Option<i64> = source.try_get("root_run_id")?;
    let stored_turn_id: Option<String> = source.try_get("turn_id")?;

    if task_status != input.prior_task_status
        || task_revision != input.expected_task_revision
        || !matches!(task_status.as_str(), "succeeded" | "failed")
        || !matches!(run_status.as_str(), "completed" | "failed")
        || stored_root_run_id != Some(input.root_run_id)
        || stored_turn_id.as_deref() != Some(input.source_turn_id)
    {
        return Err(sqlx::Error::Protocol(
            "continuation 来源状态或谱系不匹配".to_string(),
        ));
    }
    if sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM devrail_runs WHERE task_id=$1 AND status IN ('starting','active','awaiting_approval')",
    )
    .bind(input.task_id)
    .fetch_one(&mut *connection)
    .await?
        > 0
    {
        return Err(sqlx::Error::Protocol("任务存在活动运行".to_string()));
    }

    let request_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO devrail_continuation_requests (organization_id,department_id,owner_user_id,project_id,task_id,source_run_id,root_run_id,source_turn_id,requested_by_user_id,trigger_type,evidence_ref,evidence_digest,evidence_observed_at,evidence_expires_at,changeset_digest,redacted_context,context_summary,input_digest,idempotency_key,continuation_sequence,chain_depth,prior_task_status,policy_version,policy_snapshot) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24) RETURNING id",
    )
    .bind(input.actor.organization_id)
    .bind(task_department_id)
    .bind(task_owner_user_id)
    .bind(input.project_id)
    .bind(input.task_id)
    .bind(input.source_run_id)
    .bind(input.root_run_id)
    .bind(input.source_turn_id)
    .bind(input.requested_by_user_id)
    .bind(input.trigger_type)
    .bind(input.evidence_ref)
    .bind(input.evidence_digest)
    .bind(input.evidence_observed_at)
    .bind(input.evidence_expires_at)
    .bind(input.changeset_digest)
    .bind(input.redacted_context)
    .bind(input.context_summary)
    .bind(input.input_digest)
    .bind(input.idempotency_key)
    .bind(input.continuation_sequence)
    .bind(input.chain_depth)
    .bind(input.prior_task_status)
    .bind(input.policy_version)
    .bind(input.policy_snapshot)
    .fetch_one(&mut *connection)
    .await?;

    let trace = Uuid::new_v4().to_string();
    sqlx::query(
        "SELECT set_config('devrail.actor_type',$1,true),
                set_config('devrail.actor_user_id',$2,true),
                set_config('devrail.transition_reason','continuation_requested',true),
                set_config('devrail.trace_id',$3,true),
                set_config('devrail.continuation_request_id',$4,true),
                set_config('devrail.source_run_id',$5,true),
                set_config('devrail.child_run_id','',true),
                set_config('devrail.continuation_trigger_type',$6,true),
                set_config('devrail.continuation_policy_version',$7,true)",
    )
    .bind(input.actor.actor_type.as_str())
    .bind(input.actor.user_id.to_string())
    .bind(&trace)
    .bind(request_id.to_string())
    .bind(input.source_run_id.to_string())
    .bind(input.trigger_type)
    .bind(input.policy_version)
    .execute(&mut *connection)
    .await?;
    let updated = sqlx::query(
        "UPDATE devrail_tasks SET status='continuation_pending', revision=revision+1, updated_at=now() WHERE id=$1 AND organization_id=$2 AND revision=$3 AND status=$4 AND NOT EXISTS (SELECT 1 FROM devrail_runs WHERE task_id=$1 AND status IN ('starting','active','awaiting_approval'))",
    )
    .bind(input.task_id)
    .bind(input.actor.organization_id)
    .bind(input.expected_task_revision)
    .bind(input.prior_task_status)
    .execute(&mut *connection)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "任务状态在 continuation 创建期间发生变化".to_string(),
        ));
    }

    let task = sqlx::query_as::<_, crate::models::DevRailTaskRow>(AssertSqlSafe(format!(
        "SELECT {TASK_COLUMNS} FROM devrail_tasks WHERE id=$1 AND organization_id=$2"
    )))
    .bind(input.task_id)
    .bind(input.actor.organization_id)
    .fetch_one(&mut *connection)
    .await?;
    devrail::append_task_event(
        &mut *connection,
        &task,
        "continuation.created",
        &format!("continuation:{request_id}:created"),
        &serde_json::json!({
            "requestId": request_id,
            "sourceRunId": input.source_run_id,
            "triggerType": input.trigger_type,
            "sequence": input.continuation_sequence,
        }),
        "continuation 请求已创建",
    )
    .await?;
    audit_logs::record_actor(
        &mut *connection,
        input.actor,
        "devrail.continuation.create",
        "devrail_continuation_request",
        Some(request_id),
        serde_json::json!({
            "taskId": input.task_id,
            "sourceRunId": input.source_run_id,
            "triggerType": input.trigger_type,
            "sequence": input.continuation_sequence,
        }),
    )
    .await?;
    let notification_summary = format!(
        "任务 {} 已创建第 {} 次继续执行请求",
        input.task_id, input.continuation_sequence
    );
    devrail_notifications::create(
        &mut *connection,
        &devrail_notifications::NewNotification {
            organization_id: input.actor.organization_id,
            department_id: task_department_id,
            recipient_user_id: input.requested_by_user_id,
            event_type: "devrail.continuation.created",
            level: "info",
            title: "继续执行请求已创建",
            summary: &notification_summary,
            resource_type: Some("devrail_task"),
            resource_id: Some(input.task_id),
            deep_link: Some(&format!("/devrail/tasks/{}", input.task_id)),
            source_key: &format!("continuation:{request_id}:created"),
        },
    )
    .await?;
    devrail_notifications::outbox(
        &mut *connection,
        input.actor.organization_id,
        "devrail.continuation.created",
        "devrail_continuation_request",
        Some(request_id),
        &serde_json::json!({
            "notificationId": request_id,
            "eventType": "devrail.continuation.created",
            "summary": "继续执行请求已创建",
            "deepLink": format!("/devrail/tasks/{}", input.task_id),
        }),
    )
    .await?;

    let request =
        find_by_id_in_connection(&mut *connection, input.actor, request_id, input.task_id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
    Ok((request, true))
}

const TASK_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, project_id, repository_id, environment_id, assignee_user_id, title, goal, background, acceptance_criteria, constraints, priority, status, revision, dispatch_snapshot, dispatch_snapshot_digest, workflow_source, workflow_version, workflow_digest, scheduler_attempt, scheduler_retry_count, scheduler_max_attempts, scheduler_retry_at, scheduler_last_error, hook_failure_fingerprint, hook_failure_count, creation_source, source_task_id, source_run_id, followup_depth, labels, due_at, created_at, updated_at, archived_at";

pub async fn find_by_id_in_connection(
    connection: &mut PgConnection,
    actor: &ActorContext,
    id: i64,
    task_id: i64,
) -> Result<Option<DevRailContinuationRequestRow>, sqlx::Error> {
    let sql = format!(
        "{} SELECT {REQUEST_COLUMNS} FROM devrail_continuation_requests r WHERE r.id=$5 AND r.task_id=$6 AND {}",
        visible_departments_cte(),
        scoped_request("r")
    );
    sqlx::query_as::<_, DevRailContinuationRequestRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(id)
        .bind(task_id)
        .fetch_optional(connection)
        .await
}

pub async fn find_by_id(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<Option<DevRailContinuationRequestRow>, sqlx::Error> {
    let sql = format!(
        "{} SELECT {REQUEST_COLUMNS} FROM devrail_continuation_requests r WHERE r.id=$5 AND {}",
        visible_departments_cte(),
        scoped_request("r")
    );
    sqlx::query_as::<_, DevRailContinuationRequestRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: Option<i64>,
    source_run_id: Option<i64>,
    page: i64,
    page_size: i64,
) -> Result<(Vec<DevRailContinuationRequestRow>, i64), sqlx::Error> {
    let sql = format!(
        "{} SELECT {REQUEST_COLUMNS} FROM devrail_continuation_requests r WHERE ($5::bigint IS NULL OR r.task_id=$5) AND ($6::bigint IS NULL OR r.source_run_id=$6) AND {} ORDER BY r.created_at DESC,r.id DESC LIMIT $7 OFFSET $8",
        visible_departments_cte(),
        scoped_request("r")
    );
    let items = sqlx::query_as::<_, DevRailContinuationRequestRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(task_id)
        .bind(source_run_id)
        .bind(page_size)
        .bind((page - 1) * page_size)
        .fetch_all(pool)
        .await?;
    let count_sql = format!(
        "{} SELECT count(*) FROM devrail_continuation_requests r WHERE ($5::bigint IS NULL OR r.task_id=$5) AND ($6::bigint IS NULL OR r.source_run_id=$6) AND {}",
        visible_departments_cte(),
        scoped_request("r")
    );
    let total = sqlx::query_scalar::<_, i64>(AssertSqlSafe(count_sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(task_id)
        .bind(source_run_id)
        .fetch_one(pool)
        .await?;
    Ok((items, total))
}

pub async fn next_sequence(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: i64,
) -> Result<i16, sqlx::Error> {
    sqlx::query_scalar::<_, i16>(
        "SELECT (COALESCE(MAX(continuation_sequence), 0) + 1)::smallint FROM devrail_continuation_requests WHERE organization_id=$1 AND task_id=$2",
    )
    .bind(actor.organization_id)
    .bind(task_id)
    .fetch_one(pool)
    .await
}

pub(crate) async fn next_sequence_in_connection(
    connection: &mut PgConnection,
    actor: &ActorContext,
    task_id: i64,
) -> Result<i16, sqlx::Error> {
    // Serialize sequence allocation with the task row. The create operation
    // locks the same row before inserting, so concurrent requests cannot both
    // observe the same next sequence.
    sqlx::query_scalar::<_, i64>(
        "SELECT id FROM devrail_tasks WHERE id=$1 AND organization_id=$2 FOR UPDATE",
    )
    .bind(task_id)
    .bind(actor.organization_id)
    .fetch_one(&mut *connection)
    .await?;
    sqlx::query_scalar::<_, i16>(
        "SELECT (COALESCE(MAX(continuation_sequence), 0) + 1)::smallint FROM devrail_continuation_requests WHERE organization_id=$1 AND task_id=$2",
    )
    .bind(actor.organization_id)
    .bind(task_id)
    .fetch_one(&mut *connection)
    .await
}

pub async fn next_chain_depth(
    pool: &PgPool,
    actor: &ActorContext,
    root_run_id: i64,
) -> Result<i16, sqlx::Error> {
    sqlx::query_scalar::<_, i16>(
        "SELECT (COALESCE(MAX(chain_depth), 0) + 1)::smallint FROM devrail_continuation_requests WHERE organization_id=$1 AND root_run_id=$2",
    )
    .bind(actor.organization_id)
    .bind(root_run_id)
    .fetch_one(pool)
    .await
}

pub(crate) async fn next_chain_depth_in_connection(
    connection: &mut PgConnection,
    actor: &ActorContext,
    root_run_id: i64,
) -> Result<i16, sqlx::Error> {
    sqlx::query_scalar::<_, i16>(
        "SELECT (COALESCE(MAX(chain_depth), 0) + 1)::smallint FROM devrail_continuation_requests WHERE organization_id=$1 AND root_run_id=$2",
    )
    .bind(actor.organization_id)
    .bind(root_run_id)
    .fetch_one(&mut *connection)
    .await
}

pub async fn has_valid_handoff(
    pool: &PgPool,
    actor: &ActorContext,
    source_run_id: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM devrail_run_handoffs WHERE organization_id=$1 AND source_run_id=$2 AND evidence_status='available' AND validated_at IS NOT NULL)",
    )
    .bind(actor.organization_id)
    .bind(source_run_id)
    .fetch_one(pool)
    .await
}

pub async fn find_by_idempotency(
    pool: &PgPool,
    actor: &ActorContext,
    source_run_id: i64,
    idempotency_key: &str,
) -> Result<Option<DevRailContinuationRequestRow>, sqlx::Error> {
    let sql = format!(
        "{} SELECT {REQUEST_COLUMNS} FROM devrail_continuation_requests r WHERE r.source_run_id=$5 AND r.idempotency_key=$6 AND {}",
        visible_departments_cte(),
        scoped_request("r")
    );
    sqlx::query_as::<_, DevRailContinuationRequestRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(source_run_id)
        .bind(idempotency_key)
        .fetch_optional(pool)
        .await
}

pub async fn cancel(
    connection: &mut PgConnection,
    actor: &ActorContext,
    id: i64,
) -> Result<Option<DevRailContinuationRequestRow>, sqlx::Error> {
    let sql = format!(
        "{} UPDATE devrail_continuation_requests r SET status='cancelled', status_version=status_version+1, claim_owner=NULL, claim_token=NULL, claim_expires_at=NULL, cancelled_at=COALESCE(cancelled_at,now()), updated_at=now() WHERE r.id=$5 AND {} AND r.status IN ('pending','claimed') RETURNING {REQUEST_COLUMNS}",
        visible_departments_cte(),
        scoped_request("r")
    );
    let request = sqlx::query_as::<_, DevRailContinuationRequestRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(id)
        .fetch_optional(&mut *connection)
        .await?;
    let Some(request) = request else {
        return Ok(None);
    };
    let trace = Uuid::new_v4().to_string();
    sqlx::query(
        "SELECT set_config('devrail.actor_type',$1,true),
                set_config('devrail.actor_user_id',$2,true),
                set_config('devrail.transition_reason','continuation_cancelled',true),
                set_config('devrail.trace_id',$3,true),
                set_config('devrail.continuation_request_id',$4,true),
                set_config('devrail.source_run_id',$5,true),
                set_config('devrail.child_run_id','',true),
                set_config('devrail.continuation_trigger_type',$6,true),
                set_config('devrail.continuation_policy_version',$7,true)",
    )
    .bind(actor.actor_type.as_str())
    .bind(actor.user_id.to_string())
    .bind(&trace)
    .bind(request.id.to_string())
    .bind(request.source_run_id.to_string())
    .bind(&request.trigger_type)
    .bind(&request.policy_version)
    .execute(&mut *connection)
    .await?;
    let restored = sqlx::query(
        "UPDATE devrail_tasks SET status=prior_status.prior_task_status, revision=revision+1, updated_at=now() FROM devrail_continuation_requests prior_status WHERE devrail_tasks.id=prior_status.task_id AND devrail_tasks.organization_id=prior_status.organization_id AND prior_status.id=$1 AND devrail_tasks.status='continuation_pending'",
    )
    .bind(id)
    .execute(&mut *connection)
    .await?;
    if restored.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "continuation 取消期间任务状态发生变化".to_string(),
        ));
    }
    let task = sqlx::query_as::<_, crate::models::DevRailTaskRow>(AssertSqlSafe(format!(
        "SELECT {TASK_COLUMNS} FROM devrail_tasks WHERE id=$1 AND organization_id=$2"
    )))
    .bind(request.task_id)
    .bind(request.organization_id)
    .fetch_one(&mut *connection)
    .await?;
    devrail::append_task_event(
        &mut *connection,
        &task,
        "continuation.cancelled",
        &format!("continuation:{}:cancelled", request.id),
        &serde_json::json!({
            "requestId": request.id,
            "sourceRunId": request.source_run_id,
            "triggerType": request.trigger_type,
            "sequence": request.continuation_sequence,
        }),
        "continuation 请求已取消",
    )
    .await?;
    audit_logs::record_actor(
        &mut *connection,
        actor,
        "devrail.continuation.cancel",
        "devrail_continuation_request",
        Some(id),
        serde_json::json!({"taskId": request.task_id, "sourceRunId": request.source_run_id}),
    )
    .await?;
    devrail_notifications::outbox(
        &mut *connection,
        actor.organization_id,
        "devrail.continuation.cancelled",
        "devrail_continuation_request",
        Some(id),
        &serde_json::json!({
            "notificationId": id,
            "eventType": "devrail.continuation.cancelled",
            "summary": "继续执行请求已取消",
            "deepLink": format!("/devrail/tasks/{}", request.task_id),
        }),
    )
    .await?;
    Ok(Some(request))
}

pub async fn mark_dispatched(
    connection: &mut PgConnection,
    actor: &ActorContext,
    request_id: i64,
    worker_id: &str,
    claim_token: Uuid,
    child_run_id: i64,
) -> Result<Option<DevRailContinuationRequestRow>, sqlx::Error> {
    let request = sqlx::query_as::<_, DevRailContinuationRequestRow>(AssertSqlSafe(format!(
        "UPDATE devrail_continuation_requests r
         SET status='dispatched', status_version=status_version+1,
             child_run_id=$4, claim_owner=NULL, claim_token=NULL,
             claim_expires_at=NULL,
             dispatched_at=COALESCE(dispatched_at,now()), updated_at=now()
         WHERE r.id=$1 AND r.status='claimed' AND r.claim_owner=$2
           AND r.claim_token=$3 AND r.claim_expires_at>now()
         RETURNING {REQUEST_COLUMNS}"
    )))
    .bind(request_id)
    .bind(worker_id)
    .bind(claim_token)
    .bind(child_run_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(request) = request else {
        crate::app_metrics::record_continuation_claim_conflict();
        return Ok(None);
    };
    let trace = Uuid::new_v4().to_string();
    sqlx::query(
        "SELECT set_config('devrail.actor_type','system',true),
                set_config('devrail.actor_user_id','',true),
                set_config('devrail.transition_reason','continuation_dispatched',true),
                set_config('devrail.trace_id',$1,true),
                set_config('devrail.continuation_request_id',$2,true),
                set_config('devrail.source_run_id',$3,true),
                set_config('devrail.child_run_id',$4,true),
                set_config('devrail.continuation_trigger_type',$5,true),
                set_config('devrail.continuation_policy_version',$6,true)",
    )
    .bind(&trace)
    .bind(request.id.to_string())
    .bind(request.source_run_id.to_string())
    .bind(child_run_id.to_string())
    .bind(&request.trigger_type)
    .bind(&request.policy_version)
    .execute(&mut *connection)
    .await?;
    let updated = sqlx::query(
        "UPDATE devrail_tasks SET status='running', revision=revision+1, updated_at=now()
         WHERE id=$1 AND organization_id=$2 AND status='continuation_pending'
           AND EXISTS (
               SELECT 1 FROM devrail_runs child
               WHERE child.id=$3 AND child.organization_id=$2
                 AND child.task_id=devrail_tasks.id
                 AND child.continuation_request_id=$4
                 AND child.status='starting'
           )",
    )
    .bind(request.task_id)
    .bind(request.organization_id)
    .bind(child_run_id)
    .bind(request.id)
    .execute(&mut *connection)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "continuation 派发期间任务状态发生变化".to_string(),
        ));
    }
    let task = sqlx::query_as::<_, crate::models::DevRailTaskRow>(AssertSqlSafe(format!(
        "SELECT {TASK_COLUMNS} FROM devrail_tasks WHERE id=$1 AND organization_id=$2"
    )))
    .bind(request.task_id)
    .bind(request.organization_id)
    .fetch_one(&mut *connection)
    .await?;
    devrail::append_task_event(
        &mut *connection,
        &task,
        "continuation.dispatched",
        &format!("continuation:{}:dispatched", request.id),
        &serde_json::json!({
            "requestId": request.id,
            "sourceRunId": request.source_run_id,
            "childRunId": child_run_id,
            "triggerType": request.trigger_type,
            "sequence": request.continuation_sequence,
        }),
        "continuation 请求已派发",
    )
    .await?;
    audit_logs::record_actor(
        &mut *connection,
        actor,
        "devrail.continuation.dispatch",
        "devrail_continuation_request",
        Some(request.id),
        serde_json::json!({"taskId": request.task_id, "childRunId": child_run_id}),
    )
    .await?;
    devrail_notifications::outbox(
        &mut *connection,
        request.organization_id,
        "devrail.continuation.dispatched",
        "devrail_continuation_request",
        Some(request.id),
        &serde_json::json!({
            "notificationId": request.id,
            "eventType": "devrail.continuation.dispatched",
            "summary": "继续执行请求已派发",
            "deepLink": format!("/devrail/runs/{child_run_id}"),
        }),
    )
    .await?;
    Ok(Some(request))
}

pub async fn complete_for_child_run(
    connection: &mut PgConnection,
    actor: &ActorContext,
    child_run_id: i64,
    result_code: &str,
    task_status: &str,
) -> Result<Option<DevRailContinuationRequestRow>, sqlx::Error> {
    if !matches!(task_status, "succeeded" | "failed" | "cancelled") {
        return Err(sqlx::Error::Protocol(
            "continuation child 任务终态无效".to_string(),
        ));
    }
    let request = sqlx::query_as::<_, DevRailContinuationRequestRow>(AssertSqlSafe(format!(
        "WITH RECURSIVE run_lineage AS (
             SELECT child.id, child.parent_run_id, child.continuation_request_id,
                    child.status
             FROM devrail_runs child
             WHERE child.id=$1
             UNION ALL
             SELECT parent.id, parent.parent_run_id, parent.continuation_request_id,
                    parent.status
             FROM devrail_runs parent
             JOIN run_lineage child ON child.parent_run_id=parent.id
         ), matched AS (
             SELECT r.id
             FROM devrail_continuation_requests r
             JOIN run_lineage lineage ON lineage.continuation_request_id=r.id
             WHERE r.status='dispatched'
               AND lineage.status IN ('completed','failed','cancelled')
             ORDER BY CASE WHEN lineage.id=$1 THEN 0 ELSE 1 END, r.id
             LIMIT 1
         )
         UPDATE devrail_continuation_requests r
         SET status='completed', status_version=status_version+1,
             result_code=$2, completed_at=COALESCE(completed_at,now()), updated_at=now()
         FROM matched
         WHERE r.id=matched.id
         RETURNING {REQUEST_COLUMNS}"
    )))
    .bind(child_run_id)
    .bind(result_code)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(request) = request else {
        return Ok(None);
    };
    let trace = Uuid::new_v4().to_string();
    sqlx::query(
        "SELECT set_config('devrail.actor_type','system',true),
                set_config('devrail.actor_user_id','',true),
                set_config('devrail.transition_reason','continuation_completed',true),
                set_config('devrail.trace_id',$1,true),
                set_config('devrail.continuation_request_id',$2,true),
                set_config('devrail.source_run_id',$3,true),
                set_config('devrail.child_run_id',$4,true),
                set_config('devrail.continuation_trigger_type',$5,true),
                set_config('devrail.continuation_policy_version',$6,true)",
    )
    .bind(&trace)
    .bind(request.id.to_string())
    .bind(request.source_run_id.to_string())
    .bind(child_run_id.to_string())
    .bind(&request.trigger_type)
    .bind(&request.policy_version)
    .execute(&mut *connection)
    .await?;
    let projected = sqlx::query(
        "UPDATE devrail_tasks SET status=$3, revision=revision+1,
             scheduler_claim_token=NULL, scheduler_claimed_at=NULL,
             scheduler_retry_at=NULL, scheduler_last_error=NULL, updated_at=now()
         WHERE id=$1 AND organization_id=$2 AND status IN ('running','queued')",
    )
    .bind(request.task_id)
    .bind(request.organization_id)
    .bind(task_status)
    .execute(&mut *connection)
    .await?;
    if projected.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "continuation child 终态投影期间任务状态发生变化".to_string(),
        ));
    }
    let task = sqlx::query_as::<_, crate::models::DevRailTaskRow>(AssertSqlSafe(format!(
        "SELECT {TASK_COLUMNS} FROM devrail_tasks WHERE id=$1 AND organization_id=$2"
    )))
    .bind(request.task_id)
    .bind(request.organization_id)
    .fetch_one(&mut *connection)
    .await?;
    devrail::append_task_event(
        &mut *connection,
        &task,
        "continuation.completed",
        &format!("continuation:{}:completed", request.id),
        &serde_json::json!({
            "requestId": request.id,
            "sourceRunId": request.source_run_id,
            "childRunId": child_run_id,
            "resultCode": result_code,
            "sequence": request.continuation_sequence,
        }),
        "continuation child run 已结束",
    )
    .await?;
    audit_logs::record_actor(
        &mut *connection,
        actor,
        "devrail.continuation.complete",
        "devrail_continuation_request",
        Some(request.id),
        serde_json::json!({"taskId": request.task_id, "childRunId": child_run_id, "resultCode": result_code}),
    )
    .await?;
    devrail_notifications::outbox(
        &mut *connection,
        request.organization_id,
        "devrail.continuation.completed",
        "devrail_continuation_request",
        Some(request.id),
        &serde_json::json!({
            "notificationId": request.id,
            "eventType": "devrail.continuation.completed",
            "summary": "继续执行结果已更新",
            "deepLink": format!("/devrail/runs/{child_run_id}"),
        }),
    )
    .await?;
    Ok(Some(request))
}

pub async fn claim_pending(
    pool: &PgPool,
    worker_id: &str,
    claim_token: Uuid,
    limit: i64,
    lease_seconds: i64,
) -> Result<Vec<DevRailContinuationRequestRow>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let rows = sqlx::query_as::<_, DevRailContinuationRequestRow>(
        "WITH candidates AS (
             SELECT id FROM devrail_continuation_requests
             WHERE status='pending'
               AND (next_attempt_at IS NULL OR next_attempt_at <= now())
             ORDER BY created_at,id
             FOR UPDATE SKIP LOCKED
             LIMIT $1
         )
         UPDATE devrail_continuation_requests r
         SET status='claimed', status_version=status_version+1,
             claim_owner=$2, claim_token=$3,
             claim_expires_at=now() + make_interval(secs => $4),
             claimed_at=COALESCE(claimed_at,now()),
             dispatch_attempts=dispatch_attempts+1, updated_at=now()
         FROM candidates
         WHERE r.id=candidates.id
         RETURNING r.id, r.organization_id, r.department_id, r.owner_user_id, r.project_id, r.task_id, r.source_run_id, r.root_run_id, r.source_turn_id, r.requested_by_user_id, r.trigger_type, r.evidence_ref, r.evidence_digest, r.evidence_observed_at, r.evidence_expires_at, r.changeset_digest, r.redacted_context, r.context_summary, r.input_digest, r.idempotency_key, r.continuation_sequence, r.chain_depth, r.prior_task_status, r.policy_version, r.policy_snapshot, r.status, r.status_version, r.claim_owner, r.claim_token, r.claim_expires_at, r.dispatch_attempts, r.next_attempt_at, r.child_run_id, r.result_code, r.created_at, r.updated_at, r.claimed_at, r.dispatched_at, r.completed_at, r.cancelled_at, r.rejected_at",
    )
    .bind(limit.clamp(1, 100))
    .bind(worker_id)
    .bind(claim_token)
    .bind(lease_seconds.clamp(10, 3_600))
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(rows)
}

pub async fn pending_depth(pool: &PgPool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM devrail_continuation_requests WHERE status IN ('pending','claimed')",
    )
    .fetch_one(pool)
    .await
}

pub async fn list_dispatched_unstarted(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<DevRailContinuationRequestRow>, sqlx::Error> {
    sqlx::query_as::<_, DevRailContinuationRequestRow>(AssertSqlSafe(format!(
        "SELECT {REQUEST_COLUMNS}
         FROM devrail_continuation_requests r
         JOIN devrail_runs child
           ON child.id=r.child_run_id
          AND child.organization_id=r.organization_id
         WHERE r.status='dispatched'
           AND child.status='starting'
           AND child.started_at IS NULL
           AND (child.harness_start_claim_token IS NULL
                OR child.harness_start_claim_expires_at<=now())
         ORDER BY r.dispatched_at NULLS FIRST,r.id
         LIMIT $1"
    )))
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await
}

pub async fn renew_claim(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
    claim_token: Uuid,
    lease_seconds: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_continuation_requests SET claim_expires_at=now()+make_interval(secs => $4), updated_at=now() WHERE id=$1 AND status='claimed' AND claim_owner=$2 AND claim_token=$3 AND claim_expires_at>now()",
    )
    .bind(id)
    .bind(worker_id)
    .bind(claim_token)
    .bind(lease_seconds.clamp(10, 3_600))
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn release_expired_claims(pool: &PgPool, limit: i64) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_continuation_requests SET status='pending', status_version=status_version+1, claim_owner=NULL, claim_token=NULL, claim_expires_at=NULL, next_attempt_at=now(), updated_at=now() WHERE id IN (SELECT id FROM devrail_continuation_requests WHERE status='claimed' AND claim_expires_at<=now() ORDER BY claim_expires_at,id FOR UPDATE SKIP LOCKED LIMIT $1)",
    )
    .bind(limit.clamp(1, 500))
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn release_claim(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
    claim_token: Uuid,
    backoff_seconds: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_continuation_requests SET status='pending', status_version=status_version+1, claim_owner=NULL, claim_token=NULL, claim_expires_at=NULL, next_attempt_at=now()+make_interval(secs => $4), updated_at=now() WHERE id=$1 AND status='claimed' AND claim_owner=$2 AND claim_token=$3",
    )
    .bind(id)
    .bind(worker_id)
    .bind(claim_token)
    .bind(backoff_seconds.clamp(0, 3_600))
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn reject_claim(
    connection: &mut PgConnection,
    actor: &ActorContext,
    id: i64,
    worker_id: &str,
    claim_token: Uuid,
    result_code: &str,
) -> Result<bool, sqlx::Error> {
    let request = sqlx::query_as::<_, DevRailContinuationRequestRow>(AssertSqlSafe(format!(
        "UPDATE devrail_continuation_requests r
         SET status='rejected', status_version=status_version+1,
             result_code=$4, claim_owner=NULL, claim_token=NULL,
             claim_expires_at=NULL, rejected_at=COALESCE(rejected_at,now()),
             updated_at=now()
         WHERE r.id=$1 AND r.status='claimed' AND r.claim_owner=$2
           AND r.claim_token=$3
         RETURNING {REQUEST_COLUMNS}"
    )))
    .bind(id)
    .bind(worker_id)
    .bind(claim_token)
    .bind(result_code)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(request) = request else {
        return Ok(false);
    };
    let trace = Uuid::new_v4().to_string();
    sqlx::query(
        "SELECT set_config('devrail.actor_type','system',true),
                set_config('devrail.actor_user_id','',true),
                set_config('devrail.transition_reason','continuation_rejected',true),
                set_config('devrail.trace_id',$1,true),
                set_config('devrail.continuation_request_id',$2,true),
                set_config('devrail.source_run_id',$3,true),
                set_config('devrail.child_run_id','',true),
                set_config('devrail.continuation_trigger_type',$4,true),
                set_config('devrail.continuation_policy_version',$5,true)",
    )
    .bind(&trace)
    .bind(request.id.to_string())
    .bind(request.source_run_id.to_string())
    .bind(&request.trigger_type)
    .bind(&request.policy_version)
    .execute(&mut *connection)
    .await?;
    let restored = sqlx::query(
        "UPDATE devrail_tasks
         SET status=prior.prior_task_status, revision=revision+1, updated_at=now()
         FROM devrail_continuation_requests prior
         WHERE devrail_tasks.id=prior.task_id
           AND devrail_tasks.organization_id=prior.organization_id
           AND prior.id=$1 AND devrail_tasks.status='continuation_pending'",
    )
    .bind(id)
    .execute(&mut *connection)
    .await?;
    if restored.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "continuation 拒绝期间任务状态发生变化".to_string(),
        ));
    }
    let task = sqlx::query_as::<_, crate::models::DevRailTaskRow>(AssertSqlSafe(format!(
        "SELECT {TASK_COLUMNS} FROM devrail_tasks WHERE id=$1 AND organization_id=$2"
    )))
    .bind(request.task_id)
    .bind(request.organization_id)
    .fetch_one(&mut *connection)
    .await?;
    devrail::append_task_event(
        &mut *connection,
        &task,
        "continuation.rejected",
        &format!("continuation:{}:rejected", request.id),
        &serde_json::json!({
            "requestId": request.id,
            "sourceRunId": request.source_run_id,
            "resultCode": result_code,
            "sequence": request.continuation_sequence,
        }),
        "continuation 请求已拒绝",
    )
    .await?;
    audit_logs::record_actor(
        &mut *connection,
        actor,
        "devrail.continuation.reject",
        "devrail_continuation_request",
        Some(request.id),
        serde_json::json!({"taskId": request.task_id, "resultCode": result_code}),
    )
    .await?;
    devrail_notifications::outbox(
        &mut *connection,
        request.organization_id,
        "devrail.continuation.rejected",
        "devrail_continuation_request",
        Some(request.id),
        &serde_json::json!({
            "notificationId": request.id,
            "eventType": "devrail.continuation.rejected",
            "summary": "继续执行请求未通过派发校验",
            "deepLink": format!("/devrail/tasks/{}", request.task_id),
        }),
    )
    .await?;
    Ok(true)
}

#[cfg(test)]
pub(crate) mod integration_tests {
    use super::*;
    use crate::access::{ActorType, DataScope};
    use crate::db::DATABASE_TEST_LOCK;
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};

    pub(crate) struct Fixture {
        pub(crate) actor: ActorContext,
        pub(crate) project_id: i64,
        pub(crate) repository_id: i64,
        pub(crate) environment_id: i64,
        pub(crate) task_id: i64,
        pub(crate) snapshot_id: i64,
        pub(crate) source_run_id: i64,
        pub(crate) source_turn_id: String,
    }

    pub(crate) async fn test_pool() -> Option<PgPool> {
        crate::db::test_pool().await
    }

    pub(crate) async fn fixture(pool: &PgPool) -> Fixture {
        let suffix = Uuid::new_v4().simple().to_string();
        let (owner_user_id, organization_id, department_id) =
            sqlx::query_as::<_, (i64, i64, Option<i64>)>(
                "SELECT id,organization_id,department_id FROM users ORDER BY id LIMIT 1",
            )
            .fetch_one(pool)
            .await
            .expect("continuation test user");
        let project_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO devrail_projects
                 (organization_id,department_id,owner_user_id,slug,name)
             VALUES ($1,$2,$3,$4,'Continuation 测试') RETURNING id",
        )
        .bind(organization_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(format!("continuation-{suffix}"))
        .fetch_one(pool)
        .await
        .expect("continuation project");
        let repository_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO devrail_repositories
                 (organization_id,department_id,owner_user_id,project_id,name,remote_url,protocol)
             VALUES ($1,$2,$3,$4,$5,'https://example.invalid/devrail.git','https')
             RETURNING id",
        )
        .bind(organization_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(project_id)
        .bind(format!("continuation-repo-{suffix}"))
        .fetch_one(pool)
        .await
        .expect("continuation repository");
        let environment_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO devrail_environments
                 (organization_id,department_id,owner_user_id,project_id,name,workspace_root)
             VALUES ($1,$2,$3,$4,$5,'/tmp/devrail-continuation-tests')
             RETURNING id",
        )
        .bind(organization_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(project_id)
        .bind(format!("continuation-env-{suffix}"))
        .fetch_one(pool)
        .await
        .expect("continuation environment");
        let task_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO devrail_tasks
                 (organization_id,department_id,owner_user_id,project_id,repository_id,
                  environment_id,title,goal,status)
             VALUES ($1,$2,$3,$4,$5,$6,'Continuation 测试任务','验证幂等和领取','succeeded')
             RETURNING id",
        )
        .bind(organization_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(project_id)
        .bind(repository_id)
        .bind(environment_id)
        .fetch_one(pool)
        .await
        .expect("continuation task");
        let snapshot_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO devrail_task_snapshots
                 (organization_id,department_id,owner_user_id,task_id,snapshot)
             VALUES ($1,$2,$3,$4,$5) RETURNING id",
        )
        .bind(organization_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(task_id)
        .bind(json!({"goal":"验证 continuation"}))
        .fetch_one(pool)
        .await
        .expect("continuation snapshot");
        let source_turn_id = format!("turn-{suffix}");
        let source_run_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO devrail_runs
                 (organization_id,department_id,owner_user_id,task_id,snapshot_id,
                  idempotency_key,attempt,status,thread_id,turn_id,cwd,completed_at)
             VALUES ($1,$2,$3,$4,$5,$6,1,'completed',$7,$8,'/tmp/continuation-source',now())
             RETURNING id",
        )
        .bind(organization_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(task_id)
        .bind(snapshot_id)
        .bind(format!("source-{suffix}"))
        .bind(format!("thread-{suffix}"))
        .bind(&source_turn_id)
        .fetch_one(pool)
        .await
        .expect("continuation source run");
        sqlx::query("UPDATE devrail_runs SET root_run_id=id WHERE id=$1")
            .bind(source_run_id)
            .execute(pool)
            .await
            .expect("continuation root lineage");
        Fixture {
            actor: ActorContext {
                actor_type: ActorType::User,
                user_id: owner_user_id,
                session_id: 1,
                organization_id,
                department_id,
                data_scope: DataScope::Organization,
                permission_codes: BTreeSet::new(),
            },
            project_id,
            repository_id,
            environment_id,
            task_id,
            snapshot_id,
            source_run_id,
            source_turn_id,
        }
    }

    async fn create_request(
        pool: &PgPool,
        fixture: &Fixture,
        sequence: i16,
    ) -> DevRailContinuationRequestRow {
        let expected_task_revision =
            sqlx::query_scalar::<_, i64>("SELECT revision FROM devrail_tasks WHERE id=$1")
                .bind(fixture.task_id)
                .fetch_one(pool)
                .await
                .expect("current task revision");
        create_request_result(pool, fixture, sequence, expected_task_revision)
            .await
            .expect("create continuation request")
    }

    async fn create_request_result(
        pool: &PgPool,
        fixture: &Fixture,
        sequence: i16,
        expected_task_revision: i64,
    ) -> Result<DevRailContinuationRequestRow, sqlx::Error> {
        let key = format!("request-{sequence}");
        let evidence_ref = format!("user:{}:{key}", fixture.actor.user_id);
        let digest = format!("{sequence:064x}");
        let mut tx = pool.begin().await.expect("begin continuation create");
        let result = create(
            &mut tx,
            &NewContinuation {
                actor: &fixture.actor,
                project_id: fixture.project_id,
                task_id: fixture.task_id,
                source_run_id: fixture.source_run_id,
                root_run_id: fixture.source_run_id,
                source_turn_id: &fixture.source_turn_id,
                requested_by_user_id: fixture.actor.user_id,
                trigger_type: "user_context",
                evidence_ref: &evidence_ref,
                evidence_digest: &digest,
                evidence_observed_at: Utc::now(),
                evidence_expires_at: None,
                changeset_digest: None,
                redacted_context: "请继续验证",
                context_summary: "用户追加上下文",
                input_digest: &digest,
                idempotency_key: &key,
                continuation_sequence: sequence,
                chain_depth: sequence,
                prior_task_status: "succeeded",
                expected_task_revision,
                policy_version: "test-v1",
                policy_snapshot: &json!({"enabled":true}),
            },
        )
        .await;
        match result {
            Ok((request, _)) => {
                tx.commit().await.expect("commit continuation create");
                Ok(request)
            }
            Err(error) => Err(error),
        }
    }

    async fn create_trigger_request(
        pool: &PgPool,
        fixture: &Fixture,
        sequence: i16,
        trigger_type: &str,
        evidence_ref: &str,
        idempotency_key: &str,
    ) -> (DevRailContinuationRequestRow, bool) {
        let digest = format!("{sequence:064x}");
        let policy_snapshot = json!({"enabled":true});
        let expected_task_revision =
            sqlx::query_scalar::<_, i64>("SELECT revision FROM devrail_tasks WHERE id=$1")
                .bind(fixture.task_id)
                .fetch_one(pool)
                .await
                .expect("trusted trigger task revision");
        let mut tx = pool.begin().await.expect("begin trigger request");
        let result = create(
            &mut tx,
            &NewContinuation {
                actor: &fixture.actor,
                project_id: fixture.project_id,
                task_id: fixture.task_id,
                source_run_id: fixture.source_run_id,
                root_run_id: fixture.source_run_id,
                source_turn_id: &fixture.source_turn_id,
                requested_by_user_id: fixture.actor.user_id,
                trigger_type,
                evidence_ref,
                evidence_digest: &digest,
                evidence_observed_at: Utc::now(),
                evidence_expires_at: None,
                changeset_digest: Some(&digest),
                redacted_context: "请根据受信任证据继续处理",
                context_summary: "受信任触发要求继续执行",
                input_digest: &digest,
                idempotency_key,
                continuation_sequence: sequence,
                chain_depth: sequence,
                prior_task_status: "succeeded",
                expected_task_revision,
                policy_version: "test-v1",
                policy_snapshot: &policy_snapshot,
            },
        )
        .await
        .expect("create trusted trigger request");
        tx.commit().await.expect("commit trusted trigger request");
        result
    }

    async fn create_child_for_request(
        pool: &PgPool,
        fixture: &Fixture,
        request: &DevRailContinuationRequestRow,
        idempotency_key: &str,
        harness_start_key: &str,
    ) -> crate::models::DevRailRunRow {
        let task = sqlx::query_as::<_, crate::models::DevRailTaskRow>(AssertSqlSafe(format!(
            "SELECT {TASK_COLUMNS} FROM devrail_tasks WHERE id=$1"
        )))
        .bind(fixture.task_id)
        .fetch_one(pool)
        .await
        .expect("child task");
        let source =
            crate::repositories::devrail_runs::find_for_recovery(pool, fixture.source_run_id)
                .await
                .expect("child source lookup")
                .expect("child source run");
        let workflow_snapshot = task
            .dispatch_snapshot
            .get("workflow")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let policy = json!({"version":"test-v1"});
        let startup_args = json!(["app-server"]);
        let mut tx = pool.begin().await.expect("begin child create");
        let child = crate::repositories::devrail_runs::create_continuation_run(
            &mut tx,
            &crate::repositories::devrail_runs::NewContinuationRun {
                actor: &fixture.actor,
                task_id: fixture.task_id,
                snapshot_id: source.snapshot_id,
                idempotency_key,
                task_revision: task.revision,
                workflow_source: &task.workflow_source,
                workflow_version: &task.workflow_version,
                workflow_digest: &task.workflow_digest,
                workflow_snapshot: &workflow_snapshot,
                parent_run_id: fixture.source_run_id,
                parent_turn_id: &fixture.source_turn_id,
                thread_id: source.thread_id.as_deref().expect("source thread"),
                continuation_request_id: request.id,
                continuation_sequence: request.continuation_sequence,
                harness_start_key,
                cwd: "/tmp/continuation-child",
                policy: &policy,
                startup_args: &startup_args,
                model_id: None,
                department_id: fixture.actor.department_id,
            },
        )
        .await
        .expect("create or reuse child")
        .expect("continuation child");
        tx.commit().await.expect("commit child create");
        child
    }

    pub(crate) async fn set_continuation_policy(
        pool: &PgPool,
        fixture: &Fixture,
        enabled: bool,
        max_context_bytes: usize,
    ) {
        sqlx::query(
            "UPDATE devrail_tasks
             SET dispatch_snapshot=jsonb_set(
                 dispatch_snapshot,'{workflow}',
                 COALESCE(dispatch_snapshot->'workflow','{}'::jsonb)
                   || jsonb_build_object(
                       'config',jsonb_build_object('continuation',$2::jsonb)
                   ),true)
             WHERE id=$1",
        )
        .bind(fixture.task_id)
        .bind(json!({
            "enabled": enabled,
            "allowed_triggers": ["user_context", "quality_gate", "review_changes"],
            "max_continuations": 3,
            "max_chain_depth": 3,
            "max_context_bytes": max_context_bytes,
            "claim_lease_seconds": 60,
            "max_dispatch_attempts": 3,
            "retry_base_delay_seconds": 5,
            "retry_max_delay_seconds": 300
        }))
        .execute(pool)
        .await
        .expect("set continuation policy");
    }

    pub(crate) async fn persist_test_handoff(
        pool: &PgPool,
        fixture: &Fixture,
        changeset_digest: &str,
    ) {
        let task_digest = "1".repeat(64);
        let workflow_digest = "2".repeat(64);
        let environment_digest = "3".repeat(64);
        let repository_digest = "4".repeat(64);
        let base_commit = "a".repeat(40);
        let head_commit = "b".repeat(40);
        let tool_versions = json!({"git":"2.51.0","codex":"0.1.0"});
        let mut tx = pool.begin().await.expect("begin test handoff");
        create_handoff(
            &mut tx,
            &NewRunHandoff {
                actor: &fixture.actor,
                project_id: fixture.project_id,
                task_id: fixture.task_id,
                source_run_id: fixture.source_run_id,
                task_snapshot_id: fixture.snapshot_id,
                repository_id: fixture.repository_id,
                environment_id: Some(fixture.environment_id),
                task_snapshot_digest: &task_digest,
                workflow_snapshot_digest: &workflow_digest,
                environment_snapshot_digest: Some(&environment_digest),
                repository_identity: "repository:test:continuation",
                repository_identity_digest: &repository_digest,
                base_commit: &base_commit,
                head_commit: Some(&head_commit),
                branch_ref: Some("refs/heads/devrail-continuation"),
                changeset_ref: Some("handoffs/run.patch"),
                changeset_digest,
                tool_versions: &tool_versions,
                evidence_status: "available",
                error_code: None,
                validated_at: Some(Utc::now()),
            },
        )
        .await
        .expect("persist test handoff");
        tx.commit().await.expect("commit test handoff");
    }

    pub(crate) async fn stored_context(pool: &PgPool, request_id: i64) -> String {
        sqlx::query_scalar("SELECT redacted_context FROM devrail_continuation_requests WHERE id=$1")
            .bind(request_id)
            .fetch_one(pool)
            .await
            .expect("stored continuation context")
    }

    #[tokio::test]
    async fn replay_scope_pagination_and_cancel_are_deterministic() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = fixture(&pool).await;
        let source_turn_id: String =
            sqlx::query_scalar("SELECT turn_id FROM devrail_runs WHERE id=$1")
                .bind(fixture.source_run_id)
                .fetch_one(&pool)
                .await
                .expect("source turn");
        let key = "stable-request";
        let digest = "1".repeat(64);
        let evidence_ref = format!("user:{}:{key}", fixture.actor.user_id);
        let input = NewContinuation {
            actor: &fixture.actor,
            project_id: fixture.project_id,
            task_id: fixture.task_id,
            source_run_id: fixture.source_run_id,
            root_run_id: fixture.source_run_id,
            source_turn_id: &source_turn_id,
            requested_by_user_id: fixture.actor.user_id,
            trigger_type: "user_context",
            evidence_ref: &evidence_ref,
            evidence_digest: &digest,
            evidence_observed_at: Utc::now(),
            evidence_expires_at: None,
            changeset_digest: None,
            redacted_context: "请继续验证",
            context_summary: "用户追加上下文",
            input_digest: &digest,
            idempotency_key: key,
            continuation_sequence: 1,
            chain_depth: 1,
            prior_task_status: "succeeded",
            expected_task_revision: 1,
            policy_version: "test-v1",
            policy_snapshot: &json!({"enabled":true}),
        };
        let mut tx = pool.begin().await.expect("begin initial request");
        let (first, created) = create(&mut tx, &input).await.expect("initial request");
        assert!(created);
        tx.commit().await.expect("commit initial request");
        let pending_history = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                Option<i64>,
                Option<i64>,
                Option<i64>,
                Option<String>,
                Option<String>,
            ),
        >(
            "SELECT from_status,to_status,reason,continuation_request_id,
                    source_run_id,child_run_id,continuation_trigger_type,
                    continuation_policy_version
             FROM devrail_task_status_history
             WHERE task_id=$1 AND continuation_request_id=$2",
        )
        .bind(fixture.task_id)
        .bind(first.id)
        .fetch_one(&pool)
        .await
        .expect("continuation pending history");
        assert_eq!(pending_history.0, "succeeded");
        assert_eq!(pending_history.1, "continuation_pending");
        assert_eq!(pending_history.2, "continuation_requested");
        assert_eq!(pending_history.3, Some(first.id));
        assert_eq!(pending_history.4, Some(fixture.source_run_id));
        assert_eq!(pending_history.5, None);
        assert_eq!(pending_history.6.as_deref(), Some("user_context"));
        assert_eq!(pending_history.7.as_deref(), Some("test-v1"));
        let mut replay_tx = pool.begin().await.expect("begin replay");
        let (replayed, created) = create(&mut replay_tx, &input)
            .await
            .expect("replay request");
        assert!(!created);
        assert_eq!(replayed.id, first.id);
        replay_tx.commit().await.expect("commit replay");
        let create_fact_counts = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            "SELECT
                 (SELECT count(*) FROM devrail_task_events
                  WHERE task_id=$1 AND event_type='continuation.created'),
                 (SELECT count(*) FROM audit_logs
                  WHERE target_type='devrail_continuation_request'
                    AND target_id=$2 AND action='devrail.continuation.create'),
                 (SELECT count(*) FROM devrail_outbox_events
                  WHERE aggregate_type='devrail_continuation_request'
                    AND aggregate_id=$2 AND event_type='devrail.continuation.created'),
                 (SELECT count(*) FROM devrail_task_status_history
                  WHERE task_id=$1 AND continuation_request_id=$2
                    AND reason='continuation_requested')",
        )
        .bind(fixture.task_id)
        .bind(first.id)
        .fetch_one(&pool)
        .await
        .expect("idempotent creation facts");
        assert_eq!(create_fact_counts, (1, 1, 1, 1));
        let mut cancel_tx = pool.begin().await.expect("begin cancel");
        assert!(cancel(&mut cancel_tx, &fixture.actor, first.id)
            .await
            .expect("cancel request")
            .is_some());
        cancel_tx.commit().await.expect("commit cancel");
        let mut repeated_cancel = pool.begin().await.expect("begin repeated cancel");
        assert!(cancel(&mut repeated_cancel, &fixture.actor, first.id)
            .await
            .expect("repeat cancel request")
            .is_none());
        repeated_cancel
            .commit()
            .await
            .expect("commit repeated cancel");
        let restored_status: String =
            sqlx::query_scalar("SELECT status FROM devrail_tasks WHERE id=$1")
                .bind(fixture.task_id)
                .fetch_one(&pool)
                .await
                .expect("restored task status");
        assert_eq!(restored_status, "succeeded");
        let cancel_history_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM devrail_task_status_history
             WHERE task_id=$1 AND continuation_request_id=$2
               AND from_status='continuation_pending' AND to_status='succeeded'
               AND reason='continuation_cancelled'",
        )
        .bind(fixture.task_id)
        .bind(first.id)
        .fetch_one(&pool)
        .await
        .expect("continuation cancel history");
        assert_eq!(cancel_history_count, 1);
        let cancel_fact_counts = sqlx::query_as::<_, (i64, i64, i64)>(
            "SELECT
                 (SELECT count(*) FROM devrail_task_events
                  WHERE task_id=$1 AND event_type='continuation.cancelled'),
                 (SELECT count(*) FROM audit_logs
                  WHERE target_type='devrail_continuation_request'
                    AND target_id=$2 AND action='devrail.continuation.cancel'),
                 (SELECT count(*) FROM devrail_outbox_events
                  WHERE aggregate_type='devrail_continuation_request'
                    AND aggregate_id=$2 AND event_type='devrail.continuation.cancelled')",
        )
        .bind(fixture.task_id)
        .bind(first.id)
        .fetch_one(&pool)
        .await
        .expect("idempotent cancellation facts");
        assert_eq!(cancel_fact_counts, (1, 1, 1));
        let second = create_request(&pool, &fixture, 2).await;
        let mut second_cancel = pool.begin().await.expect("begin second cancel");
        cancel(&mut second_cancel, &fixture.actor, second.id)
            .await
            .expect("cancel second request");
        second_cancel.commit().await.expect("commit second cancel");
        let third = create_request(&pool, &fixture, 3).await;
        let (page_one, total) = list(&pool, &fixture.actor, Some(fixture.task_id), None, 1, 2)
            .await
            .expect("first continuation page");
        let (page_two, _) = list(&pool, &fixture.actor, Some(fixture.task_id), None, 2, 2)
            .await
            .expect("second continuation page");
        assert_eq!(total, 3);
        assert_eq!(page_one.len(), 2);
        assert_eq!(page_two.len(), 1);
        let other_actor = other_organization_actor(&pool).await;
        assert!(find_by_id(&pool, &other_actor, first.id)
            .await
            .expect("cross organization lookup")
            .is_none());
        let mut cross_scope_cancel = pool.begin().await.expect("begin cross scope cancel");
        assert!(cancel(&mut cross_scope_cancel, &other_actor, third.id)
            .await
            .expect("cross organization cancel")
            .is_none());
        cross_scope_cancel
            .commit()
            .await
            .expect("commit cross scope cancel");
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup continuation pagination schema");
    }

    #[tokio::test]
    async fn trusted_evidence_replays_return_the_original_request() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = fixture(&pool).await;
        for (sequence, trigger_type, evidence_ref) in [
            (1, "quality_gate", "gate-result:stable-1"),
            (2, "review_changes", "review-event:stable-1"),
        ] {
            let first_key = format!("{trigger_type}:delivery-a");
            let replay_key = format!("{trigger_type}:delivery-b");
            let (first, created) = create_trigger_request(
                &pool,
                &fixture,
                sequence,
                trigger_type,
                evidence_ref,
                &first_key,
            )
            .await;
            assert!(created);
            let (replayed, created) = create_trigger_request(
                &pool,
                &fixture,
                sequence,
                trigger_type,
                evidence_ref,
                &replay_key,
            )
            .await;
            assert!(!created);
            assert_eq!(replayed.id, first.id);
            assert_eq!(replayed.idempotency_key, first_key);
            let mut cancel_tx = pool.begin().await.expect("begin trigger cancel");
            assert!(cancel(&mut cancel_tx, &fixture.actor, first.id)
                .await
                .expect("cancel trusted trigger")
                .is_some());
            cancel_tx.commit().await.expect("commit trigger cancel");
        }
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup continuation evidence schema");
    }

    #[tokio::test]
    async fn claims_reject_stale_tokens_and_recover_after_expiry() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = fixture(&pool).await;
        let request = create_request(&pool, &fixture, 1).await;
        let first_token = Uuid::new_v4();
        let second_token = Uuid::new_v4();
        let (first, second) = tokio::join!(
            claim_pending(&pool, "worker-a", first_token, 1, 60),
            claim_pending(&pool, "worker-b", second_token, 1, 60)
        );
        let first = first.expect("first claim");
        let second = second.expect("second claim");
        assert_eq!(first.len() + second.len(), 1);
        let (owner, current_token, stale_owner, stale_token) = if first.is_empty() {
            ("worker-b", second_token, "worker-a", first_token)
        } else {
            ("worker-a", first_token, "worker-b", second_token)
        };
        assert!(
            !renew_claim(&pool, request.id, stale_owner, stale_token, 60)
                .await
                .expect("reject stale renewal")
        );
        sqlx::query("UPDATE devrail_continuation_requests SET claim_expires_at=now()-interval '1 second' WHERE id=$1")
            .bind(request.id)
            .execute(&pool)
            .await
            .expect("expire claim");
        assert_eq!(
            release_expired_claims(&pool, 10)
                .await
                .expect("release expiry"),
            1
        );
        assert!(!release_claim(&pool, request.id, owner, current_token, 0)
            .await
            .expect("reject old token"));
        let third_token = Uuid::new_v4();
        let reclaimed = claim_pending(&pool, "worker-c", third_token, 1, 60)
            .await
            .expect("reclaim request");
        assert_eq!(reclaimed.first().map(|row| row.id), Some(request.id));
        assert_eq!(reclaimed.first().map(|row| row.dispatch_attempts), Some(2));
        assert!(
            release_claim(&pool, request.id, "worker-c", third_token, 60)
                .await
                .expect("release with backoff")
        );
        assert!(claim_pending(&pool, "worker-d", Uuid::new_v4(), 1, 60)
            .await
            .expect("respect backoff")
            .is_empty());
        sqlx::query(
            "UPDATE devrail_continuation_requests SET next_attempt_at=now()-interval '1 second' WHERE id=$1",
        )
        .bind(request.id)
        .execute(&pool)
        .await
        .expect("finish backoff");
        let fourth_token = Uuid::new_v4();
        let replayed = claim_pending(&pool, "worker-e", fourth_token, 1, 60)
            .await
            .expect("process replay claim");
        assert_eq!(replayed.first().map(|row| row.id), Some(request.id));
        assert_eq!(replayed.first().map(|row| row.dispatch_attempts), Some(3));
        let mut reject_tx = pool.begin().await.expect("begin reject claim");
        assert!(reject_claim(
            &mut reject_tx,
            &fixture.actor,
            request.id,
            "worker-e",
            fourth_token,
            "source_thread_missing",
        )
        .await
        .expect("reject claimed continuation"));
        reject_tx.commit().await.expect("commit reject claim");
        let restored = sqlx::query_as::<_, (String, i64)>(
            "SELECT status,
                    (SELECT count(*) FROM devrail_task_status_history h
                     WHERE h.task_id=t.id AND h.continuation_request_id=$2
                       AND h.reason='continuation_rejected'
                       AND h.to_status='succeeded')
             FROM devrail_tasks t WHERE t.id=$1",
        )
        .bind(fixture.task_id)
        .bind(request.id)
        .fetch_one(&pool)
        .await
        .expect("rejected continuation projection");
        assert_eq!(restored, ("succeeded".to_string(), 1));
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup continuation claim schema");
    }

    #[tokio::test]
    async fn dispatch_and_child_terminal_projection_are_atomic_and_idempotent() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = fixture(&pool).await;
        let source_terminal = sqlx::query_as::<_, (String, Option<DateTime<Utc>>)>(
            "SELECT status,completed_at FROM devrail_runs WHERE id=$1",
        )
        .bind(fixture.source_run_id)
        .fetch_one(&pool)
        .await
        .expect("source terminal snapshot");
        let request = create_request(&pool, &fixture, 1).await;
        let claim_token = Uuid::new_v4();
        let claimed = claim_pending(&pool, "worker-dispatch", claim_token, 100, 60)
            .await
            .expect("claim dispatch request");
        assert!(claimed.iter().any(|row| row.id == request.id));
        let task = sqlx::query_as::<_, crate::models::DevRailTaskRow>(AssertSqlSafe(format!(
            "SELECT {TASK_COLUMNS} FROM devrail_tasks WHERE id=$1"
        )))
        .bind(fixture.task_id)
        .fetch_one(&pool)
        .await
        .expect("continuation task");
        let snapshot_id =
            sqlx::query_scalar::<_, i64>("SELECT snapshot_id FROM devrail_runs WHERE id=$1")
                .bind(fixture.source_run_id)
                .fetch_one(&pool)
                .await
                .expect("source snapshot");
        let workflow_snapshot = task
            .dispatch_snapshot
            .get("workflow")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let policy = json!({"version":"test-v1"});
        let startup_args = json!(["app-server"]);
        let mut dispatch_tx = pool.begin().await.expect("begin dispatch");
        let child = crate::repositories::devrail_runs::create_continuation_run(
            &mut dispatch_tx,
            &crate::repositories::devrail_runs::NewContinuationRun {
                actor: &fixture.actor,
                task_id: fixture.task_id,
                snapshot_id,
                idempotency_key: "continuation:dispatch-test",
                task_revision: task.revision,
                workflow_source: &task.workflow_source,
                workflow_version: &task.workflow_version,
                workflow_digest: &task.workflow_digest,
                workflow_snapshot: &workflow_snapshot,
                parent_run_id: fixture.source_run_id,
                parent_turn_id: &fixture.source_turn_id,
                thread_id: "thread-dispatch-test",
                continuation_request_id: request.id,
                continuation_sequence: request.continuation_sequence,
                harness_start_key: "continuation:dispatch-test:start",
                cwd: "/tmp/continuation-child",
                policy: &policy,
                startup_args: &startup_args,
                model_id: None,
                department_id: fixture.actor.department_id,
            },
        )
        .await
        .expect("create continuation child")
        .expect("unique continuation child");
        assert!(mark_dispatched(
            &mut dispatch_tx,
            &fixture.actor,
            request.id,
            "worker-dispatch",
            claim_token,
            child.id,
        )
        .await
        .expect("mark continuation dispatched")
        .is_some());
        dispatch_tx.commit().await.expect("commit dispatch");
        let running = sqlx::query_as::<_, (String, i64)>(
            "SELECT status,
                    (SELECT count(*) FROM devrail_task_status_history h
                     WHERE h.task_id=t.id AND h.continuation_request_id=$2
                       AND h.child_run_id=$3 AND h.reason='continuation_dispatched'
                       AND h.to_status='running')
             FROM devrail_tasks t WHERE t.id=$1",
        )
        .bind(fixture.task_id)
        .bind(request.id)
        .bind(child.id)
        .fetch_one(&pool)
        .await
        .expect("dispatched task projection");
        assert_eq!(running, ("running".to_string(), 1));

        let mut terminal_tx = pool.begin().await.expect("begin child terminal");
        assert!(crate::repositories::devrail_runs::update_run_terminal(
            &mut terminal_tx,
            &crate::repositories::devrail_runs::TerminalRunUpdate {
                run_id: child.id,
                status: "completed",
                exit_reason: "completed",
                exit_code: Some(0),
                stderr_summary: None,
                trace_id: "continuation-terminal-test",
                recovery_suggestion: None,
            },
        )
        .await
        .expect("terminal child run"));
        assert!(complete_for_child_run(
            &mut terminal_tx,
            &fixture.actor,
            child.id,
            "completed",
            "succeeded",
        )
        .await
        .expect("project continuation terminal")
        .is_some());
        terminal_tx.commit().await.expect("commit child terminal");
        let mut replay_tx = pool.begin().await.expect("begin terminal replay");
        assert!(complete_for_child_run(
            &mut replay_tx,
            &fixture.actor,
            child.id,
            "completed",
            "succeeded",
        )
        .await
        .expect("replay continuation terminal")
        .is_none());
        replay_tx.commit().await.expect("commit terminal replay");
        let terminal_projection = sqlx::query_as::<_, (String, String, i64)>(
            "SELECT t.status,r.status,
                    (SELECT count(*) FROM devrail_task_status_history h
                     WHERE h.task_id=t.id AND h.continuation_request_id=$2
                       AND h.child_run_id=$3 AND h.reason='continuation_completed'
                       AND h.to_status='succeeded')
             FROM devrail_tasks t
             JOIN devrail_continuation_requests r ON r.task_id=t.id
             WHERE t.id=$1 AND r.id=$2",
        )
        .bind(fixture.task_id)
        .bind(request.id)
        .bind(child.id)
        .fetch_one(&pool)
        .await
        .expect("terminal continuation projection");
        assert_eq!(
            terminal_projection,
            ("succeeded".to_string(), "completed".to_string(), 1)
        );
        let source_after = sqlx::query_as::<_, (String, Option<DateTime<Utc>>)>(
            "SELECT status,completed_at FROM devrail_runs WHERE id=$1",
        )
        .bind(fixture.source_run_id)
        .fetch_one(&pool)
        .await
        .expect("source terminal after continuation");
        assert_eq!(source_after, source_terminal);
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup continuation dispatch schema");
    }

    #[tokio::test]
    async fn concurrent_child_creation_reuses_one_run_and_preserves_source_terminal() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = fixture(&pool).await;
        let source_before = sqlx::query_as::<_, (String, Option<DateTime<Utc>>)>(
            "SELECT status,completed_at FROM devrail_runs WHERE id=$1",
        )
        .bind(fixture.source_run_id)
        .fetch_one(&pool)
        .await
        .expect("source terminal before concurrent create");
        let request = create_request(&pool, &fixture, 1).await;
        let (first, second) = tokio::join!(
            create_child_for_request(
                &pool,
                &fixture,
                &request,
                "continuation:concurrent-a",
                "continuation:concurrent-a:start",
            ),
            create_child_for_request(
                &pool,
                &fixture,
                &request,
                "continuation:concurrent-b",
                "continuation:concurrent-b:start",
            )
        );
        assert_eq!(first.id, second.id);
        assert_eq!(first.run_kind, "continuation");
        assert_eq!(first.parent_run_id, Some(fixture.source_run_id));
        assert_eq!(
            first.parent_turn_id.as_deref(),
            Some(fixture.source_turn_id.as_str())
        );
        assert_eq!(first.root_run_id, Some(fixture.source_run_id));
        assert_eq!(first.continuation_request_id, Some(request.id));
        assert_eq!(
            first.continuation_sequence,
            Some(request.continuation_sequence)
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM devrail_runs WHERE continuation_request_id=$1",
            )
            .bind(request.id)
            .fetch_one(&pool)
            .await
            .expect("unique continuation run count"),
            1
        );
        let source_after = sqlx::query_as::<_, (String, Option<DateTime<Utc>>)>(
            "SELECT status,completed_at FROM devrail_runs WHERE id=$1",
        )
        .bind(fixture.source_run_id)
        .fetch_one(&pool)
        .await
        .expect("source terminal after concurrent create");
        assert_eq!(source_after, source_before);
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup concurrent continuation schema");
    }

    #[tokio::test]
    async fn restart_reconciles_threadless_continuation_child_terminally() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = fixture(&pool).await;
        let request = create_request(&pool, &fixture, 1).await;
        let claim_token = Uuid::new_v4();
        assert!(claim_pending(&pool, "worker-restart", claim_token, 100, 60)
            .await
            .expect("claim restart continuation")
            .iter()
            .any(|row| row.id == request.id));
        let child = create_child_for_request(
            &pool,
            &fixture,
            &request,
            "continuation:restart-child",
            "continuation:restart-child:start",
        )
        .await;
        let mut dispatch_tx = pool.begin().await.expect("begin restart dispatch");
        assert!(mark_dispatched(
            &mut dispatch_tx,
            &fixture.actor,
            request.id,
            "worker-restart",
            claim_token,
            child.id,
        )
        .await
        .expect("mark restart continuation dispatched")
        .is_some());
        dispatch_tx.commit().await.expect("commit restart dispatch");
        sqlx::query("UPDATE devrail_runs SET thread_id=NULL, turn_id=NULL WHERE id=$1")
            .bind(child.id)
            .execute(&pool)
            .await
            .expect("remove child thread for restart");

        crate::repositories::devrail_runs::mark_unrecoverable_runs(&pool)
            .await
            .expect("reconcile threadless continuation child");
        let state = sqlx::query_as::<_, (String, String, String, i64)>(
            "SELECT t.status, r.status, r.result_code,
                    (SELECT count(*) FROM devrail_task_status_history h
                     WHERE h.task_id=t.id AND h.continuation_request_id=$2
                       AND h.reason='continuation_completed')
             FROM devrail_tasks t
             JOIN devrail_continuation_requests r ON r.task_id=t.id
             WHERE t.id=$1 AND r.id=$2",
        )
        .bind(fixture.task_id)
        .bind(request.id)
        .fetch_one(&pool)
        .await
        .expect("read restart continuation state");
        assert_eq!(
            state,
            (
                "failed".to_string(),
                "completed".to_string(),
                "supervisor_restart".to_string(),
                1
            )
        );
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup continuation restart schema");
    }

    #[tokio::test]
    async fn handoff_is_scoped_immutable_and_rejects_digest_drift() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = fixture(&pool).await;
        assert!(find_handoff(&pool, &fixture.actor, fixture.source_run_id)
            .await
            .expect("historical handoff lookup")
            .is_none());
        let task_digest = "1".repeat(64);
        let workflow_digest = "2".repeat(64);
        let environment_digest = "3".repeat(64);
        let repository_digest = "4".repeat(64);
        let changeset_digest = "5".repeat(64);
        let base_commit = "a".repeat(40);
        let head_commit = "b".repeat(40);
        let tool_versions = json!({"git":"2.51.0","codex":"0.1.0"});
        let validated_at = Utc::now();
        let input = NewRunHandoff {
            actor: &fixture.actor,
            project_id: fixture.project_id,
            task_id: fixture.task_id,
            source_run_id: fixture.source_run_id,
            task_snapshot_id: fixture.snapshot_id,
            repository_id: fixture.repository_id,
            environment_id: Some(fixture.environment_id),
            task_snapshot_digest: &task_digest,
            workflow_snapshot_digest: &workflow_digest,
            environment_snapshot_digest: Some(&environment_digest),
            repository_identity: "repository:test:continuation",
            repository_identity_digest: &repository_digest,
            base_commit: &base_commit,
            head_commit: Some(&head_commit),
            branch_ref: Some("refs/heads/devrail-continuation"),
            changeset_ref: Some("handoffs/run.patch"),
            changeset_digest: &changeset_digest,
            tool_versions: &tool_versions,
            evidence_status: "available",
            error_code: None,
            validated_at: Some(validated_at),
        };
        let mut first_tx = pool.begin().await.expect("begin handoff create");
        let (first, created) = create_handoff(&mut first_tx, &input)
            .await
            .expect("create handoff");
        assert!(created);
        first_tx.commit().await.expect("commit handoff create");
        let mut replay_tx = pool.begin().await.expect("begin handoff replay");
        let (replayed, created) = create_handoff(&mut replay_tx, &input)
            .await
            .expect("replay handoff");
        assert!(!created);
        assert_eq!(replayed.id, first.id);
        replay_tx.commit().await.expect("commit handoff replay");

        let drifted_digest = "6".repeat(64);
        let drifted = NewRunHandoff {
            changeset_digest: &drifted_digest,
            ..input
        };
        let mut drift_tx = pool.begin().await.expect("begin handoff drift");
        assert!(matches!(
            create_handoff(&mut drift_tx, &drifted).await,
            Err(sqlx::Error::Protocol(message)) if message.contains("不可变")
        ));
        drift_tx.rollback().await.expect("rollback handoff drift");
        assert_eq!(
            find_handoff(&pool, &fixture.actor, fixture.source_run_id)
                .await
                .expect("scoped handoff")
                .map(|row| row.id),
            Some(first.id)
        );
        let other_actor = other_organization_actor(&pool).await;
        assert!(find_handoff(&pool, &other_actor, fixture.source_run_id)
            .await
            .expect("cross organization handoff")
            .is_none());
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup continuation handoff schema");
    }

    #[tokio::test]
    async fn continuation_transaction_rolls_back_when_outbox_write_fails() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = fixture(&pool).await;
        let digest = "7".repeat(64);
        let policy_snapshot = json!({"enabled":true});
        let evidence_ref = format!("user:{}:fault", fixture.actor.user_id);
        let mut tx = pool.begin().await.expect("begin fault injection");
        sqlx::raw_sql(
            "CREATE FUNCTION devrail_test_fail_continuation_outbox()
             RETURNS TRIGGER LANGUAGE plpgsql AS $$
             BEGIN
                 IF NEW.event_type = 'devrail.continuation.created' THEN
                     RAISE EXCEPTION 'simulated continuation outbox failure';
                 END IF;
                 RETURN NEW;
             END;
             $$;
             CREATE TRIGGER trg_devrail_test_fail_continuation_outbox
             BEFORE INSERT ON devrail_outbox_events
             FOR EACH ROW EXECUTE FUNCTION devrail_test_fail_continuation_outbox();",
        )
        .execute(&mut *tx)
        .await
        .expect("install transactional fault trigger");
        let result = create(
            &mut tx,
            &NewContinuation {
                actor: &fixture.actor,
                project_id: fixture.project_id,
                task_id: fixture.task_id,
                source_run_id: fixture.source_run_id,
                root_run_id: fixture.source_run_id,
                source_turn_id: &fixture.source_turn_id,
                requested_by_user_id: fixture.actor.user_id,
                trigger_type: "user_context",
                evidence_ref: &evidence_ref,
                evidence_digest: &digest,
                evidence_observed_at: Utc::now(),
                evidence_expires_at: None,
                changeset_digest: None,
                redacted_context: "请验证事务回滚",
                context_summary: "事务故障注入",
                input_digest: &digest,
                idempotency_key: "fault-injection",
                continuation_sequence: 1,
                chain_depth: 1,
                prior_task_status: "succeeded",
                expected_task_revision: 1,
                policy_version: "test-v1",
                policy_snapshot: &policy_snapshot,
            },
        )
        .await;
        assert!(matches!(result, Err(sqlx::Error::Database(_))));
        tx.rollback().await.expect("rollback injected failure");
        let facts = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64)>(
            "SELECT t.status,
                    (SELECT count(*) FROM devrail_continuation_requests r WHERE r.task_id=t.id),
                    (SELECT count(*) FROM devrail_task_status_history h
                     WHERE h.task_id=t.id AND h.continuation_request_id IS NOT NULL),
                    (SELECT count(*) FROM devrail_task_events e
                     WHERE e.task_id=t.id AND e.event_type='continuation.created'),
                    (SELECT count(*) FROM audit_logs a
                     WHERE a.target_type='devrail_continuation_request'
                       AND a.details->>'taskId'=$2),
                    (SELECT count(*) FROM devrail_outbox_events o
                     WHERE o.organization_id=$3
                       AND o.aggregate_type='devrail_continuation_request'
                       AND o.payload->>'deepLink'=$4)
             FROM devrail_tasks t WHERE t.id=$1",
        )
        .bind(fixture.task_id)
        .bind(fixture.task_id.to_string())
        .bind(fixture.actor.organization_id)
        .bind(format!("/devrail/tasks/{}", fixture.task_id))
        .fetch_one(&pool)
        .await
        .expect("rolled back continuation facts");
        assert_eq!(facts, ("succeeded".to_string(), 0, 0, 0, 0, 0));
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup continuation rollback schema");
    }

    #[test]
    fn handoff_validation_rejects_sensitive_fields_and_absolute_paths() {
        let actor = ActorContext {
            actor_type: ActorType::System,
            user_id: 1,
            session_id: 0,
            organization_id: 1,
            department_id: Some(1),
            data_scope: DataScope::All,
            permission_codes: BTreeSet::new(),
        };
        let digest = "a".repeat(64);
        let base_commit = "b".repeat(40);
        let head_commit = "c".repeat(40);
        let sensitive_tools = json!({"api_token":"hidden-value"});
        let sensitive = NewRunHandoff {
            actor: &actor,
            project_id: 1,
            task_id: 1,
            source_run_id: 1,
            task_snapshot_id: 1,
            repository_id: 1,
            environment_id: Some(1),
            task_snapshot_digest: &digest,
            workflow_snapshot_digest: &digest,
            environment_snapshot_digest: Some(&digest),
            repository_identity: "repository:test",
            repository_identity_digest: &digest,
            base_commit: &base_commit,
            head_commit: Some(&head_commit),
            branch_ref: Some("refs/heads/test"),
            changeset_ref: Some("handoffs/run.patch"),
            changeset_digest: &digest,
            tool_versions: &sensitive_tools,
            evidence_status: "available",
            error_code: None,
            validated_at: Some(Utc::now()),
        };
        assert!(matches!(
            validate_handoff(&sensitive),
            Err(sqlx::Error::Protocol(message)) if message.contains("敏感信息")
        ));
        let safe_tools = json!({"git":"2.51.0"});
        let absolute_path = NewRunHandoff {
            changeset_ref: Some("/controlled/workspace/run.patch"),
            tool_versions: &safe_tools,
            ..sensitive
        };
        assert!(matches!(
            validate_handoff(&absolute_path),
            Err(sqlx::Error::Protocol(message)) if message.contains("证据引用无效")
        ));
    }

    #[tokio::test]
    async fn stale_task_version_and_active_run_leave_no_continuation_facts() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = fixture(&pool).await;
        sqlx::query("UPDATE devrail_tasks SET revision=revision+1 WHERE id=$1")
            .bind(fixture.task_id)
            .execute(&pool)
            .await
            .expect("advance task revision");
        let stale = create_request_result(&pool, &fixture, 1, 1).await;
        assert!(
            matches!(stale, Err(sqlx::Error::Protocol(message)) if message.contains("谱系不匹配"))
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM devrail_continuation_requests WHERE task_id=$1",
            )
            .bind(fixture.task_id)
            .fetch_one(&pool)
            .await
            .expect("stale request count"),
            0
        );
        sqlx::query("UPDATE devrail_tasks SET revision=1 WHERE id=$1")
            .bind(fixture.task_id)
            .execute(&pool)
            .await
            .expect("restore task revision");
        let active_run_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO devrail_runs
                 (organization_id,department_id,owner_user_id,task_id,snapshot_id,
                  idempotency_key,attempt,status,cwd)
             SELECT organization_id,department_id,owner_user_id,task_id,snapshot_id,
                    $2,2,'active','/tmp/continuation-active'
             FROM devrail_runs WHERE id=$1 RETURNING id",
        )
        .bind(fixture.source_run_id)
        .bind(format!("active-{}", Uuid::new_v4().simple()))
        .fetch_one(&pool)
        .await
        .expect("active run");
        let active_conflict = create_request_result(&pool, &fixture, 1, 1).await;
        assert!(
            matches!(active_conflict, Err(sqlx::Error::Protocol(message)) if message.contains("活动运行"))
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM devrail_task_status_history
                 WHERE task_id=$1 AND continuation_request_id IS NOT NULL",
            )
            .bind(fixture.task_id)
            .fetch_one(&pool)
            .await
            .expect("conflict history count"),
            0
        );
        sqlx::query("DELETE FROM devrail_runs WHERE id=$1")
            .bind(active_run_id)
            .execute(&pool)
            .await
            .expect("remove active test run");
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup continuation stale schema");
    }

    #[tokio::test]
    async fn continuation_permission_seed_is_idempotent_and_explicit() {
        let _guard = DATABASE_TEST_LOCK.lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        let seed = include_str!(
            "../../migrations/20260907110000_add_devrail_continuation_permissions.sql"
        );
        sqlx::raw_sql(seed)
            .execute(&pool)
            .await
            .expect("replay continuation permission seed once");
        sqlx::raw_sql(seed)
            .execute(&pool)
            .await
            .expect("replay continuation permission seed twice");
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT r.code,p.code
             FROM roles r
             JOIN role_permissions rp ON rp.role_id=r.id
             JOIN permissions p ON p.id=rp.permission_id
             WHERE r.code IN ('super_admin','editor','viewer','compliance_auditor','support_tier2','billing_manager')
               AND p.code LIKE 'devrail:continuation:%'
             ORDER BY r.code,p.code",
        )
        .fetch_all(&pool)
        .await
        .expect("continuation permission matrix");
        let mut matrix = BTreeMap::<String, BTreeSet<String>>::new();
        for (role, permission) in rows {
            matrix.entry(role).or_default().insert(permission);
        }
        let all = BTreeSet::from([
            "devrail:continuation:cancel".to_string(),
            "devrail:continuation:create".to_string(),
            "devrail:continuation:read".to_string(),
        ]);
        let read_only = BTreeSet::from(["devrail:continuation:read".to_string()]);
        assert_eq!(matrix.get("super_admin"), Some(&all));
        assert_eq!(matrix.get("editor"), Some(&all));
        for role in [
            "viewer",
            "compliance_auditor",
            "support_tier2",
            "billing_manager",
        ] {
            assert_eq!(matrix.get(role), Some(&read_only), "role {role}");
        }
    }

    async fn other_organization_actor(pool: &PgPool) -> ActorContext {
        let suffix = Uuid::new_v4().simple().to_string();
        let organization_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO organizations (code,name) VALUES ($1,'隔离测试组织') RETURNING id",
        )
        .bind(format!("isolation-{suffix}"))
        .fetch_one(pool)
        .await
        .expect("other organization");
        let department_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO departments (organization_id,code,name) VALUES ($1,'root','根部门') RETURNING id",
        )
        .bind(organization_id)
        .fetch_one(pool)
        .await
        .expect("other department");
        let user_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO users (username,password_hash,display_name,organization_id,department_id)
             VALUES ($1,'test','隔离用户',$2,$3) RETURNING id",
        )
        .bind(format!("isolation-{suffix}"))
        .bind(organization_id)
        .bind(department_id)
        .fetch_one(pool)
        .await
        .expect("other user");
        ActorContext {
            actor_type: ActorType::User,
            user_id,
            session_id: 1,
            organization_id,
            department_id: Some(department_id),
            data_scope: DataScope::Organization,
            permission_codes: BTreeSet::new(),
        }
    }
}
