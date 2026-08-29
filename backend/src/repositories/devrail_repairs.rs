//! Scoped persistence and transactional lifecycle facts for controlled repair runs.

use crate::access::ActorContext;
use crate::models::{
    DevRailRepairApprovalRow, DevRailRepairDiagnosisRow, DevRailRepairGateRerunRow,
    DevRailRepairHandoffRow, DevRailRepairRequestRow, DevRailTaskRow,
};
use crate::repositories::{audit_logs, devrail, devrail_notifications, devrail_workspaces};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{AssertSqlSafe, PgConnection, PgPool, Row};
use uuid::Uuid;

const REQUEST_COLUMNS: &str = "r.id, r.organization_id, r.department_id, r.owner_user_id, r.project_id, r.task_id, r.source_run_id, r.root_run_id, r.diagnosis_id, r.failure_evidence_ref, r.failure_evidence_digest, r.changeset_digest, r.idempotency_key, r.repair_sequence, r.risk_category, r.strategy_version, r.policy_snapshot, r.source_task_status, r.status, r.status_version, r.claim_owner, r.claim_token, r.claim_expires_at, r.dispatch_attempts, r.next_attempt_at, r.child_run_id, r.cost_units, r.result_code, r.handoff_reason, r.created_at, r.updated_at, r.claimed_at, r.dispatched_at, r.completed_at, r.cancelled_at";
const DIAGNOSIS_COLUMNS: &str = "d.id, d.organization_id, d.department_id, d.owner_user_id, d.project_id, d.task_id, d.source_run_id, d.evidence_ref, d.evidence_digest, d.evidence_observed_at, d.evidence_expires_at, d.affected_gates, d.error_summary, d.structured_error, d.log_ref, d.changeset_digest, d.environment_summary, d.created_at";
const DIAGNOSIS_INSERT_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, project_id, task_id, source_run_id, evidence_ref, evidence_digest, evidence_observed_at, evidence_expires_at, affected_gates, error_summary, structured_error, log_ref, changeset_digest, environment_summary, created_at";
const GATE_RERUN_COLUMNS: &str = "g.id, g.organization_id, g.department_id, g.owner_user_id, g.project_id, g.task_id, g.repair_request_id, g.child_run_id, g.gate_id, g.changeset_digest, g.idempotency_key, g.status, g.claim_owner, g.claim_token, g.claim_expires_at, g.result_code, g.log_ref, g.summary, g.duration_ms, g.created_at, g.updated_at, g.started_at, g.completed_at";
const GATE_RERUN_INSERT_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, project_id, task_id, repair_request_id, child_run_id, gate_id, changeset_digest, idempotency_key, status, claim_owner, claim_token, claim_expires_at, result_code, log_ref, summary, duration_ms, created_at, updated_at, started_at, completed_at";
const HANDOFF_COLUMNS: &str = "h.id, h.organization_id, h.department_id, h.owner_user_id, h.project_id, h.task_id, h.repair_request_id, h.reason_code, h.recommendation, h.status, h.resolved_by, h.resolved_at, h.created_at";
const APPROVAL_COLUMNS: &str = "a.id, a.organization_id, a.department_id, a.owner_user_id, a.project_id, a.task_id, a.repair_request_id, a.idempotency_key, a.risk_category, a.policy_version, a.status, a.requested_by, a.decided_by, a.decision_reason, a.expires_at, a.created_at, a.updated_at";
const APPROVAL_INSERT_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, project_id, task_id, repair_request_id, idempotency_key, risk_category, policy_version, status, requested_by, decided_by, decision_reason, expires_at, created_at, updated_at";
const TASK_COLUMNS: &str = "id, organization_id, department_id, owner_user_id, project_id, repository_id, environment_id, assignee_user_id, title, goal, background, acceptance_criteria, constraints, priority, status, revision, dispatch_snapshot, dispatch_snapshot_digest, workflow_source, workflow_version, workflow_digest, scheduler_attempt, scheduler_retry_count, scheduler_max_attempts, scheduler_retry_at, scheduler_last_error, hook_failure_fingerprint, hook_failure_count, creation_source, source_task_id, source_run_id, followup_depth, labels, due_at, created_at, updated_at, archived_at";

pub(crate) struct NewRepairDiagnosis<'a> {
    pub evidence_ref: &'a str,
    pub evidence_digest: &'a str,
    pub evidence_observed_at: DateTime<Utc>,
    pub evidence_expires_at: Option<DateTime<Utc>>,
    pub affected_gates: &'a Value,
    pub error_summary: &'a str,
    pub structured_error: &'a Value,
    pub log_ref: Option<&'a str>,
    pub changeset_digest: Option<&'a str>,
    pub environment_summary: &'a Value,
}

struct DiagnosisContext<'a> {
    actor: &'a ActorContext,
    project_id: i64,
    task_id: i64,
    source_run_id: i64,
    department_id: Option<i64>,
    owner_user_id: i64,
    input: &'a NewRepairDiagnosis<'a>,
}

pub(crate) struct NewRepairRequest<'a> {
    pub actor: &'a ActorContext,
    pub task_id: i64,
    pub source_run_id: i64,
    pub idempotency_key: &'a str,
    pub risk_category: &'a str,
    pub strategy_version: &'a str,
    pub policy_snapshot: &'a Value,
    pub max_repairs: i16,
    pub cost_units: u32,
    pub retry_of_request_id: Option<i64>,
    pub diagnosis: NewRepairDiagnosis<'a>,
}

pub(crate) struct NewRepairHandoff<'a> {
    pub reason_code: &'a str,
    pub recommendation: &'a str,
}

pub struct NewRepairGateRerun<'a> {
    pub request_id: i64,
    pub gate_id: &'a str,
    pub changeset_digest: &'a str,
    pub idempotency_key: &'a str,
    pub child_run_id: Option<i64>,
}

pub struct CompletedRepairGateRerun<'a> {
    pub id: i64,
    pub worker_id: &'a str,
    pub claim_token: Uuid,
    pub status: &'a str,
    pub result_code: Option<&'a str>,
    pub summary: Option<&'a str>,
    pub log_ref: Option<&'a str>,
    pub duration_ms: Option<i64>,
}

struct RepairHistoryContext<'a> {
    actor: &'a ActorContext,
    reason: &'a str,
    request_id: i64,
    diagnosis_id: i64,
    source_run_id: i64,
    child_run_id: Option<i64>,
    policy_version: &'a str,
    result_code: Option<&'a str>,
}

pub(crate) struct NewRepairApproval<'a> {
    pub request_id: i64,
    pub idempotency_key: &'a str,
    pub risk_category: &'a str,
    pub policy_version: &'a str,
    pub requested_by: i64,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub(crate) struct FailedQualityGateEvidence {
    pub event_id: i64,
    pub gate_id: String,
    pub log_ref: Option<String>,
    pub changeset_digest: Option<String>,
    pub observed_at: DateTime<Utc>,
}

fn visible_departments_cte() -> &'static str {
    "WITH RECURSIVE visible_departments AS (
         SELECT id FROM departments WHERE id = $4 AND organization_id = $2
         UNION
         SELECT child.id FROM departments child
         JOIN visible_departments parent ON child.parent_id = parent.id
         WHERE child.organization_id = $2
     )"
}

fn scoped(alias: &str) -> String {
    format!(
        "{alias}.organization_id = $2 AND ($1 = 'all' OR $1 = 'organization' OR ($1 = 'self' AND {alias}.owner_user_id = $3) OR ($1 = 'department' AND {alias}.department_id = $4) OR ($1 = 'department_and_children' AND {alias}.department_id IN (SELECT id FROM visible_departments)))"
    )
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_identifier(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'_' | b'.' | b':')
        })
}

async fn lifecycle_notification(
    connection: &mut PgConnection,
    request: &DevRailRepairRequestRow,
    event_type: &str,
    level: &str,
    title: &str,
    summary: &str,
    deep_link: &str,
) -> Result<(), sqlx::Error> {
    let source_key = format!("repair:{}:{event_type}", request.id);
    let notification_id = devrail_notifications::create_or_get(
        connection,
        &devrail_notifications::NewNotification {
            organization_id: request.organization_id,
            department_id: request.department_id,
            recipient_user_id: request.owner_user_id,
            event_type,
            level,
            title,
            summary,
            resource_type: Some("devrail_repair_request"),
            resource_id: Some(request.id),
            deep_link: Some(deep_link),
            source_key: &source_key,
        },
    )
    .await?;
    devrail_notifications::outbox(
        connection,
        request.organization_id,
        event_type,
        "devrail_repair_request",
        Some(request.id),
        &serde_json::json!({
            "notificationId": notification_id,
            "eventType": event_type,
            "summary": summary,
            "deepLink": deep_link,
        }),
    )
    .await
}

fn safe_reference(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && !value.starts_with('/')
        && !value.starts_with("~/")
        && !value.contains("..")
        && !value.contains('\\')
        && !value.bytes().any(|byte| byte.is_ascii_control())
}

fn sensitive_text(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("begin rsa private key")
        || lower.contains("begin openssh private key")
        || value.lines().any(|line| {
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
                "headers",
                "command",
                "stdout",
                "stderr",
                "raw_log",
                "raw_output",
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

fn validate_diagnosis(input: &NewRepairDiagnosis<'_>) -> Result<(), sqlx::Error> {
    if !safe_reference(input.evidence_ref, 256)
        || !valid_digest(input.evidence_digest)
        || input.evidence_observed_at > Utc::now()
        || input
            .evidence_expires_at
            .is_some_and(|value| value <= input.evidence_observed_at || value <= Utc::now())
        || input.error_summary.is_empty()
        || input.error_summary.len() > 512
        || sensitive_text(input.error_summary)
        || input
            .log_ref
            .is_some_and(|value| !safe_reference(value, 256))
        || input
            .changeset_digest
            .is_some_and(|value| !valid_digest(value))
        || sensitive_json(input.affected_gates)
        || sensitive_json(input.structured_error)
        || sensitive_json(input.environment_summary)
        || serde_json::to_vec(input.affected_gates).map_or(true, |value| value.len() > 4096)
        || serde_json::to_vec(input.structured_error).map_or(true, |value| value.len() > 16 * 1024)
        || serde_json::to_vec(input.environment_summary)
            .map_or(true, |value| value.len() > 8 * 1024)
    {
        return Err(sqlx::Error::Protocol(
            "repair 诊断包含无效引用、敏感内容或超长字段".to_string(),
        ));
    }
    let Some(gates) = input.affected_gates.as_array() else {
        return Err(sqlx::Error::Protocol(
            "repair 诊断缺少受影响门禁".to_string(),
        ));
    };
    if gates.is_empty()
        || gates.len() > 16
        || gates.iter().any(|gate| {
            !gate
                .as_str()
                .is_some_and(|value| valid_identifier(value, 64))
        })
    {
        return Err(sqlx::Error::Protocol("repair 门禁标识无效".to_string()));
    }
    Ok(())
}

fn validate_request(input: &NewRepairRequest<'_>) -> Result<(), sqlx::Error> {
    if !valid_identifier(input.idempotency_key, 160)
        || !valid_identifier(input.risk_category, 32)
        || !valid_identifier(input.strategy_version, 128)
        || !matches!(
            input.risk_category,
            "low_risk"
                | "logical_change"
                | "dependency_change"
                | "remote_write"
                | "security_change"
                | "forbidden"
        )
        || !(1..=5).contains(&input.max_repairs)
        || input.cost_units == 0
        || serde_json::to_vec(input.policy_snapshot).map_or(true, |value| value.len() > 16 * 1024)
        || sensitive_json(input.policy_snapshot)
    {
        return Err(sqlx::Error::Protocol("repair 请求字段无效".to_string()));
    }
    validate_diagnosis(&input.diagnosis)
}

async fn create_diagnosis(
    connection: &mut PgConnection,
    context: &DiagnosisContext<'_>,
) -> Result<DevRailRepairDiagnosisRow, sqlx::Error> {
    let sql = format!(
        "INSERT INTO devrail_repair_diagnoses (organization_id,department_id,owner_user_id,project_id,task_id,source_run_id,evidence_ref,evidence_digest,evidence_observed_at,evidence_expires_at,affected_gates,error_summary,structured_error,log_ref,changeset_digest,environment_summary) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16) ON CONFLICT (organization_id,source_run_id,evidence_ref) DO NOTHING RETURNING {DIAGNOSIS_INSERT_COLUMNS}"
    );
    if let Some(created) = sqlx::query_as::<_, DevRailRepairDiagnosisRow>(AssertSqlSafe(sql))
        .bind(context.actor.organization_id)
        .bind(context.department_id)
        .bind(context.owner_user_id)
        .bind(context.project_id)
        .bind(context.task_id)
        .bind(context.source_run_id)
        .bind(context.input.evidence_ref)
        .bind(context.input.evidence_digest)
        .bind(context.input.evidence_observed_at)
        .bind(context.input.evidence_expires_at)
        .bind(context.input.affected_gates)
        .bind(context.input.error_summary)
        .bind(context.input.structured_error)
        .bind(context.input.log_ref)
        .bind(context.input.changeset_digest)
        .bind(context.input.environment_summary)
        .fetch_optional(&mut *connection)
        .await?
    {
        return Ok(created);
    }
    let existing = sqlx::query_as::<_, DevRailRepairDiagnosisRow>(AssertSqlSafe(format!(
        "SELECT {DIAGNOSIS_COLUMNS} FROM devrail_repair_diagnoses d WHERE d.organization_id=$1 AND d.source_run_id=$2 AND d.evidence_ref=$3 FOR UPDATE"
    )))
    .bind(context.actor.organization_id)
    .bind(context.source_run_id)
    .bind(context.input.evidence_ref)
    .fetch_one(&mut *connection)
    .await?;
    if existing.evidence_digest != context.input.evidence_digest
        || existing.evidence_observed_at != context.input.evidence_observed_at
        || existing.evidence_expires_at != context.input.evidence_expires_at
        || existing.affected_gates != *context.input.affected_gates
        || existing.error_summary != context.input.error_summary
        || existing.structured_error != *context.input.structured_error
        || existing.log_ref.as_deref() != context.input.log_ref
        || existing.changeset_digest.as_deref() != context.input.changeset_digest
        || existing.environment_summary != *context.input.environment_summary
    {
        return Err(sqlx::Error::Protocol(
            "repair 诊断快照不可变且证据不匹配".to_string(),
        ));
    }
    Ok(existing)
}

pub(crate) async fn create_or_get(
    connection: &mut PgConnection,
    input: &NewRepairRequest<'_>,
) -> Result<(DevRailRepairRequestRow, bool), sqlx::Error> {
    validate_request(input)?;
    let identity_filter = if input.retry_of_request_id.is_some() {
        "r.idempotency_key=$6"
    } else {
        "r.idempotency_key=$6 OR (r.source_run_id=$7 AND r.failure_evidence_ref=$8)"
    };
    let existing_sql = format!(
        "{} SELECT {REQUEST_COLUMNS} FROM devrail_repair_requests r WHERE r.task_id=$5 AND ({identity_filter}) AND {} ORDER BY CASE WHEN r.idempotency_key=$6 THEN 0 ELSE 1 END,r.id LIMIT 1 FOR UPDATE",
        visible_departments_cte(),
        scoped("r")
    );
    if let Some(existing) =
        sqlx::query_as::<_, DevRailRepairRequestRow>(AssertSqlSafe(existing_sql))
            .bind(input.actor.data_scope.as_str())
            .bind(input.actor.organization_id)
            .bind(input.actor.user_id)
            .bind(input.actor.department_id)
            .bind(input.task_id)
            .bind(input.idempotency_key)
            .bind(input.source_run_id)
            .bind(input.diagnosis.evidence_ref)
            .fetch_optional(&mut *connection)
            .await?
    {
        if existing.source_run_id != input.source_run_id
            || existing.failure_evidence_digest != input.diagnosis.evidence_digest
            || existing.risk_category != input.risk_category
            || existing.idempotency_key == input.idempotency_key
                && existing.failure_evidence_ref != input.diagnosis.evidence_ref
        {
            return Err(sqlx::Error::Protocol(
                "repair 幂等键对应不同请求".to_string(),
            ));
        }
        return Ok((existing, false));
    }

    let source_sql = format!(
        "{} SELECT t.status AS task_status, t.revision, t.current_repair_request_id, t.project_id, t.department_id AS task_department_id, t.owner_user_id AS task_owner_user_id, r.status AS run_status, COALESCE(r.root_run_id,r.id) AS root_run_id FROM devrail_tasks t JOIN devrail_runs r ON r.task_id=t.id AND r.organization_id=t.organization_id WHERE t.id=$5 AND r.id=$6 AND {} AND {} FOR UPDATE OF t,r",
        visible_departments_cte(),
        scoped("t"),
        scoped("r")
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
    let current_repair_request_id: Option<i64> = source.try_get("current_repair_request_id")?;
    let project_id: i64 = source.try_get("project_id")?;
    let department_id: Option<i64> = source.try_get("task_department_id")?;
    let owner_user_id: i64 = source.try_get("task_owner_user_id")?;
    let run_status: String = source.try_get("run_status")?;
    let root_run_id: i64 = source.try_get("root_run_id")?;
    let valid_task_state = match task_status.as_str() {
        "failed" => input.retry_of_request_id.is_none(),
        "repair_handoff" => input.retry_of_request_id == current_repair_request_id,
        _ => false,
    };
    if !valid_task_state || run_status != "failed" {
        return Err(sqlx::Error::Protocol(
            "repair 来源必须是终态失败任务和运行".to_string(),
        ));
    }
    if sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM devrail_runs WHERE task_id=$1 AND organization_id=$2 AND status IN ('starting','active','awaiting_approval')",
    )
    .bind(input.task_id)
    .bind(input.actor.organization_id)
    .fetch_one(&mut *connection)
    .await?
        > 0
    {
        return Err(sqlx::Error::Protocol("任务存在活动运行".to_string()));
    }
    let (previous_count, previous_cost) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT count(*), COALESCE(sum(cost_units),0) FROM devrail_repair_requests WHERE organization_id=$1 AND task_id=$2",
    )
    .bind(input.actor.organization_id)
    .bind(input.task_id)
    .fetch_one(&mut *connection)
        .await?;
    if previous_count >= i64::from(input.max_repairs) {
        return Err(sqlx::Error::Protocol(
            "repair 次数达到固化策略上限".to_string(),
        ));
    }
    let max_cost_units = input
        .policy_snapshot
        .get("max_cost_units")
        .and_then(Value::as_i64)
        .unwrap_or(i64::from(input.max_repairs));
    if max_cost_units <= 0
        || previous_cost.saturating_add(i64::from(input.cost_units)) > max_cost_units
    {
        return Err(sqlx::Error::Protocol(
            "repair 成本达到固化策略上限".to_string(),
        ));
    }
    let diagnosis = create_diagnosis(
        connection,
        &DiagnosisContext {
            actor: input.actor,
            project_id,
            task_id: input.task_id,
            source_run_id: input.source_run_id,
            department_id,
            owner_user_id,
            input: &input.diagnosis,
        },
    )
    .await?;
    let sequence = i16::try_from(previous_count + 1)
        .map_err(|_| sqlx::Error::Protocol("repair 序号超出范围".to_string()))?;
    let request_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO devrail_repair_requests (organization_id,department_id,owner_user_id,project_id,task_id,source_run_id,root_run_id,diagnosis_id,failure_evidence_ref,failure_evidence_digest,changeset_digest,idempotency_key,repair_sequence,risk_category,strategy_version,policy_snapshot,source_task_status,cost_units) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18) RETURNING id",
    )
    .bind(input.actor.organization_id)
    .bind(department_id)
    .bind(owner_user_id)
    .bind(project_id)
    .bind(input.task_id)
    .bind(input.source_run_id)
    .bind(root_run_id)
    .bind(diagnosis.id)
    .bind(input.diagnosis.evidence_ref)
    .bind(input.diagnosis.evidence_digest)
    .bind(input.diagnosis.changeset_digest)
    .bind(input.idempotency_key)
    .bind(sequence)
    .bind(input.risk_category)
    .bind(input.strategy_version)
    .bind(input.policy_snapshot)
    .bind("failed")
    .bind(i32::try_from(input.cost_units).map_err(|_| sqlx::Error::Protocol("repair 成本无效".to_string()))?)
    .fetch_one(&mut *connection)
    .await?;
    set_repair_history_context(
        connection,
        &RepairHistoryContext {
            actor: input.actor,
            reason: "repair_requested",
            request_id,
            diagnosis_id: diagnosis.id,
            source_run_id: input.source_run_id,
            child_run_id: None,
            policy_version: input.strategy_version,
            result_code: None,
        },
    )
    .await?;
    let updated = sqlx::query(
        "UPDATE devrail_tasks SET status='repair_pending', current_repair_request_id=$4, revision=revision+1, updated_at=now() WHERE id=$1 AND organization_id=$2 AND revision=$3 AND ((status='failed' AND $5::bigint IS NULL) OR (status='repair_handoff' AND current_repair_request_id=$5)) AND NOT EXISTS (SELECT 1 FROM devrail_runs active WHERE active.task_id=$1 AND active.organization_id=$2 AND active.status IN ('starting','active','awaiting_approval'))",
    )
    .bind(input.task_id)
    .bind(input.actor.organization_id)
    .bind(task_revision)
    .bind(request_id)
    .bind(input.retry_of_request_id)
    .execute(&mut *connection)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "repair 创建期间任务状态发生变化".to_string(),
        ));
    }
    let task = fetch_task(connection, input.task_id, input.actor.organization_id).await?;
    devrail::append_task_event(
        connection,
        &task,
        "repair.created",
        &format!("repair:{request_id}:created"),
        &serde_json::json!({
            "requestId": request_id,
            "sourceRunId": input.source_run_id,
            "diagnosisId": diagnosis.id,
            "riskCategory": input.risk_category,
            "sequence": sequence,
        }),
        "受控修复请求已创建",
    )
    .await?;
    audit_logs::record_actor(
        connection,
        input.actor,
        "devrail.repair.create",
        "devrail_repair_request",
        Some(request_id),
        serde_json::json!({
            "taskId": input.task_id,
            "sourceRunId": input.source_run_id,
            "diagnosisId": diagnosis.id,
            "riskCategory": input.risk_category,
            "strategyVersion": input.strategy_version,
        }),
    )
    .await?;
    let request = find_by_id_in_connection(connection, input.actor, request_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    lifecycle_notification(
        connection,
        &request,
        "devrail.repair.created",
        "info",
        "受控修复请求已创建",
        "失败诊断已保存，等待策略与审批检查。",
        &format!("/devrail/repairs/{request_id}"),
    )
    .await?;
    Ok((request, true))
}

async fn fetch_task(
    connection: &mut PgConnection,
    task_id: i64,
    organization_id: i64,
) -> Result<DevRailTaskRow, sqlx::Error> {
    sqlx::query_as::<_, DevRailTaskRow>(AssertSqlSafe(format!(
        "SELECT {TASK_COLUMNS} FROM devrail_tasks WHERE id=$1 AND organization_id=$2"
    )))
    .bind(task_id)
    .bind(organization_id)
    .fetch_one(&mut *connection)
    .await
}

async fn set_repair_history_context(
    connection: &mut PgConnection,
    context: &RepairHistoryContext<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT set_config('devrail.actor_type',$1,true),
                set_config('devrail.actor_user_id',$2,true),
                set_config('devrail.transition_reason',$3,true),
                set_config('devrail.trace_id',$4,true),
                set_config('devrail.continuation_request_id','',true),
                set_config('devrail.continuation_trigger_type','',true),
                set_config('devrail.continuation_policy_version','',true),
                set_config('devrail.repair_request_id',$5,true),
                set_config('devrail.repair_diagnosis_id',$6,true),
                set_config('devrail.source_run_id',$7,true),
                set_config('devrail.child_run_id',$8,true),
                set_config('devrail.repair_policy_version',$9,true),
                set_config('devrail.repair_result_code',$10,true)",
    )
    .bind(context.actor.actor_type.as_str())
    .bind(context.actor.user_id.to_string())
    .bind(context.reason)
    .bind(Uuid::new_v4().to_string())
    .bind(context.request_id.to_string())
    .bind(context.diagnosis_id.to_string())
    .bind(context.source_run_id.to_string())
    .bind(
        context
            .child_run_id
            .map(|value| value.to_string())
            .unwrap_or_default(),
    )
    .bind(context.policy_version)
    .bind(context.result_code.unwrap_or_default())
    .execute(&mut *connection)
    .await
    .map(|_| ())
}

pub async fn find_by_id_in_connection(
    connection: &mut PgConnection,
    actor: &ActorContext,
    id: i64,
) -> Result<Option<DevRailRepairRequestRow>, sqlx::Error> {
    let sql = format!(
        "{} SELECT {REQUEST_COLUMNS} FROM devrail_repair_requests r WHERE r.id=$5 AND {}",
        visible_departments_cte(),
        scoped("r")
    );
    sqlx::query_as::<_, DevRailRepairRequestRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(id)
        .fetch_optional(&mut *connection)
        .await
}

pub async fn find_by_id(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<Option<DevRailRepairRequestRow>, sqlx::Error> {
    let sql = format!(
        "{} SELECT {REQUEST_COLUMNS} FROM devrail_repair_requests r WHERE r.id=$5 AND {}",
        visible_departments_cte(),
        scoped("r")
    );
    sqlx::query_as::<_, DevRailRepairRequestRow>(AssertSqlSafe(sql))
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
) -> Result<(Vec<DevRailRepairRequestRow>, i64), sqlx::Error> {
    let sql = format!(
        "{} SELECT {REQUEST_COLUMNS} FROM devrail_repair_requests r WHERE ($5::bigint IS NULL OR r.task_id=$5) AND ($6::bigint IS NULL OR r.source_run_id=$6) AND {} ORDER BY r.created_at DESC,r.id DESC LIMIT $7 OFFSET $8",
        visible_departments_cte(),
        scoped("r")
    );
    let items = sqlx::query_as::<_, DevRailRepairRequestRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(task_id)
        .bind(source_run_id)
        .bind(page_size.clamp(1, 100))
        .bind((page.max(1) - 1) * page_size.clamp(1, 100))
        .fetch_all(pool)
        .await?;
    let count_sql = format!(
        "{} SELECT count(*) FROM devrail_repair_requests r WHERE ($5::bigint IS NULL OR r.task_id=$5) AND ($6::bigint IS NULL OR r.source_run_id=$6) AND {}",
        visible_departments_cte(),
        scoped("r")
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

pub async fn find_diagnosis(
    pool: &PgPool,
    actor: &ActorContext,
    diagnosis_id: i64,
) -> Result<Option<DevRailRepairDiagnosisRow>, sqlx::Error> {
    let sql = format!(
        "{} SELECT {DIAGNOSIS_COLUMNS} FROM devrail_repair_diagnoses d WHERE d.id=$5 AND {}",
        visible_departments_cte(),
        scoped("d")
    );
    sqlx::query_as::<_, DevRailRepairDiagnosisRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(diagnosis_id)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn failed_quality_gate_evidence(
    pool: &PgPool,
    actor: &ActorContext,
    source_run_id: i64,
) -> Result<Vec<FailedQualityGateEvidence>, sqlx::Error> {
    let sql = format!(
        "{} SELECT e.id AS event_id, e.payload->>'name' AS gate_id, e.payload->>'log_ref' AS log_ref, h.changeset_digest, e.occurred_at FROM devrail_runs r JOIN devrail_tasks t ON t.id=r.task_id AND t.organization_id=r.organization_id JOIN devrail_run_events e ON e.run_id=r.id AND e.organization_id=r.organization_id JOIN devrail_run_handoffs h ON h.source_run_id=r.id AND h.organization_id=r.organization_id AND h.evidence_status='available' AND h.validated_at IS NOT NULL WHERE r.id=$5 AND r.status='failed' AND e.event_type='quality_gate' AND COALESCE(e.payload->>'status','') NOT IN ('passed','success','succeeded') AND {} AND {} ORDER BY e.id",
        visible_departments_cte(),
        scoped("r"),
        scoped("t")
    );
    sqlx::query_as::<
        _,
        (
            i64,
            Option<String>,
            Option<String>,
            Option<String>,
            DateTime<Utc>,
        ),
    >(AssertSqlSafe(sql))
    .bind(actor.data_scope.as_str())
    .bind(actor.organization_id)
    .bind(actor.user_id)
    .bind(actor.department_id)
    .bind(source_run_id)
    .fetch_all(pool)
    .await
    .map(|rows| {
        rows.into_iter()
            .filter_map(
                |(event_id, gate_id, log_ref, changeset_digest, observed_at)| {
                    gate_id
                        .filter(|value| valid_identifier(value, 64))
                        .map(|gate_id| FailedQualityGateEvidence {
                            event_id,
                            gate_id,
                            log_ref: log_ref.filter(|value| safe_reference(value, 256)),
                            changeset_digest,
                            observed_at,
                        })
                },
            )
            .collect()
    })
}

pub async fn dispatch_evidence_is_current(
    pool: &PgPool,
    actor: &ActorContext,
    request_id: i64,
) -> Result<bool, sqlx::Error> {
    let sql = format!(
        "{} SELECT EXISTS(
             SELECT 1
             FROM devrail_repair_requests r
             JOIN devrail_repair_diagnoses d
               ON d.id=r.diagnosis_id AND d.organization_id=r.organization_id
             JOIN devrail_run_handoffs h
               ON h.source_run_id=r.source_run_id AND h.organization_id=r.organization_id
              AND h.evidence_status='available' AND h.validated_at IS NOT NULL
             WHERE r.id=$5 AND r.changeset_digest=d.changeset_digest
               AND r.changeset_digest=h.changeset_digest
               AND d.evidence_expires_at>now()
               AND {} AND {}
         )",
        visible_departments_cte(),
        scoped("r"),
        scoped("d")
    );
    sqlx::query_scalar(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(request_id)
        .fetch_one(pool)
        .await
}

pub async fn approval_is_current(
    pool: &PgPool,
    actor: &ActorContext,
    request_id: i64,
    risk_category: &str,
) -> Result<bool, sqlx::Error> {
    if risk_category == "low_risk" {
        return Ok(true);
    }
    let sql = format!(
        "{} SELECT EXISTS(
             SELECT 1
             FROM devrail_repair_approvals a
             WHERE a.repair_request_id=$5 AND a.risk_category=$6
               AND a.status='approved' AND a.expires_at>now() AND {}
         )",
        visible_departments_cte(),
        scoped("a")
    );
    sqlx::query_scalar(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(request_id)
        .bind(risk_category)
        .fetch_one(pool)
        .await
}

pub async fn claim_pending(
    pool: &PgPool,
    worker_id: &str,
    claim_token: Uuid,
    limit: i64,
    lease_seconds: i64,
) -> Result<Vec<DevRailRepairRequestRow>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let rows = sqlx::query_as::<_, DevRailRepairRequestRow>(AssertSqlSafe(format!(
        "WITH candidates AS (
             SELECT id FROM devrail_repair_requests
             WHERE status='pending' AND (next_attempt_at IS NULL OR next_attempt_at<=now())
             ORDER BY created_at,id FOR UPDATE SKIP LOCKED LIMIT $1
         )
         UPDATE devrail_repair_requests r
         SET status='claimed', status_version=status_version+1, claim_owner=$2,
             claim_token=$3, claim_expires_at=now()+make_interval(secs=>$4),
             claimed_at=COALESCE(claimed_at,now()), dispatch_attempts=dispatch_attempts+1,
             updated_at=now()
         FROM candidates WHERE r.id=candidates.id RETURNING {REQUEST_COLUMNS}"
    )))
    .bind(limit.clamp(1, 100))
    .bind(worker_id)
    .bind(claim_token)
    .bind(lease_seconds.clamp(10, 3_600))
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(rows)
}

pub async fn renew_claim(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
    claim_token: Uuid,
    lease_seconds: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_repair_requests SET claim_expires_at=now()+make_interval(secs=>$4), updated_at=now() WHERE id=$1 AND status='claimed' AND claim_owner=$2 AND claim_token=$3 AND claim_expires_at>now()",
    )
    .bind(id)
    .bind(worker_id)
    .bind(claim_token)
    .bind(lease_seconds.clamp(10, 3_600))
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn release_claim(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
    claim_token: Uuid,
    backoff_seconds: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_repair_requests SET status='pending', status_version=status_version+1, claim_owner=NULL, claim_token=NULL, claim_expires_at=NULL, next_attempt_at=now()+make_interval(secs=>$4), updated_at=now() WHERE id=$1 AND status='claimed' AND claim_owner=$2 AND claim_token=$3",
    )
    .bind(id)
    .bind(worker_id)
    .bind(claim_token)
    .bind(backoff_seconds.clamp(0, 3_600))
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn release_expired_claims(pool: &PgPool, limit: i64) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_repair_requests SET status='pending', status_version=status_version+1, claim_owner=NULL, claim_token=NULL, claim_expires_at=NULL, next_attempt_at=now(), updated_at=now() WHERE id IN (SELECT id FROM devrail_repair_requests WHERE status='claimed' AND claim_expires_at<=now() ORDER BY claim_expires_at,id FOR UPDATE SKIP LOCKED LIMIT $1)",
    )
    .bind(limit.clamp(1, 500))
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn list_dispatched_unstarted(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<DevRailRepairRequestRow>, sqlx::Error> {
    sqlx::query_as::<_, DevRailRepairRequestRow>(AssertSqlSafe(format!(
        "SELECT {REQUEST_COLUMNS}
         FROM devrail_repair_requests r
         JOIN devrail_runs child
           ON child.id=r.child_run_id AND child.organization_id=r.organization_id
         WHERE r.status='dispatched' AND child.status='starting'
           AND child.started_at IS NULL
           AND (child.harness_start_claim_token IS NULL
                OR child.harness_start_claim_expires_at<=now())
         ORDER BY r.dispatched_at NULLS FIRST,r.id LIMIT $1"
    )))
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await
}

pub async fn mark_dispatched(
    connection: &mut PgConnection,
    actor: &ActorContext,
    request_id: i64,
    worker_id: &str,
    claim_token: Uuid,
    child_run_id: i64,
) -> Result<Option<DevRailRepairRequestRow>, sqlx::Error> {
    let request = sqlx::query_as::<_, DevRailRepairRequestRow>(AssertSqlSafe(format!(
        "UPDATE devrail_repair_requests r
         SET status='dispatched', status_version=status_version+1,
             child_run_id=$4, claim_owner=NULL, claim_token=NULL,
             claim_expires_at=NULL, dispatched_at=COALESCE(dispatched_at,now()), updated_at=now()
         WHERE r.id=$1 AND r.organization_id=$5 AND r.status='claimed'
           AND r.claim_owner=$2 AND r.claim_token=$3 AND r.claim_expires_at>now()
         RETURNING {REQUEST_COLUMNS}"
    )))
    .bind(request_id)
    .bind(worker_id)
    .bind(claim_token)
    .bind(child_run_id)
    .bind(actor.organization_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(request) = request else {
        return Ok(None);
    };
    set_repair_history_context(
        connection,
        &RepairHistoryContext {
            actor,
            reason: "repair_dispatched",
            request_id: request.id,
            diagnosis_id: request.diagnosis_id,
            source_run_id: request.source_run_id,
            child_run_id: Some(child_run_id),
            policy_version: &request.strategy_version,
            result_code: None,
        },
    )
    .await?;
    let updated = sqlx::query(
        "UPDATE devrail_tasks SET status='repair_running', revision=revision+1, updated_at=now()
         WHERE id=$1 AND organization_id=$2 AND status='repair_pending'
           AND current_repair_request_id=$3
           AND EXISTS (SELECT 1 FROM devrail_runs child
                       WHERE child.id=$4 AND child.organization_id=$2
                         AND child.task_id=devrail_tasks.id
                         AND child.repair_request_id=$3 AND child.status='starting')",
    )
    .bind(request.task_id)
    .bind(request.organization_id)
    .bind(request.id)
    .bind(child_run_id)
    .execute(&mut *connection)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "repair 派发期间任务状态发生变化".to_string(),
        ));
    }
    let task = fetch_task(connection, request.task_id, request.organization_id).await?;
    devrail::append_task_event(
        connection,
        &task,
        "repair.dispatched",
        &format!("repair:{}:dispatched", request.id),
        &serde_json::json!({"requestId": request.id, "sourceRunId": request.source_run_id, "childRunId": child_run_id}),
        "受控修复请求已派发",
    )
    .await?;
    audit_logs::record_actor(
        connection,
        actor,
        "devrail.repair.dispatch",
        "devrail_repair_request",
        Some(request.id),
        serde_json::json!({"taskId": request.task_id, "childRunId": child_run_id}),
    )
    .await?;
    lifecycle_notification(
        connection,
        &request,
        "devrail.repair.dispatched",
        "info",
        "受控修复已派发",
        "修复运行已派发，正在受控 workspace 中执行。",
        &format!("/devrail/runs/{child_run_id}"),
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
) -> Result<Option<DevRailRepairRequestRow>, sqlx::Error> {
    if !matches!(task_status, "succeeded" | "failed" | "cancelled")
        || !valid_identifier(result_code, 64)
    {
        return Err(sqlx::Error::Protocol(
            "repair child 任务终态无效".to_string(),
        ));
    }
    let request = sqlx::query_as::<_, DevRailRepairRequestRow>(AssertSqlSafe(format!(
        "UPDATE devrail_repair_requests r
         SET status=CASE WHEN $2='succeeded' THEN 'succeeded' WHEN $2='cancelled' THEN 'cancelled' ELSE 'failed' END,
             status_version=status_version+1, result_code=$3,
             completed_at=COALESCE(r.completed_at,now()), updated_at=now()
         FROM devrail_runs child
         WHERE r.child_run_id=child.id AND child.id=$1
           AND r.organization_id=$4 AND child.organization_id=$4
           AND r.status IN ('dispatched','running')
           AND child.status IN ('completed','failed','cancelled')
         RETURNING {REQUEST_COLUMNS}"
    )))
    .bind(child_run_id)
    .bind(task_status)
    .bind(result_code)
    .bind(actor.organization_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(request) = request else {
        return Ok(None);
    };
    let projected_status = if task_status == "succeeded" {
        "succeeded"
    } else {
        "repair_handoff"
    };
    set_repair_history_context(
        connection,
        &RepairHistoryContext {
            actor,
            reason: "repair_completed",
            request_id: request.id,
            diagnosis_id: request.diagnosis_id,
            source_run_id: request.source_run_id,
            child_run_id: Some(child_run_id),
            policy_version: &request.strategy_version,
            result_code: Some(result_code),
        },
    )
    .await?;
    let projected = sqlx::query(
        "UPDATE devrail_tasks SET status=$3, revision=revision+1,
             scheduler_claim_token=NULL, scheduler_claimed_at=NULL,
             scheduler_retry_at=NULL, scheduler_last_error=NULL, updated_at=now()
         WHERE id=$1 AND organization_id=$2 AND status='repair_running'
           AND current_repair_request_id=$4",
    )
    .bind(request.task_id)
    .bind(request.organization_id)
    .bind(projected_status)
    .bind(request.id)
    .execute(&mut *connection)
    .await?;
    if projected.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "repair child 终态投影期间任务状态发生变化".to_string(),
        ));
    }
    if task_status != "succeeded" {
        sqlx::query(
            "INSERT INTO devrail_repair_handoffs
             (organization_id,department_id,owner_user_id,project_id,task_id,repair_request_id,reason_code,recommendation)
             VALUES ($1,$2,$3,$4,$5,$6,'gate_failed','修复运行未通过验证，请由授权人员检查诊断和门禁结果。')
             ON CONFLICT (organization_id,repair_request_id,reason_code) DO NOTHING",
        )
        .bind(request.organization_id)
        .bind(request.department_id)
        .bind(request.owner_user_id)
        .bind(request.project_id)
        .bind(request.task_id)
        .bind(request.id)
        .execute(&mut *connection)
        .await?;
    }
    let task = fetch_task(connection, request.task_id, request.organization_id).await?;
    devrail::append_task_event(
        connection,
        &task,
        "repair.completed",
        &format!("repair:{}:completed", request.id),
        &serde_json::json!({"requestId": request.id, "sourceRunId": request.source_run_id, "childRunId": child_run_id, "resultCode": result_code}),
        "受控修复运行已结束",
    )
    .await?;
    audit_logs::record_actor(
        connection,
        actor,
        "devrail.repair.complete",
        "devrail_repair_request",
        Some(request.id),
        serde_json::json!({"taskId": request.task_id, "childRunId": child_run_id, "resultCode": result_code}),
    )
    .await?;
    lifecycle_notification(
        connection,
        &request,
        "devrail.repair.completed",
        if task_status == "succeeded" {
            "success"
        } else {
            "warning"
        },
        if task_status == "succeeded" {
            "受控修复已成功"
        } else {
            "受控修复需要人工处理"
        },
        if task_status == "succeeded" {
            "修复运行已完成，正在重新执行受影响门禁。"
        } else {
            "修复运行未完成自动验证，需要人工处理。"
        },
        &format!("/devrail/repairs/{}", request.id),
    )
    .await?;
    Ok(Some(request))
}

pub async fn begin_gate_reruns_for_child_run(
    connection: &mut PgConnection,
    actor: &ActorContext,
    child_run_id: i64,
) -> Result<bool, sqlx::Error> {
    let context = sqlx::query(
        "SELECT r.id,r.status,d.affected_gates,h.changeset_digest
         FROM devrail_repair_requests r
         JOIN devrail_runs child
           ON child.id=r.child_run_id AND child.organization_id=r.organization_id
         JOIN devrail_repair_diagnoses d
           ON d.id=r.diagnosis_id AND d.organization_id=r.organization_id
         JOIN devrail_run_handoffs h
           ON h.source_run_id=child.id AND h.organization_id=child.organization_id
          AND h.evidence_status='available' AND h.validated_at IS NOT NULL
         WHERE child.id=$1 AND r.organization_id=$2 AND child.status='completed'
           AND r.status IN ('dispatched','running')
         FOR UPDATE OF r,d,h",
    )
    .bind(child_run_id)
    .bind(actor.organization_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(context) = context else {
        return Ok(false);
    };
    let request_id: i64 = context.try_get("id")?;
    let affected_gates: Value = context.try_get("affected_gates")?;
    let changeset_digest: String = context.try_get("changeset_digest")?;
    if !valid_digest(&changeset_digest) {
        return Err(sqlx::Error::Protocol(
            "repair child changeset 摘要无效".to_string(),
        ));
    }
    let Some(gates) = affected_gates.as_array() else {
        return Err(sqlx::Error::Protocol(
            "repair 诊断缺少门禁重跑范围".to_string(),
        ));
    };
    if gates.is_empty() {
        return Err(sqlx::Error::Protocol(
            "repair 诊断缺少门禁重跑范围".to_string(),
        ));
    }
    sqlx::query(
        "UPDATE devrail_repair_requests
         SET status='running', status_version=status_version+1, updated_at=now()
         WHERE id=$1 AND organization_id=$2 AND status IN ('dispatched','running')",
    )
    .bind(request_id)
    .bind(actor.organization_id)
    .execute(&mut *connection)
    .await?;
    for gate in gates {
        let gate_id = gate
            .as_str()
            .filter(|value| valid_identifier(value, 64))
            .ok_or_else(|| sqlx::Error::Protocol("repair 诊断门禁标识无效".to_string()))?;
        let _ = create_gate_rerun(
            connection,
            actor,
            &NewRepairGateRerun {
                request_id,
                gate_id,
                changeset_digest: &changeset_digest,
                idempotency_key: &format!("repair:{request_id}:gate:{gate_id}:{changeset_digest}"),
                child_run_id: Some(child_run_id),
            },
        )
        .await?;
    }
    Ok(true)
}

pub async fn pending_depth(pool: &PgPool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM devrail_repair_requests WHERE status IN ('pending','claimed')",
    )
    .fetch_one(pool)
    .await
}

pub async fn create_gate_rerun(
    connection: &mut PgConnection,
    actor: &ActorContext,
    input: &NewRepairGateRerun<'_>,
) -> Result<(DevRailRepairGateRerunRow, bool), sqlx::Error> {
    if !valid_identifier(input.gate_id, 64)
        || !valid_digest(input.changeset_digest)
        || !valid_identifier(input.idempotency_key, 256)
    {
        return Err(sqlx::Error::Protocol("repair 门禁重跑字段无效".to_string()));
    }
    let source = sqlx::query(
        "SELECT r.project_id,r.task_id,r.department_id,r.owner_user_id,r.changeset_digest,r.status,d.affected_gates
         FROM devrail_repair_requests r
         JOIN devrail_repair_diagnoses d ON d.id=r.diagnosis_id AND d.organization_id=r.organization_id
         WHERE r.id=$1 AND r.organization_id=$2 FOR UPDATE OF r,d",
    )
    .bind(input.request_id)
    .bind(actor.organization_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(sqlx::Error::RowNotFound)?;
    let project_id: i64 = source.try_get("project_id")?;
    let task_id: i64 = source.try_get("task_id")?;
    let department_id: Option<i64> = source.try_get("department_id")?;
    let owner_user_id: i64 = source.try_get("owner_user_id")?;
    let request_changeset: Option<String> = source.try_get("changeset_digest")?;
    let request_status: String = source.try_get("status")?;
    let affected_gates: Value = source.try_get("affected_gates")?;
    if matches!(
        request_status.as_str(),
        "succeeded" | "failed" | "cancelled" | "handed_off" | "rejected"
    ) {
        return Err(sqlx::Error::Protocol(
            "repair 请求已结束，不能新增门禁重跑".to_string(),
        ));
    }
    if !affected_gates.as_array().is_some_and(|values| {
        values
            .iter()
            .any(|value| value.as_str() == Some(input.gate_id))
    }) {
        return Err(sqlx::Error::Protocol(
            "门禁不属于 repair 诊断范围".to_string(),
        ));
    }
    if let Some(child_run_id) = input.child_run_id {
        let child_changeset = sqlx::query_scalar::<_, String>(
            "SELECT h.changeset_digest
             FROM devrail_runs child
             JOIN devrail_run_handoffs h
               ON h.source_run_id=child.id AND h.organization_id=child.organization_id
              AND h.evidence_status='available' AND h.validated_at IS NOT NULL
             WHERE child.id=$1 AND child.organization_id=$2
               AND child.repair_request_id=$3 AND child.status='completed'",
        )
        .bind(child_run_id)
        .bind(actor.organization_id)
        .bind(input.request_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or_else(|| sqlx::Error::Protocol("repair 子运行 changeset 证据不可用".to_string()))?;
        if child_changeset != input.changeset_digest {
            return Err(sqlx::Error::Protocol(
                "repair 门禁重跑 changeset 与子运行不匹配".to_string(),
            ));
        }
    } else if request_changeset
        .as_deref()
        .is_some_and(|value| value != input.changeset_digest)
    {
        return Err(sqlx::Error::Protocol(
            "repair 门禁重跑 changeset 与请求不匹配".to_string(),
        ));
    }
    let existing_key = sqlx::query_as::<_, DevRailRepairGateRerunRow>(AssertSqlSafe(format!(
        "SELECT {GATE_RERUN_COLUMNS} FROM devrail_repair_gate_reruns g WHERE g.organization_id=$1 AND g.idempotency_key=$2 FOR UPDATE"
    )))
    .bind(actor.organization_id)
    .bind(input.idempotency_key)
    .fetch_optional(&mut *connection)
    .await?;
    if let Some(existing) = existing_key {
        if existing.repair_request_id != input.request_id
            || existing.gate_id != input.gate_id
            || existing.changeset_digest != input.changeset_digest
            || existing.child_run_id != input.child_run_id
        {
            return Err(sqlx::Error::Protocol(
                "repair 门禁重跑幂等身份不匹配".to_string(),
            ));
        }
        return Ok((existing, false));
    }
    let sql = format!(
        "INSERT INTO devrail_repair_gate_reruns (organization_id,department_id,owner_user_id,project_id,task_id,repair_request_id,child_run_id,gate_id,changeset_digest,idempotency_key) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT (organization_id,repair_request_id,gate_id,changeset_digest) DO NOTHING RETURNING {GATE_RERUN_INSERT_COLUMNS}"
    );
    if let Some(created) = sqlx::query_as::<_, DevRailRepairGateRerunRow>(AssertSqlSafe(sql))
        .bind(actor.organization_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(project_id)
        .bind(task_id)
        .bind(input.request_id)
        .bind(input.child_run_id)
        .bind(input.gate_id)
        .bind(input.changeset_digest)
        .bind(input.idempotency_key)
        .fetch_optional(&mut *connection)
        .await?
    {
        return Ok((created, true));
    }
    let existing = sqlx::query_as::<_, DevRailRepairGateRerunRow>(AssertSqlSafe(format!(
        "SELECT {GATE_RERUN_COLUMNS} FROM devrail_repair_gate_reruns g WHERE g.organization_id=$1 AND g.repair_request_id=$2 AND g.gate_id=$3 AND g.changeset_digest=$4 FOR UPDATE"
    )))
    .bind(actor.organization_id)
    .bind(input.request_id)
    .bind(input.gate_id)
    .bind(input.changeset_digest)
    .fetch_one(&mut *connection)
    .await?;
    if existing.idempotency_key != input.idempotency_key
        || existing.child_run_id != input.child_run_id
    {
        return Err(sqlx::Error::Protocol(
            "repair 门禁重跑幂等身份不匹配".to_string(),
        ));
    }
    Ok((existing, false))
}

pub async fn list_gate_reruns(
    pool: &PgPool,
    actor: &ActorContext,
    request_id: i64,
) -> Result<Vec<DevRailRepairGateRerunRow>, sqlx::Error> {
    let sql = format!(
        "{} SELECT {GATE_RERUN_COLUMNS} FROM devrail_repair_gate_reruns g WHERE g.repair_request_id=$5 AND {} ORDER BY g.created_at,g.id",
        visible_departments_cte(),
        scoped("g")
    );
    sqlx::query_as::<_, DevRailRepairGateRerunRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(request_id)
        .fetch_all(pool)
        .await
}

pub async fn claim_gate_reruns(
    pool: &PgPool,
    worker_id: &str,
    claim_token: Uuid,
    limit: i64,
    lease_seconds: i64,
) -> Result<Vec<DevRailRepairGateRerunRow>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let rows = sqlx::query_as::<_, DevRailRepairGateRerunRow>(AssertSqlSafe(format!(
        "WITH candidates AS (
             SELECT id FROM devrail_repair_gate_reruns
             WHERE status='pending'
             ORDER BY created_at,id FOR UPDATE SKIP LOCKED LIMIT $1
         )
         UPDATE devrail_repair_gate_reruns g
         SET status='running', claim_owner=$2, claim_token=$3,
             claim_expires_at=now()+make_interval(secs=>$4),
             started_at=COALESCE(started_at,now()), updated_at=now()
         FROM candidates WHERE g.id=candidates.id
         RETURNING {GATE_RERUN_COLUMNS}"
    )))
    .bind(limit.clamp(1, 100))
    .bind(worker_id)
    .bind(claim_token)
    .bind(lease_seconds.clamp(10, 3_600))
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(rows)
}

pub async fn release_expired_gate_rerun_claims(
    pool: &PgPool,
    limit: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_repair_gate_reruns
         SET status='pending', claim_owner=NULL, claim_token=NULL, claim_expires_at=NULL,
             updated_at=now()
         WHERE id IN (
             SELECT id FROM devrail_repair_gate_reruns
             WHERE status='running' AND claim_expires_at<=now()
             ORDER BY claim_expires_at,id FOR UPDATE SKIP LOCKED LIMIT $1
         )",
    )
    .bind(limit.clamp(1, 500))
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn release_gate_rerun_claim(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
    claim_token: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_repair_gate_reruns
         SET status='pending', claim_owner=NULL, claim_token=NULL, claim_expires_at=NULL,
             updated_at=now()
         WHERE id=$1 AND status='running' AND claim_owner=$2 AND claim_token=$3",
    )
    .bind(id)
    .bind(worker_id)
    .bind(claim_token)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn renew_gate_rerun_claim(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
    claim_token: Uuid,
    lease_seconds: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE devrail_repair_gate_reruns
         SET claim_expires_at=now()+make_interval(secs=>$4), updated_at=now()
         WHERE id=$1 AND status='running' AND claim_owner=$2
           AND claim_token=$3 AND claim_expires_at>now()",
    )
    .bind(id)
    .bind(worker_id)
    .bind(claim_token)
    .bind(lease_seconds.clamp(10, 3_600))
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn complete_gate_rerun(
    connection: &mut PgConnection,
    input: &CompletedRepairGateRerun<'_>,
) -> Result<Option<DevRailRepairGateRerunRow>, sqlx::Error> {
    if !matches!(input.status, "passed" | "failed" | "cancelled")
        || input
            .result_code
            .is_some_and(|value| !valid_identifier(value, 64))
        || input
            .summary
            .is_some_and(|value| value.len() > 512 || sensitive_text(value))
        || input
            .log_ref
            .is_some_and(|value| !safe_reference(value, 256))
        || input
            .duration_ms
            .is_some_and(|value| !(0..=86_400_000).contains(&value))
    {
        return Err(sqlx::Error::Protocol("repair 门禁重跑结果无效".to_string()));
    }
    sqlx::query_as::<_, DevRailRepairGateRerunRow>(AssertSqlSafe(format!(
        "UPDATE devrail_repair_gate_reruns g
         SET status=$4, result_code=$5, summary=$6, log_ref=$7,
             duration_ms=$8, claim_owner=NULL, claim_token=NULL,
             claim_expires_at=NULL, completed_at=COALESCE(g.completed_at,now()),
             updated_at=now()
         WHERE g.id=$1 AND g.status='running' AND g.claim_owner=$2
           AND g.claim_token=$3
         RETURNING {GATE_RERUN_COLUMNS}"
    )))
    .bind(input.id)
    .bind(input.worker_id)
    .bind(input.claim_token)
    .bind(input.status)
    .bind(input.result_code)
    .bind(input.summary)
    .bind(input.log_ref)
    .bind(input.duration_ms)
    .fetch_optional(&mut *connection)
    .await
}

pub async fn finalize_gate_reruns(
    connection: &mut PgConnection,
    actor: &ActorContext,
    request_id: i64,
) -> Result<Option<DevRailRepairRequestRow>, sqlx::Error> {
    let request = sqlx::query_as::<_, DevRailRepairRequestRow>(AssertSqlSafe(format!(
        "SELECT {REQUEST_COLUMNS} FROM devrail_repair_requests r
         WHERE r.id=$1 AND r.organization_id=$2 AND r.status='running' FOR UPDATE"
    )))
    .bind(request_id)
    .bind(actor.organization_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(_) = request else {
        return Ok(None);
    };
    let statuses = sqlx::query_scalar::<_, String>(
        "SELECT status FROM devrail_repair_gate_reruns
         WHERE organization_id=$1 AND repair_request_id=$2 FOR UPDATE",
    )
    .bind(actor.organization_id)
    .bind(request_id)
    .fetch_all(&mut *connection)
    .await?;
    if statuses.is_empty()
        || statuses
            .iter()
            .any(|status| matches!(status.as_str(), "pending" | "running"))
    {
        return Ok(None);
    }
    let succeeded = statuses.iter().all(|status| status == "passed");
    let (status, result_code, task_status) = if succeeded {
        ("succeeded", None, "succeeded")
    } else {
        ("handed_off", Some("gate_failed"), "repair_handoff")
    };
    let completed = sqlx::query_as::<_, DevRailRepairRequestRow>(AssertSqlSafe(format!(
        "UPDATE devrail_repair_requests r
         SET status=$3, status_version=status_version+1, result_code=$4,
             completed_at=COALESCE(completed_at,now()), updated_at=now()
         WHERE r.id=$1 AND r.organization_id=$2 AND r.status='running'
         RETURNING {REQUEST_COLUMNS}"
    )))
    .bind(request_id)
    .bind(actor.organization_id)
    .bind(status)
    .bind(result_code)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(completed) = completed else {
        return Ok(None);
    };
    set_repair_history_context(
        connection,
        &RepairHistoryContext {
            actor,
            reason: "repair_gates_completed",
            request_id: completed.id,
            diagnosis_id: completed.diagnosis_id,
            source_run_id: completed.source_run_id,
            child_run_id: completed.child_run_id,
            policy_version: &completed.strategy_version,
            result_code,
        },
    )
    .await?;
    let projected = sqlx::query(
        "UPDATE devrail_tasks SET status=$3, revision=revision+1,
             scheduler_claim_token=NULL, scheduler_claimed_at=NULL,
             scheduler_retry_at=NULL, scheduler_last_error=NULL, updated_at=now()
         WHERE id=$1 AND organization_id=$2 AND status='repair_running'
           AND current_repair_request_id=$4",
    )
    .bind(completed.task_id)
    .bind(completed.organization_id)
    .bind(task_status)
    .bind(completed.id)
    .execute(&mut *connection)
    .await?;
    if projected.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "repair 门禁终态投影期间任务状态发生变化".to_string(),
        ));
    }
    if !succeeded {
        sqlx::query(
            "INSERT INTO devrail_repair_handoffs
             (organization_id,department_id,owner_user_id,project_id,task_id,repair_request_id,reason_code,recommendation)
             VALUES ($1,$2,$3,$4,$5,$6,'gate_failed','受控修复未通过受影响门禁，请由授权人员检查脱敏诊断和门禁结果。')
             ON CONFLICT (organization_id,repair_request_id,reason_code) DO NOTHING",
        )
        .bind(completed.organization_id)
        .bind(completed.department_id)
        .bind(completed.owner_user_id)
        .bind(completed.project_id)
        .bind(completed.task_id)
        .bind(completed.id)
        .execute(&mut *connection)
        .await?;
    }
    if let Some(child_run_id) = completed.child_run_id {
        devrail_workspaces::mark_cleanup_pending_for_run(
            connection,
            completed.organization_id,
            child_run_id,
            "after_gate_rerun",
        )
        .await?;
    }
    let task = fetch_task(connection, completed.task_id, completed.organization_id).await?;
    devrail::append_task_event(
        connection,
        &task,
        "repair.gates_completed",
        &format!("repair:{}:gates_completed", completed.id),
        &serde_json::json!({"requestId": completed.id, "childRunId": completed.child_run_id, "resultCode": result_code}),
        if succeeded {
            "受控修复已通过受影响门禁"
        } else {
            "受控修复未通过受影响门禁，等待人工处理"
        },
    )
    .await?;
    audit_logs::record_actor(
        connection,
        actor,
        "devrail.repair.gates_complete",
        "devrail_repair_request",
        Some(completed.id),
        serde_json::json!({"taskId": completed.task_id, "childRunId": completed.child_run_id, "resultCode": result_code}),
    )
    .await?;
    lifecycle_notification(
        connection,
        &completed,
        "devrail.repair.gates_completed",
        if succeeded { "success" } else { "warning" },
        if succeeded {
            "受控修复已通过门禁"
        } else {
            "受控修复需人工处理"
        },
        if succeeded {
            "所有受影响门禁已通过。"
        } else {
            "受影响门禁仍未通过，需要人工处理。"
        },
        &format!("/devrail/repairs/{}", completed.id),
    )
    .await?;
    Ok(Some(completed))
}

pub(crate) async fn create_approval(
    connection: &mut PgConnection,
    actor: &ActorContext,
    input: &NewRepairApproval<'_>,
) -> Result<(DevRailRepairApprovalRow, bool), sqlx::Error> {
    if !valid_identifier(input.idempotency_key, 160)
        || !valid_identifier(input.policy_version, 128)
        || !matches!(
            input.risk_category,
            "logical_change" | "dependency_change" | "remote_write" | "security_change"
        )
        || input.expires_at <= Utc::now()
    {
        return Err(sqlx::Error::Protocol("repair 审批字段无效".to_string()));
    }
    let request = sqlx::query(
        "SELECT project_id,task_id,department_id,owner_user_id FROM devrail_repair_requests WHERE id=$1 AND organization_id=$2 FOR UPDATE",
    )
    .bind(input.request_id)
    .bind(actor.organization_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(sqlx::Error::RowNotFound)?;
    let project_id: i64 = request.try_get("project_id")?;
    let task_id: i64 = request.try_get("task_id")?;
    let department_id: Option<i64> = request.try_get("department_id")?;
    let owner_user_id: i64 = request.try_get("owner_user_id")?;
    let sql = format!(
        "INSERT INTO devrail_repair_approvals (organization_id,department_id,owner_user_id,project_id,task_id,repair_request_id,idempotency_key,risk_category,policy_version,requested_by,expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT (organization_id,repair_request_id,idempotency_key) DO NOTHING RETURNING {APPROVAL_INSERT_COLUMNS}"
    );
    if let Some(created) = sqlx::query_as::<_, DevRailRepairApprovalRow>(AssertSqlSafe(sql))
        .bind(actor.organization_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(project_id)
        .bind(task_id)
        .bind(input.request_id)
        .bind(input.idempotency_key)
        .bind(input.risk_category)
        .bind(input.policy_version)
        .bind(input.requested_by)
        .bind(input.expires_at)
        .fetch_optional(&mut *connection)
        .await?
    {
        return Ok((created, true));
    }
    let existing = sqlx::query_as::<_, DevRailRepairApprovalRow>(AssertSqlSafe(format!(
        "SELECT {APPROVAL_COLUMNS} FROM devrail_repair_approvals a WHERE a.organization_id=$1 AND a.repair_request_id=$2 AND a.idempotency_key=$3 FOR UPDATE"
    )))
    .bind(actor.organization_id)
    .bind(input.request_id)
    .bind(input.idempotency_key)
    .fetch_one(&mut *connection)
    .await?;
    if existing.risk_category != input.risk_category
        || existing.policy_version != input.policy_version
    {
        return Err(sqlx::Error::Protocol(
            "repair 审批幂等身份不匹配".to_string(),
        ));
    }
    Ok((existing, false))
}

pub async fn find_approval(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<Option<DevRailRepairApprovalRow>, sqlx::Error> {
    let sql = format!(
        "{} SELECT {APPROVAL_COLUMNS} FROM devrail_repair_approvals a WHERE a.id=$5 AND {}",
        visible_departments_cte(),
        scoped("a")
    );
    sqlx::query_as::<_, DevRailRepairApprovalRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn find_latest_approval(
    pool: &PgPool,
    actor: &ActorContext,
    request_id: i64,
) -> Result<Option<DevRailRepairApprovalRow>, sqlx::Error> {
    let sql = format!(
        "{} SELECT {APPROVAL_COLUMNS} FROM devrail_repair_approvals a WHERE a.repair_request_id=$5 AND {} ORDER BY a.created_at DESC,a.id DESC LIMIT 1",
        visible_departments_cte(),
        scoped("a")
    );
    sqlx::query_as::<_, DevRailRepairApprovalRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(request_id)
        .fetch_optional(pool)
        .await
}

pub async fn decide_approval(
    connection: &mut PgConnection,
    actor: &ActorContext,
    id: i64,
    decision: &str,
    reason: Option<&str>,
) -> Result<Option<DevRailRepairApprovalRow>, sqlx::Error> {
    if !matches!(decision, "approved" | "rejected" | "withdrawn")
        || reason.is_some_and(|value| value.len() > 512 || sensitive_text(value))
    {
        return Err(sqlx::Error::Protocol("repair 审批决定无效".to_string()));
    }
    let sql = format!(
        "{} UPDATE devrail_repair_approvals a SET status=$6, decided_by=$3, decision_reason=$7, updated_at=now() WHERE a.id=$5 AND a.status='pending' AND a.expires_at>now() AND ($6 <> 'withdrawn' OR a.requested_by=$3) AND {} RETURNING {APPROVAL_COLUMNS}",
        visible_departments_cte(),
        scoped("a")
    );
    let row = sqlx::query_as::<_, DevRailRepairApprovalRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(id)
        .bind(decision)
        .bind(reason)
        .fetch_optional(&mut *connection)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let request = find_by_id_in_connection(connection, actor, row.repair_request_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    let (event_type, title, summary) = match decision {
        "approved" => (
            "devrail.repair.approval_approved",
            "受控修复审批已批准",
            "修复审批已批准，等待调度器进行派发。",
        ),
        "rejected" => (
            "devrail.repair.approval_rejected",
            "受控修复审批已拒绝",
            "修复审批已拒绝，已转人工处理。",
        ),
        "withdrawn" => (
            "devrail.repair.approval_withdrawn",
            "受控修复审批已撤回",
            "修复审批已撤回，已转人工处理。",
        ),
        _ => return Err(sqlx::Error::Protocol("repair 审批决定无效".to_string())),
    };
    let task = fetch_task(connection, request.task_id, request.organization_id).await?;
    devrail::append_task_event(
        connection,
        &task,
        "repair.approval_decided",
        &format!("repair:{}:approval:{}:{decision}", request.id, row.id),
        &serde_json::json!({
            "requestId": request.id,
            "sourceRunId": request.source_run_id,
            "diagnosisId": request.diagnosis_id,
            "approvalId": row.id,
            "riskCategory": row.risk_category,
            "policyVersion": row.policy_version,
            "decision": decision,
        }),
        title,
    )
    .await?;
    audit_logs::record_actor(
        connection,
        actor,
        &format!("devrail.repair.approval.{decision}"),
        "devrail_repair_approval",
        Some(row.id),
        serde_json::json!({
            "repairRequestId": request.id,
            "taskId": request.task_id,
            "riskCategory": row.risk_category,
            "policyVersion": row.policy_version,
            "decision": decision,
        }),
    )
    .await?;
    lifecycle_notification(
        connection,
        &request,
        event_type,
        if decision == "approved" {
            "success"
        } else {
            "warning"
        },
        title,
        summary,
        &format!("/devrail/repairs/{}", request.id),
    )
    .await?;
    Ok(Some(row))
}

pub async fn approval_satisfied(
    connection: &mut PgConnection,
    request_id: i64,
    risk_category: &str,
) -> Result<bool, sqlx::Error> {
    if risk_category == "low_risk" {
        return Ok(true);
    }
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM devrail_repair_approvals WHERE repair_request_id=$1 AND risk_category=$2 AND status='approved' AND expires_at>now())",
    )
    .bind(request_id)
    .bind(risk_category)
    .fetch_one(&mut *connection)
    .await
}

pub(crate) async fn handoff(
    connection: &mut PgConnection,
    actor: &ActorContext,
    request_id: i64,
    input: &NewRepairHandoff<'_>,
) -> Result<Option<DevRailRepairRequestRow>, sqlx::Error> {
    if !valid_identifier(input.reason_code, 64)
        || input.recommendation.is_empty()
        || input.recommendation.len() > 512
        || sensitive_text(input.recommendation)
    {
        return Err(sqlx::Error::Protocol("repair 人工交接字段无效".to_string()));
    }
    let sql = format!(
        "{} UPDATE devrail_repair_requests r SET status='handed_off', status_version=status_version+1, handoff_reason=$6, result_code=$6, claim_owner=NULL, claim_token=NULL, claim_expires_at=NULL, completed_at=COALESCE(completed_at,now()), updated_at=now() WHERE r.id=$5 AND r.status IN ('pending','claimed','dispatched','running') AND {} RETURNING {REQUEST_COLUMNS}",
        visible_departments_cte(),
        scoped("r")
    );
    let request = sqlx::query_as::<_, DevRailRepairRequestRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(request_id)
        .bind(input.reason_code)
        .fetch_optional(&mut *connection)
        .await?;
    let Some(request) = request else {
        return Ok(None);
    };
    crate::app_metrics::record_repair_handoff(input.reason_code);
    sqlx::query(
        "INSERT INTO devrail_repair_handoffs (organization_id,department_id,owner_user_id,project_id,task_id,repair_request_id,reason_code,recommendation) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (organization_id,repair_request_id,reason_code) DO NOTHING",
    )
    .bind(request.organization_id)
    .bind(request.department_id)
    .bind(request.owner_user_id)
    .bind(request.project_id)
    .bind(request.task_id)
    .bind(request.id)
    .bind(input.reason_code)
    .bind(input.recommendation)
    .execute(&mut *connection)
    .await?;
    set_repair_history_context(
        connection,
        &RepairHistoryContext {
            actor,
            reason: "repair_handed_off",
            request_id: request.id,
            diagnosis_id: request.diagnosis_id,
            source_run_id: request.source_run_id,
            child_run_id: request.child_run_id,
            policy_version: &request.strategy_version,
            result_code: Some(input.reason_code),
        },
    )
    .await?;
    let updated = sqlx::query(
        "UPDATE devrail_tasks SET status='repair_handoff', revision=revision+1, current_repair_request_id=$3, updated_at=now() WHERE id=$1 AND organization_id=$2 AND status IN ('repair_pending','repair_running')",
    )
    .bind(request.task_id)
    .bind(request.organization_id)
    .bind(request.id)
    .execute(&mut *connection)
    .await?;
    if updated.rows_affected() > 1 {
        return Err(sqlx::Error::Protocol("repair 任务投影不唯一".to_string()));
    }
    let task = fetch_task(connection, request.task_id, request.organization_id).await?;
    devrail::append_task_event(
        connection,
        &task,
        "repair.handed_off",
        &format!("repair:{}:handoff:{}", request.id, input.reason_code),
        &serde_json::json!({
            "requestId": request.id,
            "sourceRunId": request.source_run_id,
            "reasonCode": input.reason_code,
        }),
        "受控修复已转人工处理",
    )
    .await?;
    audit_logs::record_actor(
        connection,
        actor,
        "devrail.repair.handoff",
        "devrail_repair_request",
        Some(request.id),
        serde_json::json!({"taskId": request.task_id, "reasonCode": input.reason_code}),
    )
    .await?;
    lifecycle_notification(
        connection,
        &request,
        "devrail.repair.handed_off",
        "warning",
        "受控修复需要人工处理",
        "自动修复已停止，请由授权人员评估失败诊断。",
        &format!("/devrail/repairs/{}", request.id),
    )
    .await?;
    Ok(Some(request))
}

pub async fn list_handoffs(
    pool: &PgPool,
    actor: &ActorContext,
    request_id: i64,
) -> Result<Vec<DevRailRepairHandoffRow>, sqlx::Error> {
    let sql = format!(
        "{} SELECT {HANDOFF_COLUMNS} FROM devrail_repair_handoffs h WHERE h.repair_request_id=$5 AND {} ORDER BY h.created_at,h.id",
        visible_departments_cte(),
        scoped("h")
    );
    sqlx::query_as::<_, DevRailRepairHandoffRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(request_id)
        .fetch_all(pool)
        .await
}

pub async fn cancel(
    connection: &mut PgConnection,
    actor: &ActorContext,
    request_id: i64,
) -> Result<Option<DevRailRepairRequestRow>, sqlx::Error> {
    let sql = format!(
        "{} UPDATE devrail_repair_requests r SET status='cancelled', status_version=status_version+1, result_code='cancelled', claim_owner=NULL, claim_token=NULL, claim_expires_at=NULL, cancelled_at=COALESCE(cancelled_at,now()), completed_at=COALESCE(completed_at,now()), updated_at=now() WHERE r.id=$5 AND r.status IN ('pending','claimed') AND {} RETURNING {REQUEST_COLUMNS}",
        visible_departments_cte(),
        scoped("r")
    );
    let request = sqlx::query_as::<_, DevRailRepairRequestRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(request_id)
        .fetch_optional(&mut *connection)
        .await?;
    let Some(request) = request else {
        return Ok(None);
    };
    crate::app_metrics::record_repair_request("cancelled", &request.status, &request.risk_category);
    set_repair_history_context(
        connection,
        &RepairHistoryContext {
            actor,
            reason: "repair_cancelled",
            request_id: request.id,
            diagnosis_id: request.diagnosis_id,
            source_run_id: request.source_run_id,
            child_run_id: None,
            policy_version: &request.strategy_version,
            result_code: Some("cancelled"),
        },
    )
    .await?;
    let updated = sqlx::query(
        "UPDATE devrail_tasks SET status='failed', revision=revision+1, current_repair_request_id=NULL, updated_at=now() WHERE id=$1 AND organization_id=$2 AND status='repair_pending' AND current_repair_request_id=$3",
    )
    .bind(request.task_id)
    .bind(request.organization_id)
    .bind(request.id)
    .execute(&mut *connection)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "repair 取消期间任务状态发生变化".to_string(),
        ));
    }
    let task = fetch_task(connection, request.task_id, request.organization_id).await?;
    devrail::append_task_event(
        connection,
        &task,
        "repair.cancelled",
        &format!("repair:{}:cancelled", request.id),
        &serde_json::json!({"requestId": request.id, "sourceRunId": request.source_run_id}),
        "受控修复请求已取消",
    )
    .await?;
    audit_logs::record_actor(
        connection,
        actor,
        "devrail.repair.cancel",
        "devrail_repair_request",
        Some(request.id),
        serde_json::json!({"taskId": request.task_id, "sourceRunId": request.source_run_id}),
    )
    .await?;
    lifecycle_notification(
        connection,
        &request,
        "devrail.repair.cancelled",
        "warning",
        "受控修复请求已取消",
        "修复请求已在启动 Agent 前取消。",
        &format!("/devrail/repairs/{}", request.id),
    )
    .await?;
    Ok(Some(request))
}

#[cfg(test)]
pub(crate) mod integration_tests {
    use super::*;
    use crate::access::{ActorContext, ActorType, DataScope};
    use crate::db::DATABASE_TEST_LOCK;
    use crate::models::RepairPolicy;
    use crate::repositories::devrail_continuations::integration_tests::{
        fixture, test_pool, Fixture,
    };
    use chrono::Utc;
    use serde_json::json;
    use sqlx::PgPool;
    use std::collections::{BTreeMap, BTreeSet};
    use uuid::Uuid;

    fn diagnosis_input<'a>(
        evidence_ref: &'a str,
        error_summary: &'a str,
        structured_error: &'a Value,
        log_ref: Option<&'a str>,
    ) -> NewRepairDiagnosis<'a> {
        let digest = Box::leak("a".repeat(64).into_boxed_str());
        let affected_gates = Box::leak(Box::new(json!(["backend_tests"])));
        let environment_summary = Box::leak(Box::new(json!({"source": "quality_gate"})));
        NewRepairDiagnosis {
            evidence_ref,
            evidence_digest: digest,
            evidence_observed_at: Utc::now(),
            evidence_expires_at: Some(Utc::now() + chrono::Duration::minutes(5)),
            affected_gates,
            error_summary,
            structured_error,
            log_ref,
            changeset_digest: Some(digest),
            environment_summary,
        }
    }

    #[test]
    fn diagnosis_validation_rejects_sensitive_and_stale_inputs() {
        let safe = json!({"code": "quality_gate_failed"});
        let command = json!({"command": "cargo test"});
        let sensitive = diagnosis_input("quality-gate:event", "质量门禁未通过", &command, None);
        assert!(validate_diagnosis(&sensitive).is_err());

        let absolute = diagnosis_input(
            "quality-gate:event",
            "/controlled/workspace/output",
            &safe,
            None,
        );
        assert!(validate_diagnosis(&absolute).is_err());

        let stale = NewRepairDiagnosis {
            evidence_observed_at: Utc::now() - chrono::Duration::minutes(10),
            evidence_expires_at: Some(Utc::now() - chrono::Duration::minutes(1)),
            ..diagnosis_input("quality-gate:stale", "质量门禁未通过", &safe, None)
        };
        assert!(validate_diagnosis(&stale).is_err());
    }

    #[test]
    fn diagnosis_validation_bounds_context_and_rejects_raw_fields() {
        let safe = json!({"code": "quality_gate_failed"});
        let long_summary = "x".repeat(513);
        let oversized_summary = diagnosis_input(
            "quality-gate:long-summary",
            &long_summary,
            &safe,
            Some("quality-gates/backend-tests"),
        );
        assert!(validate_diagnosis(&oversized_summary).is_err());

        let full_context = json!({"request_headers": {"x-trace": "omitted"}});
        let raw_context = diagnosis_input(
            "quality-gate:full-context",
            "质量门禁未通过",
            &full_context,
            Some("quality-gates/backend-tests"),
        );
        assert!(validate_diagnosis(&raw_context).is_err());

        let controlled_path = diagnosis_input(
            "quality-gate:absolute-log",
            "质量门禁未通过",
            &safe,
            Some("/controlled/workspace/quality.log"),
        );
        assert!(validate_diagnosis(&controlled_path).is_err());

        let oversized_environment = json!({"summary": "x".repeat(8 * 1024)});
        let oversized_context = NewRepairDiagnosis {
            environment_summary: &oversized_environment,
            ..diagnosis_input(
                "quality-gate:oversized-environment",
                "质量门禁未通过",
                &safe,
                Some("quality-gates/backend-tests"),
            )
        };
        assert!(validate_diagnosis(&oversized_context).is_err());
    }

    pub(crate) async fn failed_fixture(pool: &PgPool) -> Fixture {
        let fixture = fixture(pool).await;
        sqlx::query(
            "UPDATE devrail_runs
             SET status='failed', completed_at=COALESCE(completed_at, now())
             WHERE id=$1",
        )
        .bind(fixture.source_run_id)
        .execute(pool)
        .await
        .expect("mark source run failed");
        sqlx::query("UPDATE devrail_tasks SET status='failed' WHERE id=$1")
            .bind(fixture.task_id)
            .execute(pool)
            .await
            .expect("mark source task failed");
        fixture
    }

    pub(crate) async fn configure_controlled_repair_fixture(
        pool: &PgPool,
        fixture: &Fixture,
        repository_root: &str,
        policy: &RepairPolicy,
    ) {
        sqlx::query("UPDATE devrail_environments SET workspace_root=$2 WHERE id=$1")
            .bind(fixture.environment_id)
            .bind(repository_root)
            .execute(pool)
            .await
            .expect("bind controlled repository");
        sqlx::query("UPDATE devrail_projects SET quality_gate_template=$2 WHERE id=$1")
            .bind(fixture.project_id)
            .bind(json!({"gates":[{"name":"backend_tests","command":"npm run test:ci"}]}))
            .execute(pool)
            .await
            .expect("configure quality gate");
        let workflow_snapshot = json!({
            "source":"legacy",
            "version":"legacy-v1",
            "digest":"0000000000000000000000000000000000000000000000000000000000000000",
            "config":{"repair":policy}
        });
        sqlx::query(
            "UPDATE devrail_tasks SET dispatch_snapshot=dispatch_snapshot || $2::jsonb WHERE id=$1",
        )
        .bind(fixture.task_id)
        .bind(json!({"workflow":workflow_snapshot}))
        .execute(pool)
        .await
        .expect("enable repair policy");
    }

    pub(crate) async fn callback_side_effect_counts(
        pool: &PgPool,
        source_run_id: i64,
        task_id: i64,
        request_id: i64,
    ) -> (i64, i64, i64) {
        let event_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM devrail_run_events
             WHERE run_id=$1 AND event_type='repair_callback' AND source_event_id=$2",
        )
        .bind(source_run_id)
        .bind("delivery-integration-1")
        .fetch_one(pool)
        .await
        .expect("callback event count");
        let request_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM devrail_repair_requests
             WHERE task_id=$1 AND idempotency_key=$2",
        )
        .bind(task_id)
        .bind("repair-callback:ci_callback:delivery-integration-1")
        .fetch_one(pool)
        .await
        .expect("callback request count");
        let outbox_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM devrail_outbox_events
             WHERE aggregate_type='devrail_repair_request' AND aggregate_id=$1
               AND event_type='devrail.repair.created'",
        )
        .bind(request_id)
        .fetch_one(pool)
        .await
        .expect("callback outbox count");
        (event_count, request_count, outbox_count)
    }

    async fn create_request(
        pool: &PgPool,
        fixture: &Fixture,
        idempotency_key: &str,
    ) -> DevRailRepairRequestRow {
        let digest = "a".repeat(64);
        let policy_snapshot = json!({"enabled": false, "max_repairs": 2});
        let affected_gates = json!(["backend_tests"]);
        let structured_error = json!({"code": "quality_gate_failed"});
        let environment_summary = json!({"source": "quality_gate"});
        let evidence_ref = format!("quality-gate:{}:{idempotency_key}", fixture.source_run_id);
        let mut transaction = pool.begin().await.expect("begin repair create");
        let result = create_or_get(
            &mut transaction,
            &NewRepairRequest {
                actor: &fixture.actor,
                task_id: fixture.task_id,
                source_run_id: fixture.source_run_id,
                idempotency_key,
                risk_category: "low_risk",
                strategy_version: "repair-policy-v1",
                policy_snapshot: &policy_snapshot,
                max_repairs: 2,
                cost_units: 1,
                retry_of_request_id: None,
                diagnosis: NewRepairDiagnosis {
                    evidence_ref: &evidence_ref,
                    evidence_digest: &digest,
                    evidence_observed_at: Utc::now(),
                    evidence_expires_at: Some(Utc::now() + chrono::Duration::minutes(5)),
                    affected_gates: &affected_gates,
                    error_summary: "质量门禁未通过：backend_tests",
                    structured_error: &structured_error,
                    log_ref: Some("quality-gates/backend-tests"),
                    changeset_digest: Some(&digest),
                    environment_summary: &environment_summary,
                },
            },
        )
        .await
        .expect("create repair request");
        transaction.commit().await.expect("commit repair create");
        assert!(result.1);
        result.0
    }

    #[tokio::test]
    async fn migration_adds_repair_schema_without_rewriting_historical_run_lineage() {
        let _guard = DATABASE_TEST_LOCK.lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        let fixture = fixture(&pool).await;
        let migrated = sqlx::query_as::<_, (String, Option<i64>, String, Option<i64>, Option<i16>)>(
            "SELECT t.status,t.current_repair_request_id,r.run_kind,r.repair_request_id,r.repair_sequence
             FROM devrail_tasks t
             JOIN devrail_runs r ON r.task_id=t.id AND r.organization_id=t.organization_id
             WHERE t.id=$1 AND r.id=$2",
        )
        .bind(fixture.task_id)
        .bind(fixture.source_run_id)
        .fetch_one(&pool)
        .await
        .expect("migrated historical task and run");
        assert_eq!(migrated.0, "succeeded");
        assert_eq!(migrated.1, None);
        assert_eq!(migrated.2, "primary");
        assert_eq!(migrated.3, None);
        assert_eq!(migrated.4, None);

        let repair_tables = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM information_schema.tables
             WHERE table_schema='public' AND table_name IN (
                 'devrail_repair_diagnoses', 'devrail_repair_requests',
                 'devrail_repair_approvals', 'devrail_repair_gate_reruns',
                 'devrail_repair_handoffs'
             )",
        )
        .fetch_one(&pool)
        .await
        .expect("repair schema tables");
        assert_eq!(repair_tables, 5);
    }

    #[tokio::test]
    async fn repair_permissions_are_idempotent_and_follow_the_role_matrix() {
        let _guard = DATABASE_TEST_LOCK.lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        let seed =
            include_str!("../../migrations/20260909100100_add_devrail_repair_permissions.sql");
        sqlx::raw_sql(seed)
            .execute(&pool)
            .await
            .expect("replay repair permission seed once");
        sqlx::raw_sql(seed)
            .execute(&pool)
            .await
            .expect("replay repair permission seed twice");
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT r.code,p.code
             FROM roles r
             JOIN role_permissions rp ON rp.role_id=r.id
             JOIN permissions p ON p.id=rp.permission_id
             WHERE r.code IN ('super_admin','editor','viewer','compliance_auditor','support_tier2','billing_manager')
               AND p.code LIKE 'devrail:repair:%'
             ORDER BY r.code,p.code",
        )
        .fetch_all(&pool)
        .await
        .expect("repair permission matrix");
        let mut matrix = BTreeMap::<String, BTreeSet<String>>::new();
        for (role, permission) in rows {
            matrix.entry(role).or_default().insert(permission);
        }
        let all = BTreeSet::from([
            "devrail:repair:approve".to_string(),
            "devrail:repair:cancel".to_string(),
            "devrail:repair:create".to_string(),
            "devrail:repair:handoff".to_string(),
            "devrail:repair:read".to_string(),
        ]);
        let editor = BTreeSet::from([
            "devrail:repair:cancel".to_string(),
            "devrail:repair:create".to_string(),
            "devrail:repair:read".to_string(),
        ]);
        let read_only = BTreeSet::from(["devrail:repair:read".to_string()]);
        assert_eq!(matrix.get("super_admin"), Some(&all));
        assert_eq!(matrix.get("editor"), Some(&editor));
        for role in ["viewer", "compliance_auditor", "support_tier2"] {
            assert_eq!(matrix.get(role), Some(&read_only), "role {role}");
        }
        assert!(!matrix.contains_key("billing_manager"));
    }

    #[tokio::test]
    async fn repair_requests_are_invisible_across_organizations() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = failed_fixture(&pool).await;
        let request = create_request(&pool, &fixture, "cross-organization").await;
        let suffix = Uuid::new_v4().simple().to_string();
        let organization_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO organizations (code,name) VALUES ($1,'修复隔离组织') RETURNING id",
        )
        .bind(format!("repair-isolation-{suffix}"))
        .fetch_one(&pool)
        .await
        .expect("other organization");
        let department_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO departments (organization_id,code,name)
             VALUES ($1,'root','根部门') RETURNING id",
        )
        .bind(organization_id)
        .fetch_one(&pool)
        .await
        .expect("other department");
        let user_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO users (username,password_hash,display_name,organization_id,department_id)
             VALUES ($1,'test','隔离用户',$2,$3) RETURNING id",
        )
        .bind(format!("repair-isolation-{suffix}"))
        .bind(organization_id)
        .bind(department_id)
        .fetch_one(&pool)
        .await
        .expect("other user");
        let other_actor = ActorContext {
            actor_type: ActorType::User,
            user_id,
            session_id: 1,
            organization_id,
            department_id: Some(department_id),
            data_scope: DataScope::Organization,
            permission_codes: BTreeSet::new(),
        };
        assert!(find_by_id(&pool, &other_actor, request.id)
            .await
            .expect("cross-organization lookup")
            .is_none());
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup repair isolation schema");
    }

    #[tokio::test]
    async fn handoff_retry_requires_the_current_request_and_creates_a_new_sequence() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = failed_fixture(&pool).await;
        let previous = create_request(&pool, &fixture, "handoff-retry-first").await;
        sqlx::query("UPDATE devrail_repair_requests SET status='handed_off' WHERE id=$1")
            .bind(previous.id)
            .execute(&pool)
            .await
            .expect("handoff first repair");
        sqlx::query(
            "UPDATE devrail_tasks SET status='repair_handoff', current_repair_request_id=$2 WHERE id=$1",
        )
        .bind(fixture.task_id)
        .bind(previous.id)
        .execute(&pool)
        .await
        .expect("project repair handoff");

        let digest = "a".repeat(64);
        let policy_snapshot = json!({"enabled": true, "max_repairs": 2});
        let affected_gates = json!(["backend_tests"]);
        let structured_error = json!({"code": "quality_gate_failed"});
        let environment_summary = json!({"source": "quality_gate"});
        let retry_input = NewRepairRequest {
            actor: &fixture.actor,
            task_id: fixture.task_id,
            source_run_id: fixture.source_run_id,
            idempotency_key: "handoff-retry-second",
            risk_category: "low_risk",
            strategy_version: "repair-policy-v1",
            policy_snapshot: &policy_snapshot,
            max_repairs: 2,
            cost_units: 1,
            retry_of_request_id: Some(previous.id),
            diagnosis: NewRepairDiagnosis {
                evidence_ref: "quality-gate:handoff-retry-second",
                evidence_digest: &digest,
                evidence_observed_at: Utc::now(),
                evidence_expires_at: Some(Utc::now() + chrono::Duration::minutes(5)),
                affected_gates: &affected_gates,
                error_summary: "质量门禁未通过：backend_tests",
                structured_error: &structured_error,
                log_ref: Some("quality-gates/backend-tests"),
                changeset_digest: Some(&digest),
                environment_summary: &environment_summary,
            },
        };
        let mut transaction = pool.begin().await.expect("begin handoff retry");
        let (retry, created) = create_or_get(&mut transaction, &retry_input)
            .await
            .expect("create handoff retry");
        transaction.commit().await.expect("commit handoff retry");
        assert!(created);
        assert_eq!(retry.repair_sequence, 2);
        assert_ne!(retry.id, previous.id);

        let task_status: String =
            sqlx::query_scalar("SELECT status FROM devrail_tasks WHERE id=$1")
                .bind(fixture.task_id)
                .fetch_one(&pool)
                .await
                .expect("read retry task status");
        assert_eq!(task_status, "repair_pending");
        let source_status: String =
            sqlx::query_scalar("SELECT status FROM devrail_runs WHERE id=$1")
                .bind(fixture.source_run_id)
                .fetch_one(&pool)
                .await
                .expect("read source status");
        assert_eq!(source_status, "failed");
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup handoff retry schema");
    }

    #[tokio::test]
    async fn repair_claim_child_creation_and_terminal_projection_are_idempotent() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = failed_fixture(&pool).await;
        let request = create_request(&pool, &fixture, "repair-child").await;
        let claim_token = Uuid::new_v4();
        assert!(claim_pending(&pool, "repair-worker", claim_token, 10, 60)
            .await
            .expect("claim repair request")
            .iter()
            .any(|row| row.id == request.id));

        let task = sqlx::query_as::<_, DevRailTaskRow>(AssertSqlSafe(format!(
            "SELECT {TASK_COLUMNS} FROM devrail_tasks WHERE id=$1"
        )))
        .bind(fixture.task_id)
        .fetch_one(&pool)
        .await
        .expect("repair task");
        let source =
            crate::repositories::devrail_runs::find_for_recovery(&pool, fixture.source_run_id)
                .await
                .expect("repair source lookup")
                .expect("repair source run");
        let workflow_snapshot = task
            .dispatch_snapshot
            .get("workflow")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let policy = json!({"version": "repair-test-v1"});
        let startup_args = json!(["app-server"]);
        let mut transaction = pool.begin().await.expect("begin repair dispatch");
        let input = crate::repositories::devrail_runs::NewRepairRun {
            actor: &fixture.actor,
            task_id: fixture.task_id,
            snapshot_id: source.snapshot_id,
            idempotency_key: "repair:child",
            task_revision: task.revision,
            workflow_source: &task.workflow_source,
            workflow_version: &task.workflow_version,
            workflow_digest: &task.workflow_digest,
            workflow_snapshot: &workflow_snapshot,
            parent_run_id: fixture.source_run_id,
            parent_turn_id: source.turn_id.as_deref(),
            repair_request_id: request.id,
            repair_sequence: request.repair_sequence,
            harness_start_key: "repair:child:start",
            cwd: "/tmp/repair-child",
            policy: &policy,
            startup_args: &startup_args,
            model_id: None,
            department_id: fixture.actor.department_id,
        };
        let child = crate::repositories::devrail_runs::create_repair_run(&mut transaction, &input)
            .await
            .expect("create repair child")
            .expect("repair child run");
        let replay = crate::repositories::devrail_runs::create_repair_run(&mut transaction, &input)
            .await
            .expect("replay repair child")
            .expect("replayed repair child run");
        assert_eq!(child.id, replay.id);
        assert_eq!(child.run_kind, "repair");
        assert_eq!(child.repair_request_id, Some(request.id));
        assert_eq!(child.repair_sequence, Some(request.repair_sequence));
        assert_eq!(child.parent_run_id, Some(fixture.source_run_id));
        assert_eq!(
            child.harness_start_key.as_deref(),
            Some("repair:child:start")
        );
        assert!(mark_dispatched(
            &mut transaction,
            &fixture.actor,
            request.id,
            "repair-worker",
            claim_token,
            child.id,
        )
        .await
        .expect("mark repair dispatched")
        .is_some());
        let changeset_digest = "a".repeat(64);
        let gate_input = NewRepairGateRerun {
            request_id: request.id,
            gate_id: "backend_tests",
            changeset_digest: &changeset_digest,
            idempotency_key: "repair:child:gate:backend_tests",
            child_run_id: None,
        };
        let (gate, created) = create_gate_rerun(&mut transaction, &fixture.actor, &gate_input)
            .await
            .expect("create gate rerun");
        assert!(created);
        let (replayed_gate, replayed) =
            create_gate_rerun(&mut transaction, &fixture.actor, &gate_input)
                .await
                .expect("replay gate rerun");
        assert!(!replayed);
        assert_eq!(gate.id, replayed_gate.id);
        transaction.commit().await.expect("commit repair dispatch");

        let gate_token = Uuid::new_v4();
        let claimed_gates = claim_gate_reruns(&pool, "gate-worker", gate_token, 10, 60)
            .await
            .expect("claim gate rerun");
        assert_eq!(
            claimed_gates.iter().filter(|row| row.id == gate.id).count(),
            1
        );
        assert!(
            !renew_gate_rerun_claim(&pool, gate.id, "gate-worker", Uuid::new_v4(), 60,)
                .await
                .expect("reject stale gate token")
        );
        let mut completed_gate_tx = pool.begin().await.expect("begin gate rerun completion");
        let completed_gate = complete_gate_rerun(
            &mut completed_gate_tx,
            &CompletedRepairGateRerun {
                id: gate.id,
                worker_id: "gate-worker",
                claim_token: gate_token,
                status: "passed",
                result_code: Some("passed"),
                summary: Some("后端测试通过"),
                log_ref: Some("quality-gates/backend-tests"),
                duration_ms: Some(120),
            },
        )
        .await
        .expect("complete gate rerun")
        .expect("completed gate rerun");
        completed_gate_tx
            .commit()
            .await
            .expect("commit gate rerun completion");
        assert_eq!(completed_gate.status, "passed");

        let mut terminal = pool.begin().await.expect("begin repair terminal");
        assert!(crate::repositories::devrail_runs::update_run_terminal(
            &mut terminal,
            &crate::repositories::devrail_runs::TerminalRunUpdate {
                run_id: child.id,
                status: "completed",
                exit_reason: "completed",
                exit_code: Some(0),
                stderr_summary: None,
                trace_id: "repair-terminal",
                recovery_suggestion: None,
            },
        )
        .await
        .expect("repair terminal run"));
        assert!(complete_for_child_run(
            &mut terminal,
            &fixture.actor,
            child.id,
            "succeeded",
            "succeeded",
        )
        .await
        .expect("complete repair child")
        .is_some());
        terminal.commit().await.expect("commit repair terminal");
        let state = sqlx::query_as::<_, (String, String, i64)>(
            "SELECT t.status,r.status,
                    (SELECT count(*) FROM devrail_task_status_history h
                     WHERE h.task_id=t.id AND h.repair_request_id=$2
                       AND h.reason='repair_completed')
             FROM devrail_tasks t
             JOIN devrail_repair_requests r ON r.task_id=t.id
             WHERE t.id=$1 AND r.id=$2",
        )
        .bind(fixture.task_id)
        .bind(request.id)
        .fetch_one(&pool)
        .await
        .expect("repair terminal projection");
        assert_eq!(state, ("succeeded".to_string(), "succeeded".to_string(), 1));
        let source_status: String =
            sqlx::query_scalar("SELECT status FROM devrail_runs WHERE id=$1")
                .bind(fixture.source_run_id)
                .fetch_one(&pool)
                .await
                .expect("source status");
        assert_eq!(source_status, "failed");
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup repair child schema");
    }

    #[tokio::test]
    async fn repair_lifecycle_outbox_contains_only_safe_fields_and_is_idempotent() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = failed_fixture(&pool).await;
        let request = create_request(&pool, &fixture, "repair-payload").await;
        let (notification_id, payload): (i64, Value) = sqlx::query_as(
            "SELECT n.id,o.payload
             FROM devrail_notifications n
             JOIN devrail_outbox_events o
               ON o.organization_id=n.organization_id
              AND o.aggregate_type='devrail_repair_request'
              AND o.aggregate_id=n.resource_id
              AND o.payload->>'notificationId'=n.id::text
              AND o.event_type='devrail.repair.created'
             WHERE n.organization_id=$1 AND n.resource_id=$2",
        )
        .bind(fixture.actor.organization_id)
        .bind(request.id)
        .fetch_one(&pool)
        .await
        .expect("repair notification payload");
        assert_eq!(payload["notificationId"], notification_id);
        assert_eq!(payload["eventType"], "devrail.repair.created");
        assert_eq!(
            payload["deepLink"],
            format!("/devrail/repairs/{}", request.id)
        );
        assert_eq!(payload["summary"], "失败诊断已保存，等待策略与审批检查。");
        let fields = payload.as_object().expect("payload object");
        assert_eq!(fields.len(), 4);
        assert!(fields.keys().all(|key| matches!(
            key.as_str(),
            "notificationId" | "eventType" | "summary" | "deepLink"
        )));

        let mut transaction = pool.begin().await.expect("replay lifecycle notification");
        lifecycle_notification(
            &mut transaction,
            &request,
            "devrail.repair.created",
            "info",
            "受控修复请求已创建",
            "失败诊断已保存，等待策略与审批检查。",
            &format!("/devrail/repairs/{}", request.id),
        )
        .await
        .expect("replay lifecycle notification");
        transaction.commit().await.expect("commit lifecycle replay");
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM devrail_outbox_events
             WHERE organization_id=$1 AND aggregate_type='devrail_repair_request'
               AND aggregate_id=$2 AND event_type='devrail.repair.created'",
        )
        .bind(fixture.actor.organization_id)
        .bind(request.id)
        .fetch_one(&pool)
        .await
        .expect("count lifecycle outbox");
        assert_eq!(count, 1);
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup repair lifecycle schema");
    }

    #[tokio::test]
    async fn repair_approval_decision_records_idempotent_lifecycle_side_effects() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = failed_fixture(&pool).await;
        let request = create_request(&pool, &fixture, "repair-approval-lifecycle").await;
        let mut transaction = pool.begin().await.expect("begin repair approval create");
        let (approval, created) = create_approval(
            &mut transaction,
            &fixture.actor,
            &NewRepairApproval {
                request_id: request.id,
                idempotency_key: "repair-approval-lifecycle",
                risk_category: "logical_change",
                policy_version: "repair-policy-v1",
                requested_by: fixture.actor.user_id,
                expires_at: Utc::now() + chrono::Duration::minutes(5),
            },
        )
        .await
        .expect("create repair approval");
        transaction
            .commit()
            .await
            .expect("commit repair approval create");
        assert!(created);

        let mut transaction = pool.begin().await.expect("begin repair approval decision");
        let decided = decide_approval(
            &mut transaction,
            &fixture.actor,
            approval.id,
            "approved",
            Some("审批依据已确认"),
        )
        .await
        .expect("approve repair approval")
        .expect("repair approval decision");
        transaction
            .commit()
            .await
            .expect("commit repair approval decision");
        assert_eq!(decided.status, "approved");

        let event_payload: Value = sqlx::query_scalar(
            "SELECT payload FROM devrail_task_events
             WHERE organization_id=$1 AND task_id=$2 AND event_type='repair.approval_decided'",
        )
        .bind(fixture.actor.organization_id)
        .bind(request.task_id)
        .fetch_one(&pool)
        .await
        .expect("repair approval task event");
        assert_eq!(event_payload["approvalId"], approval.id);
        assert_eq!(event_payload["decision"], "approved");
        assert_eq!(event_payload["policyVersion"], "repair-policy-v1");
        assert!(event_payload.get("reason").is_none());

        let payload: Value = sqlx::query_scalar(
            "SELECT payload FROM devrail_outbox_events
             WHERE organization_id=$1 AND aggregate_type='devrail_repair_request'
               AND aggregate_id=$2 AND event_type='devrail.repair.approval_approved'",
        )
        .bind(fixture.actor.organization_id)
        .bind(request.id)
        .fetch_one(&pool)
        .await
        .expect("repair approval notification payload");
        let fields = payload.as_object().expect("payload object");
        assert_eq!(fields.len(), 4);
        assert!(fields.keys().all(|key| matches!(
            key.as_str(),
            "notificationId" | "eventType" | "summary" | "deepLink"
        )));
        assert!(!payload.to_string().contains("审批依据已确认"));

        let audit_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM audit_logs
             WHERE organization_id=$1 AND action='devrail.repair.approval.approved'
               AND target_type='devrail_repair_approval' AND target_id=$2",
        )
        .bind(fixture.actor.organization_id)
        .bind(approval.id)
        .fetch_one(&pool)
        .await
        .expect("repair approval audit");
        assert_eq!(audit_count, 1);

        let mut transaction = pool.begin().await.expect("begin approval replay");
        assert!(decide_approval(
            &mut transaction,
            &fixture.actor,
            approval.id,
            "approved",
            Some("审批依据已确认"),
        )
        .await
        .expect("replay repair approval")
        .is_none());
        transaction.commit().await.expect("commit approval replay");
        let (event_count, outbox_count): (i64, i64) = sqlx::query_as(
            "SELECT
                (SELECT count(*) FROM devrail_task_events
                 WHERE organization_id=$1 AND task_id=$2 AND event_type='repair.approval_decided'),
                (SELECT count(*) FROM devrail_outbox_events
                 WHERE organization_id=$1 AND aggregate_type='devrail_repair_request'
                   AND aggregate_id=$3 AND event_type='devrail.repair.approval_approved')",
        )
        .bind(fixture.actor.organization_id)
        .bind(request.task_id)
        .bind(request.id)
        .fetch_one(&pool)
        .await
        .expect("count repair approval side effects");
        assert_eq!((event_count, outbox_count), (1, 1));
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup repair approval schema");
    }

    #[tokio::test]
    async fn repair_transaction_rolls_back_when_outbox_write_fails() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = failed_fixture(&pool).await;
        let safe = json!({"code": "quality_gate_failed"});
        let mut transaction = pool.begin().await.expect("begin repair fault injection");
        sqlx::raw_sql(
            "CREATE FUNCTION devrail_test_fail_repair_outbox()
             RETURNS TRIGGER LANGUAGE plpgsql AS $$
             BEGIN
                 IF NEW.event_type = 'devrail.repair.created' THEN
                     RAISE EXCEPTION 'simulated repair outbox failure';
                 END IF;
                 RETURN NEW;
             END;
             $$;
             CREATE TRIGGER trg_devrail_test_fail_repair_outbox
             BEFORE INSERT ON devrail_outbox_events
             FOR EACH ROW EXECUTE FUNCTION devrail_test_fail_repair_outbox();",
        )
        .execute(&mut *transaction)
        .await
        .expect("install repair outbox fault trigger");
        let policy_snapshot = json!({"enabled": true});
        let input = NewRepairRequest {
            actor: &fixture.actor,
            task_id: fixture.task_id,
            source_run_id: fixture.source_run_id,
            idempotency_key: "repair:fault-injection",
            risk_category: "low_risk",
            strategy_version: "repair-policy-v1",
            policy_snapshot: &policy_snapshot,
            max_repairs: 2,
            cost_units: 1,
            retry_of_request_id: None,
            diagnosis: diagnosis_input(
                "quality-gate:repair-fault",
                "质量门禁未通过",
                &safe,
                Some("quality-gates/backend-tests"),
            ),
        };
        let result = create_or_get(&mut transaction, &input).await;
        assert!(matches!(result, Err(sqlx::Error::Database(_))));
        transaction
            .rollback()
            .await
            .expect("rollback repair failure");
        let facts = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64)>(
            "SELECT t.status,
                    (SELECT count(*) FROM devrail_repair_requests r WHERE r.task_id=t.id),
                    (SELECT count(*) FROM devrail_repair_diagnoses d WHERE d.task_id=t.id),
                    (SELECT count(*) FROM devrail_task_status_history h
                     WHERE h.task_id=t.id AND h.repair_request_id IS NOT NULL),
                    (SELECT count(*) FROM audit_logs a
                     WHERE a.target_type='devrail_repair_request' AND a.details->>'taskId'=$2),
                    (SELECT count(*) FROM devrail_outbox_events o
                     WHERE o.organization_id=$3 AND o.aggregate_type='devrail_repair_request'
                       AND o.payload->>'deepLink'=$4)
             FROM devrail_tasks t WHERE t.id=$1",
        )
        .bind(fixture.task_id)
        .bind(fixture.task_id.to_string())
        .bind(fixture.actor.organization_id)
        .bind(format!("/devrail/repairs/{}", fixture.task_id))
        .fetch_one(&pool)
        .await
        .expect("rolled back repair facts");
        assert_eq!(facts, ("failed".to_string(), 0, 0, 0, 0, 0));
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup repair rollback schema");
    }
}
