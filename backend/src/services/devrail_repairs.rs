//! Policy evaluation and API orchestration for controlled repair requests.

use crate::access::ActorContext;
use crate::error::ApiError;
use crate::models::*;
use crate::repositories::{devrail, devrail_continuations, devrail_repairs, devrail_runs};
use axum::body::Bytes;
use axum::http::HeaderMap;
use chrono::{Duration, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::process::Stdio;
use std::time::{Duration as StdDuration, Instant};
use tokio::process::Command;

const MAX_IDEMPOTENCY_KEY_BYTES: usize = 160;
const MAX_CALLBACK_BODY_BYTES: usize = 128 * 1024;

pub struct TrustedRepairEvidence<'a> {
    pub source: &'a str,
    pub stable_evidence_ref: &'a str,
    pub evidence_observed_at: chrono::DateTime<Utc>,
    pub evidence_expires_at: chrono::DateTime<Utc>,
    pub changeset_digest: &'a str,
    pub affected_gates: &'a serde_json::Value,
    pub error_summary: &'a str,
    pub structured_error: &'a serde_json::Value,
    pub log_ref: Option<&'a str>,
    pub environment_summary: &'a serde_json::Value,
}

fn verify_callback_signature(secret: &str, signature: &str, body: &[u8]) -> bool {
    use hmac::{Hmac, Mac};
    use sha2_legacy::Sha256;
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    let expected = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
    signature.len() == expected.len()
        && bool::from(subtle::ConstantTimeEq::ct_eq(
            signature.as_bytes(),
            expected.as_bytes(),
        ))
}

fn valid_callback_event_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'_' | b'-'))
}

fn validate_callback_body(body: &[u8]) -> Result<(), ApiError> {
    if body.len() > MAX_CALLBACK_BODY_BYTES {
        return Err(ApiError::validation("repair 回调 payload 超过大小限制"));
    }
    Ok(())
}

fn callback_actor(organization_id: i64) -> ActorContext {
    ActorContext {
        actor_type: crate::access::ActorType::System,
        user_id: 0,
        session_id: 0,
        organization_id,
        department_id: None,
        data_scope: crate::access::DataScope::All,
        permission_codes: std::collections::BTreeSet::new(),
    }
}

pub async fn handle_repair_callback(
    pool: &PgPool,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<DevRailRepairResponse, ApiError> {
    validate_callback_body(body)?;
    let secret = std::env::var("DEVRAIL_REPAIR_CALLBACK_SECRET")
        .map_err(|_| ApiError::forbidden("repair 回调未配置"))?;
    let signature = headers
        .get("x-devrail-repair-signature")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !verify_callback_signature(&secret, signature, body) {
        return Err(ApiError::forbidden("repair 回调签名无效"));
    }
    let callback: DevRailRepairCallbackRequest = serde_json::from_slice(body)
        .map_err(|_| ApiError::validation("repair 回调 payload 无效"))?;
    let source_prefix = match callback.source.as_str() {
        "ci_callback" => "ci-callback:",
        "review_event" => "review-event:",
        _ => return Err(ApiError::validation("repair 回调来源类型无效")),
    };
    if callback.organization_id < 1
        || callback.project_id < 1
        || callback.task_id < 1
        || callback.source_run_id < 1
        || !valid_callback_event_id(&callback.event_id)
    {
        return Err(ApiError::validation("repair 回调范围或事件 ID 无效"));
    }
    let stable_evidence_ref = format!("{source_prefix}{}", callback.event_id);
    let idempotency_key = format!("repair-callback:{}:{}", callback.source, callback.event_id);
    let evidence = TrustedRepairEvidence {
        source: &callback.source,
        stable_evidence_ref: &stable_evidence_ref,
        evidence_observed_at: callback.evidence_observed_at,
        evidence_expires_at: callback.evidence_expires_at,
        changeset_digest: &callback.changeset_digest,
        affected_gates: &callback.affected_gates,
        error_summary: &callback.error_summary,
        structured_error: &callback.structured_error,
        log_ref: callback.log_ref.as_deref(),
        environment_summary: &callback.environment_summary,
    };
    validate_trusted_repair_evidence(&callback_actor(callback.organization_id), &evidence)?;
    let actor = callback_actor(callback.organization_id);
    let source_run = devrail_runs::find_run(pool, &actor, callback.source_run_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("来源运行不存在"))?;
    let task = devrail::find_task_by_id(pool, &actor, callback.task_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("来源任务不存在"))?;
    if source_run.organization_id != callback.organization_id
        || source_run.task_id != callback.task_id
        || task.organization_id != callback.organization_id
        || task.project_id != callback.project_id
        || source_run.status != "failed"
        || !matches!(task.status.as_str(), "failed" | "repair_handoff")
    {
        return Err(ApiError::conflict("repair 回调来源范围或终态不匹配"));
    }
    let repair_policy = policy(&task)?;
    if repair_policy.evidence_max_age_seconds <= 0
        || callback.evidence_observed_at
            < Utc::now() - Duration::seconds(repair_policy.evidence_max_age_seconds)
    {
        crate::app_metrics::record_repair_diagnosis_rejected("evidence_expired");
        return Err(ApiError::conflict("repair 回调失败证据已过期"));
    }
    let payload = json!({
        "source": callback.source,
        "eventId": callback.event_id,
        "organizationId": callback.organization_id,
        "projectId": callback.project_id,
        "taskId": callback.task_id,
        "sourceRunId": callback.source_run_id,
        "evidenceObservedAt": callback.evidence_observed_at,
        "evidenceExpiresAt": callback.evidence_expires_at,
        "changesetDigest": callback.changeset_digest,
        "affectedGates": callback.affected_gates,
        "errorSummary": callback.error_summary,
        "structuredError": callback.structured_error,
        "logRef": callback.log_ref,
        "environmentSummary": callback.environment_summary,
        "riskCategory": callback.risk_category,
    });
    let mut transaction = pool.begin().await.map_err(db_error)?;
    devrail_runs::append_idempotent_callback_event(
        &mut transaction,
        &devrail_runs::NewRunEvent {
            run_id: callback.source_run_id,
            organization_id: callback.organization_id,
            department_id: source_run.department_id,
            owner_user_id: source_run.owner_user_id,
            event_type: "repair_callback",
            source_event_id: Some(&callback.event_id),
            idempotency_key: &idempotency_key,
            payload: &payload,
            summary: Some(&callback.error_summary),
        },
    )
    .await
    .map_err(|error| match error {
        sqlx::Error::Protocol(message) if message.contains("漂移") => {
            ApiError::conflict("repair 回调事件 payload 已变化")
        }
        other => db_error(other),
    })?;
    transaction.commit().await.map_err(db_error)?;
    create_from_trusted_evidence(
        pool,
        &actor,
        callback.source_run_id,
        &evidence,
        &CreateDevRailRepairRequest {
            idempotency_key,
            risk_category: callback.risk_category,
        },
    )
    .await
}

fn db_error(error: sqlx::Error) -> ApiError {
    match error {
        sqlx::Error::Protocol(_) | sqlx::Error::RowNotFound => {
            ApiError::conflict("受控修复状态或证据已变化，请刷新后重试")
        }
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => ApiError::unavailable(),
        error => ApiError::internal(error),
    }
}

fn valid_idempotency_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDEMPOTENCY_KEY_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'_' | b'-'))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
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

fn sensitive_json(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
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
        serde_json::Value::Array(values) => values.iter().any(sensitive_json),
        serde_json::Value::String(value) => sensitive_text(value),
        _ => false,
    }
}

fn validate_repair_evidence_fields(evidence: &TrustedRepairEvidence<'_>) -> Result<(), ApiError> {
    let expected_prefix = match evidence.source {
        "quality_gate" => "quality-gate:",
        "ci_callback" => "ci-callback:",
        "review_event" => "review-event:",
        _ => return Err(ApiError::validation("repair 证据来源类型无效")),
    };
    if !evidence.stable_evidence_ref.starts_with(expected_prefix)
        || !valid_idempotency_key(evidence.stable_evidence_ref)
        || !valid_digest(evidence.changeset_digest)
    {
        return Err(ApiError::validation("repair 证据身份或 changeset 摘要无效"));
    }
    if evidence.evidence_observed_at > Utc::now()
        || evidence.evidence_expires_at <= evidence.evidence_observed_at
        || evidence.evidence_expires_at <= Utc::now()
    {
        return Err(ApiError::conflict("repair 失败证据已过期或时间无效"));
    }
    let Some(gates) = evidence.affected_gates.as_array() else {
        return Err(ApiError::validation("repair 证据缺少受影响门禁"));
    };
    if gates.is_empty()
        || gates.len() > 16
        || gates.iter().any(|gate| {
            !gate.as_str().is_some_and(|value| {
                !value.is_empty()
                    && value.len() <= 64
                    && value.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(byte, b'-' | b'_' | b'.' | b':')
                    })
            })
        })
    {
        return Err(ApiError::validation("repair 证据门禁范围无效"));
    }
    if evidence.error_summary.trim().is_empty()
        || evidence.error_summary.len() > 512
        || sensitive_text(evidence.error_summary)
        || evidence
            .log_ref
            .is_some_and(|value| !safe_reference(value, 256))
        || sensitive_json(evidence.structured_error)
        || sensitive_json(evidence.environment_summary)
    {
        return Err(ApiError::validation("repair 证据包含未脱敏或超长字段"));
    }
    Ok(())
}

pub(crate) fn validate_trusted_repair_evidence(
    actor: &ActorContext,
    evidence: &TrustedRepairEvidence<'_>,
) -> Result<(), ApiError> {
    if !matches!(actor.actor_type, crate::access::ActorType::System) {
        crate::app_metrics::record_repair_diagnosis_rejected("policy_rejected");
        return Err(ApiError::forbidden("repair 受信任证据入口仅供后端系统调用"));
    }
    match validate_repair_evidence_fields(evidence) {
        Ok(()) => Ok(()),
        Err(error) => {
            crate::app_metrics::record_repair_diagnosis_rejected(diagnosis_rejection_reason(
                &error,
            ));
            Err(error)
        }
    }
}

fn diagnosis_rejection_reason(error: &ApiError) -> &'static str {
    match error {
        ApiError::Forbidden(_) => "policy_rejected",
        ApiError::Conflict(message) if message.contains("过期") => "evidence_expired",
        ApiError::Conflict(message) if message.contains("摘要") => "evidence_mismatch",
        ApiError::Validation(message) if message.contains("超长") => "diagnostic_too_large",
        ApiError::Validation(message) if message.contains("门禁") => "evidence_missing",
        ApiError::Validation(_) => "validation_rejected",
        _ => "other",
    }
}

fn record_policy_rejection(decision: RepairPolicyDecision) {
    match decision {
        RepairPolicyDecision::Handoff(DevRailRepairErrorCode::BudgetExceeded) => {
            crate::app_metrics::record_repair_budget_rejected();
        }
        RepairPolicyDecision::Handoff(DevRailRepairErrorCode::HookFailureCircuitOpen) => {
            crate::app_metrics::record_repair_hook_circuit();
        }
        _ => {}
    }
}

fn digest(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

fn risk_category_code(category: DevRailRepairRiskCategory) -> &'static str {
    match category {
        DevRailRepairRiskCategory::LowRisk => "low_risk",
        DevRailRepairRiskCategory::LogicalChange => "logical_change",
        DevRailRepairRiskCategory::DependencyChange => "dependency_change",
        DevRailRepairRiskCategory::RemoteWrite => "remote_write",
        DevRailRepairRiskCategory::SecurityChange => "security_change",
        DevRailRepairRiskCategory::Forbidden => "forbidden",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepairPolicyDecision {
    Auto,
    RequiresApproval,
    Handoff(DevRailRepairErrorCode),
}

pub(crate) fn evaluate_repair_policy(
    policy: &RepairPolicy,
    category: DevRailRepairRiskCategory,
    repair_count: u16,
    cost_units: u32,
    hook_failure_count: i32,
) -> RepairPolicyDecision {
    if !policy.enabled {
        return RepairPolicyDecision::Handoff(DevRailRepairErrorCode::PolicyDisabled);
    }
    if hook_failure_count >= crate::repositories::devrail_runs::MAX_HOOK_FAILURES {
        return RepairPolicyDecision::Handoff(DevRailRepairErrorCode::HookFailureCircuitOpen);
    }
    if repair_count >= policy.max_repairs || cost_units > policy.max_cost_units {
        return RepairPolicyDecision::Handoff(DevRailRepairErrorCode::BudgetExceeded);
    }
    if category == DevRailRepairRiskCategory::Forbidden {
        return RepairPolicyDecision::Handoff(DevRailRepairErrorCode::ForbiddenOperation);
    }
    if policy.approval_categories.contains(&category) {
        return RepairPolicyDecision::RequiresApproval;
    }
    if policy.auto_categories.contains(&category) {
        return RepairPolicyDecision::Auto;
    }
    RepairPolicyDecision::Handoff(DevRailRepairErrorCode::ForbiddenOperation)
}

fn policy_error_code(code: DevRailRepairErrorCode) -> &'static str {
    match code {
        DevRailRepairErrorCode::PolicyDisabled => "policy_disabled",
        DevRailRepairErrorCode::BudgetExceeded => "budget_exceeded",
        DevRailRepairErrorCode::HookFailureCircuitOpen => "hook_failure_circuit_open",
        DevRailRepairErrorCode::ForbiddenOperation => "forbidden_operation",
        _ => "manual_handoff",
    }
}

fn policy(task: &DevRailTaskRow) -> Result<RepairPolicy, ApiError> {
    let value = task
        .dispatch_snapshot
        .get("workflow")
        .and_then(|workflow| workflow.get("config"))
        .and_then(|config| config.get("repair"))
        .cloned()
        .unwrap_or_else(|| serde_json::to_value(RepairPolicy::default()).unwrap_or_default());
    serde_json::from_value(value).map_err(|_| ApiError::conflict("任务 repair 策略不可用"))
}

fn diagnosis_response(row: DevRailRepairDiagnosisRow) -> DevRailRepairDiagnosisResponse {
    let affected_gates = row
        .affected_gates
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect();
    DevRailRepairDiagnosisResponse {
        id: row.id,
        source_run_id: row.source_run_id,
        evidence_ref: row.evidence_ref,
        evidence_observed_at: row.evidence_observed_at,
        evidence_expires_at: row.evidence_expires_at,
        affected_gates,
        error_summary: row.error_summary,
        structured_error: row.structured_error,
        log_ref: row.log_ref,
        changeset_digest: row.changeset_digest,
        environment_summary: row.environment_summary,
        created_at: row.created_at,
    }
}

fn gate_rerun_response(row: DevRailRepairGateRerunRow) -> DevRailRepairGateRerunResponse {
    DevRailRepairGateRerunResponse {
        id: row.id,
        gate_id: row.gate_id,
        changeset_digest: row.changeset_digest,
        status: row.status,
        result_code: row.result_code,
        log_ref: row.log_ref,
        summary: row.summary,
        duration_ms: row.duration_ms,
        child_run_id: row.child_run_id,
        created_at: row.created_at,
        completed_at: row.completed_at,
    }
}

fn handoff_response(row: DevRailRepairHandoffRow) -> DevRailRepairHandoffResponse {
    DevRailRepairHandoffResponse {
        id: row.id,
        reason_code: row.reason_code,
        recommendation: row.recommendation,
        status: row.status,
        resolved_at: row.resolved_at,
        created_at: row.created_at,
    }
}

fn approval_response(row: DevRailRepairApprovalRow) -> DevRailRepairApprovalResponse {
    DevRailRepairApprovalResponse {
        id: row.id,
        repair_request_id: row.repair_request_id,
        risk_category: row.risk_category,
        policy_version: row.policy_version,
        status: row.status,
        requested_by: row.requested_by,
        decided_by: row.decided_by,
        decision_reason: row.decision_reason,
        expires_at: row.expires_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

async fn response(
    pool: &PgPool,
    actor: &ActorContext,
    row: DevRailRepairRequestRow,
) -> Result<DevRailRepairResponse, ApiError> {
    let diagnosis = devrail_repairs::find_diagnosis(pool, actor, row.diagnosis_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("受控修复请求不存在"))?;
    let (gates, handoffs, approval) = tokio::try_join!(
        devrail_repairs::list_gate_reruns(pool, actor, row.id),
        devrail_repairs::list_handoffs(pool, actor, row.id),
        devrail_repairs::find_latest_approval(pool, actor, row.id),
    )
    .map_err(db_error)?;
    Ok(DevRailRepairResponse {
        id: row.id,
        task_id: row.task_id,
        source_run_id: row.source_run_id,
        root_run_id: row.root_run_id,
        diagnosis_id: row.diagnosis_id,
        repair_sequence: row.repair_sequence,
        risk_category: row.risk_category,
        strategy_version: row.strategy_version,
        status: row.status,
        child_run_id: row.child_run_id,
        cost_units: row.cost_units,
        result_code: row.result_code,
        handoff_reason: row.handoff_reason,
        diagnosis: diagnosis_response(diagnosis),
        gate_reruns: gates.into_iter().map(gate_rerun_response).collect(),
        handoffs: handoffs.into_iter().map(handoff_response).collect(),
        approval: approval.map(approval_response),
        created_at: row.created_at,
        updated_at: row.updated_at,
        dispatched_at: row.dispatched_at,
        completed_at: row.completed_at,
        cancelled_at: row.cancelled_at,
    })
}

pub async fn get(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<DevRailRepairResponse, ApiError> {
    let row = devrail_repairs::find_by_id(pool, actor, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("受控修复请求不存在"))?;
    response(pool, actor, row).await
}

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    query: &DevRailListQuery,
) -> Result<DevRailRepairPage, ApiError> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let (items, total) =
        devrail_repairs::list(pool, actor, query.task_id, query.run_id, page, page_size)
            .await
            .map_err(db_error)?;
    let mut responses = Vec::with_capacity(items.len());
    for item in items {
        responses.push(response(pool, actor, item).await?);
    }
    Ok(DevRailRepairPage {
        items: responses,
        total,
        page,
        page_size,
    })
}

pub async fn create_for_failed_quality_gates(
    pool: &PgPool,
    actor: &ActorContext,
    source_run_id: i64,
    request: &CreateDevRailRepairRequest,
) -> Result<DevRailRepairResponse, ApiError> {
    create_for_failed_quality_gates_with_mode(pool, actor, source_run_id, request, None).await
}

async fn create_for_failed_quality_gates_with_mode(
    pool: &PgPool,
    actor: &ActorContext,
    source_run_id: i64,
    request: &CreateDevRailRepairRequest,
    retry_of_request_id: Option<i64>,
) -> Result<DevRailRepairResponse, ApiError> {
    let idempotency_key = request.idempotency_key.trim();
    if !valid_idempotency_key(idempotency_key) {
        return Err(ApiError::validation("幂等键格式无效"));
    }
    let source_run = devrail_runs::find_run(pool, actor, source_run_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("来源运行不存在"))?;
    let task = devrail::find_task_by_id(pool, actor, source_run.task_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("来源任务不存在"))?;
    let repair_policy = policy(&task)?;
    let evidence = devrail_repairs::failed_quality_gate_evidence(pool, actor, source_run_id)
        .await
        .map_err(db_error)?;
    if evidence.is_empty() {
        crate::app_metrics::record_repair_diagnosis_rejected("evidence_missing");
        return Err(ApiError::conflict("来源运行没有可信的失败质量门禁"));
    }
    let gates = evidence
        .iter()
        .map(|item| item.gate_id.clone())
        .collect::<Vec<_>>();
    let evidence_observed_at = evidence
        .iter()
        .map(|item| item.observed_at)
        .min()
        .ok_or_else(|| ApiError::conflict("来源运行没有可信的失败质量门禁"))?;
    if repair_policy.evidence_max_age_seconds <= 0
        || evidence_observed_at
            < Utc::now() - Duration::seconds(repair_policy.evidence_max_age_seconds)
    {
        crate::app_metrics::record_repair_diagnosis_rejected("evidence_expired");
        return Err(ApiError::conflict("失败质量门禁证据已过期"));
    }
    let base_evidence_ref = format!(
        "quality-gate:{}:{}",
        source_run_id,
        evidence
            .iter()
            .map(|item| item.event_id.to_string())
            .collect::<Vec<_>>()
            .join("-")
    );
    let evidence_ref = if retry_of_request_id.is_some() {
        format!("{base_evidence_ref}:manual:{}", idempotency_key)
    } else {
        base_evidence_ref
    };
    let evidence_digest = digest(&format!("{evidence_ref}:{}", gates.join(",")));
    let changeset_digest = evidence
        .first()
        .and_then(|item| item.changeset_digest.as_deref())
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| {
            crate::app_metrics::record_repair_diagnosis_rejected("evidence_missing");
            ApiError::conflict("失败门禁缺少可验证的 changeset 证据")
        })?;
    if evidence.iter().any(|item| {
        item.changeset_digest.as_deref() != Some(changeset_digest)
            || item.observed_at < evidence_observed_at
    }) {
        crate::app_metrics::record_repair_diagnosis_rejected("evidence_mismatch");
        return Err(ApiError::conflict("失败门禁的 changeset 证据不一致"));
    }
    let risk_category = risk_category_code(request.risk_category);
    let policy_snapshot = serde_json::to_value(&repair_policy)
        .map_err(|_| ApiError::internal("repair 策略无法序列化"))?;
    let error_summary = format!("质量门禁未通过：{}", gates.join("、"));
    if error_summary.len() > repair_policy.max_diagnostic_bytes {
        crate::app_metrics::record_repair_diagnosis_rejected("diagnostic_too_large");
        return Err(ApiError::conflict("失败门禁诊断摘要超过固化策略限制"));
    }
    let diagnosis = devrail_repairs::NewRepairDiagnosis {
        evidence_ref: &evidence_ref,
        evidence_digest: &evidence_digest,
        evidence_observed_at,
        evidence_expires_at: Some(
            evidence_observed_at + Duration::seconds(repair_policy.evidence_max_age_seconds),
        ),
        affected_gates: &json!(gates),
        error_summary: &error_summary,
        structured_error: &json!({"code": "quality_gate_failed"}),
        log_ref: evidence.iter().find_map(|item| item.log_ref.as_deref()),
        changeset_digest: Some(changeset_digest),
        environment_summary: &json!({"source": "quality_gate"}),
    };
    let system_actor = ActorContext {
        actor_type: crate::access::ActorType::System,
        ..actor.clone()
    };
    let effective_actor = if matches!(actor.actor_type, crate::access::ActorType::System) {
        let mut effective = system_actor.clone();
        effective.user_id = task.owner_user_id;
        effective
    } else {
        actor.clone()
    };
    validate_trusted_repair_evidence(
        &system_actor,
        &TrustedRepairEvidence {
            source: "quality_gate",
            stable_evidence_ref: &evidence_ref,
            evidence_observed_at,
            evidence_expires_at: evidence_observed_at
                + Duration::seconds(repair_policy.evidence_max_age_seconds),
            changeset_digest,
            affected_gates: &json!(gates),
            error_summary: &error_summary,
            structured_error: &json!({"code": "quality_gate_failed"}),
            log_ref: evidence.iter().find_map(|item| item.log_ref.as_deref()),
            environment_summary: &json!({"source": "quality_gate"}),
        },
    )?;
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let (row, created) = devrail_repairs::create_or_get(
        &mut transaction,
        &devrail_repairs::NewRepairRequest {
            actor: &effective_actor,
            task_id: task.id,
            source_run_id,
            idempotency_key,
            risk_category,
            strategy_version: "repair-policy-v1",
            policy_snapshot: &policy_snapshot,
            max_repairs: repair_policy.max_repairs as i16,
            cost_units: 1,
            retry_of_request_id,
            diagnosis,
        },
    )
    .await
    .map_err(db_error)?;
    let policy_decision = evaluate_repair_policy(
        &repair_policy,
        request.risk_category,
        0,
        1,
        task.hook_failure_count,
    );
    record_policy_rejection(policy_decision);
    match policy_decision {
        RepairPolicyDecision::Handoff(reason_code) => {
            let _ = devrail_repairs::handoff(
                &mut transaction,
                &effective_actor,
                row.id,
                &devrail_repairs::NewRepairHandoff {
                    reason_code: policy_error_code(reason_code),
                    recommendation:
                        "当前修复策略或运行安全门禁不允许自动处理，请由授权人员评估失败诊断。",
                },
            )
            .await
            .map_err(db_error)?;
        }
        RepairPolicyDecision::RequiresApproval => {
            let _ = devrail_repairs::create_approval(
                &mut transaction,
                &effective_actor,
                &devrail_repairs::NewRepairApproval {
                    request_id: row.id,
                    idempotency_key: &format!("repair:{}:approval", row.id),
                    risk_category,
                    policy_version: "repair-policy-v1",
                    requested_by: task.owner_user_id,
                    expires_at: Utc::now() + Duration::minutes(15),
                },
            )
            .await
            .map_err(db_error)?;
        }
        RepairPolicyDecision::Auto => {}
    }
    transaction.commit().await.map_err(db_error)?;
    crate::app_metrics::record_repair_request(
        if created { "created" } else { "replayed" },
        &row.status,
        risk_category,
    );
    get(pool, &effective_actor, row.id).await
}

pub async fn cancel(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<DevRailRepairResponse, ApiError> {
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let row = devrail_repairs::cancel(&mut transaction, actor, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::conflict("受控修复不能取消或已启动"))?;
    transaction.commit().await.map_err(db_error)?;
    get(pool, actor, row.id).await
}

pub async fn handoff(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
    request: &DevRailRepairHandoffRequest,
) -> Result<DevRailRepairResponse, ApiError> {
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let row = devrail_repairs::handoff(
        &mut transaction,
        actor,
        id,
        &devrail_repairs::NewRepairHandoff {
            reason_code: request.reason_code.trim(),
            recommendation: request.recommendation.trim(),
        },
    )
    .await
    .map_err(db_error)?
    .ok_or_else(|| ApiError::conflict("受控修复不能交接或已结束"))?;
    transaction.commit().await.map_err(db_error)?;
    get(pool, actor, row.id).await
}

pub async fn retry_after_handoff(
    pool: &PgPool,
    actor: &ActorContext,
    request_id: i64,
    request: &CreateDevRailRepairRequest,
) -> Result<DevRailRepairResponse, ApiError> {
    let previous = devrail_repairs::find_by_id(pool, actor, request_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("受控修复请求不存在"))?;
    if previous.status != "handed_off" {
        return Err(ApiError::conflict("只有已转人工的修复请求可以人工重试"));
    }
    if risk_category_code(request.risk_category) != previous.risk_category {
        return Err(ApiError::conflict("人工重试不能改变原修复风险类别"));
    }
    create_for_failed_quality_gates_with_mode(
        pool,
        actor,
        previous.source_run_id,
        request,
        Some(previous.id),
    )
    .await
}

pub async fn decide_approval(
    pool: &PgPool,
    actor: &ActorContext,
    approval_id: i64,
    decision: &str,
    request: &DevRailRepairApprovalDecisionRequest,
) -> Result<DevRailRepairApprovalResponse, ApiError> {
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let row = devrail_repairs::decide_approval(
        &mut transaction,
        actor,
        approval_id,
        decision,
        request.reason.as_deref(),
    )
    .await
    .map_err(db_error)?
    .ok_or_else(|| ApiError::conflict("repair 审批不存在、已处理或已过期"))?;
    if matches!(decision, "rejected" | "withdrawn") {
        let reason = if decision == "rejected" {
            "approval_rejected"
        } else {
            "approval_expired"
        };
        let _ = devrail_repairs::handoff(
            &mut transaction,
            actor,
            row.repair_request_id,
            &devrail_repairs::NewRepairHandoff {
                reason_code: reason,
                recommendation: "修复审批未通过，请由授权人员人工处理失败诊断。",
            },
        )
        .await
        .map_err(db_error)?;
    }
    transaction.commit().await.map_err(db_error)?;
    Ok(approval_response(row))
}

pub async fn withdraw_approval(
    pool: &PgPool,
    actor: &ActorContext,
    approval_id: i64,
    request: &DevRailRepairApprovalDecisionRequest,
) -> Result<DevRailRepairApprovalResponse, ApiError> {
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let row = devrail_repairs::decide_approval(
        &mut transaction,
        actor,
        approval_id,
        "withdrawn",
        request.reason.as_deref(),
    )
    .await
    .map_err(db_error)?
    .ok_or_else(|| ApiError::conflict("只有审批申请人可以撤回有效审批，且审批必须仍待处理"))?;
    let _ = devrail_repairs::handoff(
        &mut transaction,
        actor,
        row.repair_request_id,
        &devrail_repairs::NewRepairHandoff {
            reason_code: "approval_expired",
            recommendation: "修复审批已撤回，请由授权人员人工处理失败诊断。",
        },
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(approval_response(row))
}

pub(crate) async fn execute_gate_rerun(
    pool: &PgPool,
    worker_id: &str,
    claim_token: uuid::Uuid,
    rerun: DevRailRepairGateRerunRow,
) -> Result<(), ApiError> {
    let child_run_id = rerun
        .child_run_id
        .ok_or_else(|| ApiError::conflict("repair 门禁重跑缺少子运行"))?;
    let actor = ActorContext {
        actor_type: crate::access::ActorType::System,
        user_id: rerun.owner_user_id,
        session_id: 0,
        organization_id: rerun.organization_id,
        department_id: rerun.department_id,
        data_scope: crate::access::DataScope::All,
        permission_codes: Default::default(),
    };
    let run = devrail_runs::find_for_recovery(pool, child_run_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("repair 门禁重跑子运行不存在"))?;
    if run.organization_id != rerun.organization_id
        || run.task_id != rerun.task_id
        || run.repair_request_id != Some(rerun.repair_request_id)
        || run.status != "completed"
    {
        return Err(ApiError::conflict("repair 门禁重跑子运行范围或状态不匹配"));
    }
    let project = devrail::find_project(pool, &actor, rerun.project_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("repair 门禁重跑项目不存在"))?;
    let command =
        crate::services::devrail_runs::quality_gate_commands(&project.quality_gate_template)?
            .into_iter()
            .find(|(name, _)| name == &rerun.gate_id)
            .ok_or_else(|| ApiError::conflict("repair 门禁不在当前可信模板中"))?;
    let started = Instant::now();
    let result = tokio::time::timeout(
        StdDuration::from_secs(900),
        Command::new(&command.1[0])
            .args(&command.1[1..])
            .current_dir(&run.cwd)
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
    )
    .await;
    let (status, result_code, summary) = match result {
        Ok(Ok(exit_status)) if exit_status.success() => {
            ("passed", Some("passed"), "受影响门禁已通过")
        }
        Ok(Ok(_)) => ("failed", Some("command_failed"), "受影响门禁未通过"),
        Ok(Err(_)) => ("failed", Some("command_unavailable"), "受影响门禁无法启动"),
        Err(_) => ("failed", Some("timeout"), "受影响门禁执行超时"),
    };
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let completed = devrail_repairs::complete_gate_rerun(
        &mut transaction,
        &devrail_repairs::CompletedRepairGateRerun {
            id: rerun.id,
            worker_id,
            claim_token,
            status,
            result_code,
            summary: Some(summary),
            log_ref: None,
            duration_ms: Some(started.elapsed().as_millis() as i64),
        },
    )
    .await
    .map_err(db_error)?;
    crate::app_metrics::record_repair_gate_rerun(status);
    if completed.is_some() {
        let _ = devrail_repairs::finalize_gate_reruns(
            &mut transaction,
            &actor,
            rerun.repair_request_id,
        )
        .await
        .map_err(db_error)?;
    }
    transaction.commit().await.map_err(db_error)
}

/// Creates a repair request from a backend-verified CI or review event.
///
/// This entry point intentionally accepts only a System actor.  Browser/API
/// callers must use the quality-gate route, which derives evidence from the
/// persisted run journal instead of accepting caller-provided evidence.
pub async fn create_from_trusted_evidence(
    pool: &PgPool,
    actor: &ActorContext,
    source_run_id: i64,
    evidence: &TrustedRepairEvidence<'_>,
    request: &CreateDevRailRepairRequest,
) -> Result<DevRailRepairResponse, ApiError> {
    validate_trusted_repair_evidence(actor, evidence)?;
    if !matches!(evidence.source, "ci_callback" | "review_event") {
        return Err(ApiError::forbidden("repair 可信证据来源不允许直接创建请求"));
    }
    let idempotency_key = request.idempotency_key.trim();
    if !valid_idempotency_key(idempotency_key) {
        return Err(ApiError::validation("幂等键格式无效"));
    }
    let source_run = devrail_runs::find_run(pool, actor, source_run_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("来源运行不存在"))?;
    if source_run.status != "failed" {
        return Err(ApiError::conflict("可信事件来源运行不是失败终态"));
    }
    let task = devrail::find_task_by_id(pool, actor, source_run.task_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("来源任务不存在"))?;
    let effective_actor = if matches!(actor.actor_type, crate::access::ActorType::System) {
        let mut effective = actor.clone();
        effective.user_id = task.owner_user_id;
        effective
    } else {
        actor.clone()
    };
    if !matches!(task.status.as_str(), "failed" | "repair_handoff") {
        return Err(ApiError::conflict("可信事件来源任务状态不允许创建修复"));
    }
    let handoff = devrail_continuations::find_handoff(pool, actor, source_run_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::conflict("来源运行缺少可验证交接证据"))?;
    if handoff.evidence_status != "available"
        || handoff.validated_at.is_none()
        || handoff.changeset_digest != evidence.changeset_digest
    {
        return Err(ApiError::conflict("可信事件与当前 changeset 摘要不匹配"));
    }
    let repair_policy = policy(&task)?;
    if repair_policy.evidence_max_age_seconds <= 0
        || evidence.evidence_observed_at
            < Utc::now() - Duration::seconds(repair_policy.evidence_max_age_seconds)
    {
        crate::app_metrics::record_repair_diagnosis_rejected("evidence_expired");
        return Err(ApiError::conflict("可信事件失败证据已过期"));
    }
    let risk_category = risk_category_code(request.risk_category);
    let policy_snapshot = serde_json::to_value(&repair_policy)
        .map_err(|_| ApiError::internal("repair 策略无法序列化"))?;
    let evidence_digest = digest(&format!(
        "{}:{}:{}",
        evidence.source, evidence.stable_evidence_ref, evidence.changeset_digest
    ));
    let diagnosis = devrail_repairs::NewRepairDiagnosis {
        evidence_ref: evidence.stable_evidence_ref,
        evidence_digest: &evidence_digest,
        evidence_observed_at: evidence.evidence_observed_at,
        evidence_expires_at: Some(evidence.evidence_expires_at),
        affected_gates: evidence.affected_gates,
        error_summary: evidence.error_summary,
        structured_error: evidence.structured_error,
        log_ref: evidence.log_ref,
        changeset_digest: Some(evidence.changeset_digest),
        environment_summary: evidence.environment_summary,
    };
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let (row, created) = devrail_repairs::create_or_get(
        &mut transaction,
        &devrail_repairs::NewRepairRequest {
            actor: &effective_actor,
            task_id: task.id,
            source_run_id,
            idempotency_key,
            risk_category,
            strategy_version: "repair-policy-v1",
            policy_snapshot: &policy_snapshot,
            max_repairs: repair_policy.max_repairs as i16,
            cost_units: 1,
            retry_of_request_id: None,
            diagnosis,
        },
    )
    .await
    .map_err(db_error)?;
    let policy_decision = evaluate_repair_policy(
        &repair_policy,
        request.risk_category,
        0,
        1,
        task.hook_failure_count,
    );
    record_policy_rejection(policy_decision);
    match policy_decision {
        RepairPolicyDecision::Handoff(reason_code) => {
            let _ = devrail_repairs::handoff(
                &mut transaction,
                &effective_actor,
                row.id,
                &devrail_repairs::NewRepairHandoff {
                    reason_code: policy_error_code(reason_code),
                    recommendation:
                        "当前修复策略或运行安全门禁不允许自动处理，请由授权人员评估失败诊断。",
                },
            )
            .await
            .map_err(db_error)?;
        }
        RepairPolicyDecision::RequiresApproval => {
            let _ = devrail_repairs::create_approval(
                &mut transaction,
                &effective_actor,
                &devrail_repairs::NewRepairApproval {
                    request_id: row.id,
                    idempotency_key: &format!("repair:{}:approval", row.id),
                    risk_category,
                    policy_version: "repair-policy-v1",
                    requested_by: task.owner_user_id,
                    expires_at: Utc::now() + Duration::minutes(15),
                },
            )
            .await
            .map_err(db_error)?;
        }
        RepairPolicyDecision::Auto => {}
    }
    transaction.commit().await.map_err(db_error)?;
    crate::app_metrics::record_repair_request(
        if created { "created" } else { "replayed" },
        &row.status,
        risk_category,
    );
    get(pool, &effective_actor, row.id).await
}

#[cfg(test)]
mod trusted_evidence_tests {
    use super::*;
    use crate::access::{ActorType, DataScope};
    use std::collections::BTreeSet;

    fn actor(actor_type: ActorType) -> ActorContext {
        ActorContext {
            actor_type,
            user_id: 7,
            session_id: 0,
            organization_id: 1,
            department_id: None,
            data_scope: DataScope::Organization,
            permission_codes: BTreeSet::new(),
        }
    }

    fn evidence<'a>(source: &'a str) -> TrustedRepairEvidence<'a> {
        let affected_gates = Box::leak(Box::new(json!(["frontend_tests"])));
        let structured_error = Box::leak(Box::new(json!({"code": "gate_failed"})));
        let environment_summary = Box::leak(Box::new(json!({"source": "verified"})));
        let changeset_digest = Box::leak("a".repeat(64).into_boxed_str());
        TrustedRepairEvidence {
            source,
            stable_evidence_ref: match source {
                "quality_gate" => "quality-gate:run-1",
                "ci_callback" => "ci-callback:delivery-1",
                "review_event" => "review-event:review-1",
                _ => "unknown:1",
            },
            evidence_observed_at: Utc::now() - Duration::seconds(1),
            evidence_expires_at: Utc::now() + Duration::minutes(5),
            changeset_digest,
            affected_gates,
            error_summary: "质量门禁未通过",
            structured_error,
            log_ref: Some("quality-gates/frontend-tests"),
            environment_summary,
        }
    }

    #[test]
    fn trusted_evidence_requires_system_actor() {
        let error =
            validate_trusted_repair_evidence(&actor(ActorType::User), &evidence("ci_callback"))
                .expect_err("browser actor must be rejected");
        assert!(matches!(error, ApiError::Forbidden(_)));
    }

    #[test]
    fn trusted_evidence_accepts_verified_sources() {
        for source in ["quality_gate", "ci_callback", "review_event"] {
            validate_trusted_repair_evidence(&actor(ActorType::System), &evidence(source))
                .expect("verified source");
        }
    }

    #[test]
    fn trusted_evidence_rejects_unknown_source_and_expired_input() {
        assert!(
            validate_trusted_repair_evidence(&actor(ActorType::System), &evidence("forged"))
                .is_err()
        );
        let stale = TrustedRepairEvidence {
            evidence_observed_at: Utc::now() - Duration::minutes(10),
            evidence_expires_at: Utc::now() - Duration::minutes(1),
            ..evidence("ci_callback")
        };
        assert!(validate_trusted_repair_evidence(&actor(ActorType::System), &stale).is_err());
    }

    #[test]
    fn trusted_evidence_rejects_changeset_drift_and_sensitive_fields() {
        let drifted = TrustedRepairEvidence {
            changeset_digest: "not-a-digest",
            ..evidence("review_event")
        };
        assert!(validate_trusted_repair_evidence(&actor(ActorType::System), &drifted).is_err());
        let sensitive = TrustedRepairEvidence {
            structured_error: &json!({"authorization": "Bearer secret"}),
            ..evidence("review_event")
        };
        assert!(validate_trusted_repair_evidence(&actor(ActorType::System), &sensitive).is_err());
    }

    #[test]
    fn callback_signature_is_exact_and_body_is_bounded() {
        use hmac::{Hmac, Mac};
        use sha2_legacy::Sha256;

        let body = br#"{"eventId":"ci-1"}"#;
        let secret = "callback-secret";
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac");
        mac.update(body);
        let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        assert!(verify_callback_signature(secret, &signature, body));
        assert!(!verify_callback_signature(secret, "", body));
        assert!(!verify_callback_signature(
            secret,
            &signature,
            br#"{"eventId":"ci-2"}"#
        ));
        assert!(validate_callback_body(&vec![0_u8; MAX_CALLBACK_BODY_BYTES]).is_ok());
        assert!(validate_callback_body(&vec![0_u8; MAX_CALLBACK_BODY_BYTES + 1]).is_err());
    }

    #[test]
    fn callback_event_ids_cannot_expand_stable_evidence_identity() {
        assert!(valid_callback_event_id("ci-delivery-1"));
        assert!(!valid_callback_event_id(&"x".repeat(129)));
        assert!(!valid_callback_event_id("ci/event"));
    }

    #[test]
    fn repair_storage_errors_use_safe_conflict_and_availability_contracts() {
        assert!(matches!(
            db_error(sqlx::Error::Protocol("repository detail".to_string())),
            ApiError::Conflict(message) if message == "受控修复状态或证据已变化，请刷新后重试"
        ));
        assert!(matches!(
            db_error(sqlx::Error::PoolTimedOut),
            ApiError::Unavailable
        ));
    }
}

#[cfg(test)]
mod callback_integration_tests {
    use super::*;
    use crate::db::DATABASE_TEST_LOCK;
    use crate::repositories::devrail_continuations::integration_tests::{
        persist_test_handoff, test_pool, Fixture,
    };
    use crate::repositories::devrail_repairs::integration_tests::{
        callback_side_effect_counts, failed_fixture,
    };
    use axum::body::Bytes;
    use axum::http::{HeaderMap, HeaderValue};
    use chrono::Utc;
    use hmac::{Hmac, Mac};
    use serde_json::json;
    use sha2_legacy::Sha256;
    use tokio::sync::Mutex;

    static CALLBACK_ENV_LOCK: Mutex<()> = Mutex::const_new(());

    fn signed_callback(
        fixture: &Fixture,
        source: &str,
        event_id: &str,
        organization_id: i64,
        observed_at: chrono::DateTime<Utc>,
        expires_at: chrono::DateTime<Utc>,
        changeset_digest: &str,
    ) -> (Bytes, HeaderMap) {
        let body = serde_json::to_vec(&json!({
            "source": source,
            "eventId": event_id,
            "organizationId": organization_id,
            "projectId": fixture.project_id,
            "taskId": fixture.task_id,
            "sourceRunId": fixture.source_run_id,
            "evidenceObservedAt": observed_at,
            "evidenceExpiresAt": expires_at,
            "changesetDigest": changeset_digest,
            "affectedGates": ["backend_tests"],
            "errorSummary": "质量门禁未通过：backend_tests",
            "structuredError": {"code": "gate_failed"},
            "logRef": "quality-gates/backend-tests",
            "environmentSummary": {"source": "ci"},
            "riskCategory": "low_risk"
        }))
        .expect("serialize repair callback");
        let mut mac = Hmac::<Sha256>::new_from_slice(b"repair-callback-test-secret")
            .expect("callback HMAC secret");
        mac.update(&body);
        let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-devrail-repair-signature",
            HeaderValue::from_str(&signature).expect("callback signature header"),
        );
        (Bytes::from(body), headers)
    }

    #[tokio::test]
    async fn trusted_callback_rejects_forgery_stale_and_cross_scope_and_replays_idempotently() {
        let _env_guard = CALLBACK_ENV_LOCK.lock().await;
        std::env::set_var(
            "DEVRAIL_REPAIR_CALLBACK_SECRET",
            "repair-callback-test-secret",
        );
        let _guard = DATABASE_TEST_LOCK.lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        let fixture = failed_fixture(&pool).await;
        let changeset_digest = "a".repeat(64);
        persist_test_handoff(&pool, &fixture, &changeset_digest).await;

        let (body, mut headers) = signed_callback(
            &fixture,
            "ci_callback",
            "delivery-integration-1",
            fixture.actor.organization_id,
            Utc::now() - chrono::Duration::seconds(1),
            Utc::now() + chrono::Duration::minutes(5),
            &changeset_digest,
        );
        headers.remove("x-devrail-repair-signature");
        assert!(matches!(
            handle_repair_callback(&pool, &headers, &body).await,
            Err(ApiError::Forbidden(_))
        ));

        let (forged_body, forged_headers) = signed_callback(
            &fixture,
            "frontend",
            "delivery-forged",
            fixture.actor.organization_id,
            Utc::now() - chrono::Duration::seconds(1),
            Utc::now() + chrono::Duration::minutes(5),
            &changeset_digest,
        );
        assert!(matches!(
            handle_repair_callback(&pool, &forged_headers, &forged_body).await,
            Err(ApiError::Validation(_))
        ));

        let (stale_body, stale_headers) = signed_callback(
            &fixture,
            "ci_callback",
            "delivery-stale",
            fixture.actor.organization_id,
            Utc::now() - chrono::Duration::hours(1),
            Utc::now() - chrono::Duration::minutes(30),
            &changeset_digest,
        );
        assert!(matches!(
            handle_repair_callback(&pool, &stale_headers, &stale_body).await,
            Err(ApiError::Conflict(_))
        ));

        let (cross_scope_body, cross_scope_headers) = signed_callback(
            &fixture,
            "ci_callback",
            "delivery-cross-scope",
            fixture.actor.organization_id + 1,
            Utc::now() - chrono::Duration::seconds(1),
            Utc::now() + chrono::Duration::minutes(5),
            &changeset_digest,
        );
        assert!(matches!(
            handle_repair_callback(&pool, &cross_scope_headers, &cross_scope_body).await,
            Err(ApiError::NotFound(_))
        ));

        let (body, headers) = signed_callback(
            &fixture,
            "ci_callback",
            "delivery-integration-1",
            fixture.actor.organization_id,
            Utc::now() - chrono::Duration::seconds(1),
            Utc::now() + chrono::Duration::minutes(5),
            &changeset_digest,
        );
        let first = handle_repair_callback(&pool, &headers, &body)
            .await
            .expect("trusted callback");
        let second = handle_repair_callback(&pool, &headers, &body)
            .await
            .expect("idempotent callback replay");
        assert_eq!(first.id, second.id);
        assert_eq!(
            callback_side_effect_counts(&pool, fixture.source_run_id, fixture.task_id, first.id)
                .await,
            (1, 1, 1)
        );
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    use std::collections::BTreeSet;

    fn enabled_policy() -> RepairPolicy {
        RepairPolicy {
            enabled: true,
            max_repairs: 2,
            max_cost_units: 10,
            auto_categories: BTreeSet::from([DevRailRepairRiskCategory::LowRisk]),
            approval_categories: BTreeSet::from([
                DevRailRepairRiskCategory::LogicalChange,
                DevRailRepairRiskCategory::DependencyChange,
                DevRailRepairRiskCategory::RemoteWrite,
                DevRailRepairRiskCategory::SecurityChange,
            ]),
            ..RepairPolicy::default()
        }
    }

    #[test]
    fn policy_decision_separates_auto_approval_and_forbidden_categories() {
        let policy = enabled_policy();
        assert_eq!(
            evaluate_repair_policy(&policy, DevRailRepairRiskCategory::LowRisk, 0, 1, 0),
            RepairPolicyDecision::Auto
        );
        assert_eq!(
            evaluate_repair_policy(&policy, DevRailRepairRiskCategory::LogicalChange, 0, 1, 0),
            RepairPolicyDecision::RequiresApproval
        );
        assert_eq!(
            evaluate_repair_policy(&policy, DevRailRepairRiskCategory::Forbidden, 0, 1, 0),
            RepairPolicyDecision::Handoff(DevRailRepairErrorCode::ForbiddenOperation)
        );
    }

    #[test]
    fn policy_decision_fails_closed_for_disabled_budget_and_hook_breaker() {
        let mut policy = enabled_policy();
        policy.enabled = false;
        assert_eq!(
            evaluate_repair_policy(&policy, DevRailRepairRiskCategory::LowRisk, 0, 1, 0),
            RepairPolicyDecision::Handoff(DevRailRepairErrorCode::PolicyDisabled)
        );
        policy.enabled = true;
        assert_eq!(
            evaluate_repair_policy(&policy, DevRailRepairRiskCategory::LowRisk, 2, 1, 0),
            RepairPolicyDecision::Handoff(DevRailRepairErrorCode::BudgetExceeded)
        );
        assert_eq!(
            evaluate_repair_policy(&policy, DevRailRepairRiskCategory::LowRisk, 0, 11, 0),
            RepairPolicyDecision::Handoff(DevRailRepairErrorCode::BudgetExceeded)
        );
        assert_eq!(
            evaluate_repair_policy(
                &policy,
                DevRailRepairRiskCategory::LowRisk,
                0,
                1,
                crate::repositories::devrail_runs::MAX_HOOK_FAILURES,
            ),
            RepairPolicyDecision::Handoff(DevRailRepairErrorCode::HookFailureCircuitOpen)
        );
    }
}
