//! Continuation policy validation and user-triggered request orchestration.

use crate::access::ActorContext;
use crate::error::{db_error, ApiError};
use crate::models::{
    ContinuationPolicy, CreateDevRailContinuationRequest, DevRailContinuationPage,
    DevRailContinuationRequestRow, DevRailContinuationResponse, DevRailContinuationTrigger,
};
use crate::repositories::{devrail, devrail_continuations, devrail_runs};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::path::Path;

const MAX_IDEMPOTENCY_KEY_BYTES: usize = 128;

pub(crate) struct TrustedContinuationEvidence<'a> {
    pub trigger: DevRailContinuationTrigger,
    pub stable_evidence_ref: &'a str,
    pub evidence_observed_at: DateTime<Utc>,
    pub evidence_expires_at: DateTime<Utc>,
    pub changeset_digest: &'a str,
    pub redacted_context: &'a str,
    pub context_summary: &'a str,
}

fn digest(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

fn valid_idempotency_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDEMPOTENCY_KEY_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'_' | b'-'))
}

fn contains_sensitive_input(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "password",
        "passwd",
        "token",
        "authorization",
        "cookie",
        "database_url",
        "private_key",
        "secret",
        "begin rsa private key",
        "begin openssh private key",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        || value.lines().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with('/')
                || trimmed.starts_with("~/")
                || trimmed.contains("\\.\\")
                || trimmed.contains(":\\")
        })
}

fn policy(task: &crate::models::DevRailTaskRow) -> Result<ContinuationPolicy, ApiError> {
    let value = task
        .dispatch_snapshot
        .get("workflow")
        .and_then(|workflow| workflow.get("config"))
        .and_then(|config| config.get("continuation"))
        .cloned()
        .unwrap_or_else(|| serde_json::to_value(ContinuationPolicy::default()).unwrap_or_default());
    serde_json::from_value(value).map_err(|_| ApiError::conflict("任务 continuation 策略不可用"))
}

fn response(row: DevRailContinuationRequestRow) -> DevRailContinuationResponse {
    DevRailContinuationResponse {
        id: row.id,
        task_id: row.task_id,
        source_run_id: row.source_run_id,
        root_run_id: row.root_run_id,
        source_turn_id: row.source_turn_id,
        trigger_type: row.trigger_type,
        context_summary: row.context_summary,
        continuation_sequence: row.continuation_sequence,
        chain_depth: row.chain_depth,
        status: row.status,
        child_run_id: row.child_run_id,
        result_code: row.result_code,
        created_at: row.created_at,
        updated_at: row.updated_at,
        dispatched_at: row.dispatched_at,
        completed_at: row.completed_at,
        cancelled_at: row.cancelled_at,
    }
}

fn trigger_code(trigger: DevRailContinuationTrigger) -> &'static str {
    match trigger {
        DevRailContinuationTrigger::UserContext => "user_context",
        DevRailContinuationTrigger::QualityGate => "quality_gate",
        DevRailContinuationTrigger::ReviewChanges => "review_changes",
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(crate) async fn persist_handoff(
    pool: &PgPool,
    actor: &ActorContext,
    source_run_id: i64,
    workspace_root: &Path,
) -> Result<bool, ApiError> {
    let Some(source) = devrail_continuations::handoff_source(pool, source_run_id)
        .await
        .map_err(db_error)?
    else {
        return Ok(false);
    };
    if source.organization_id != actor.organization_id
        || source.owner_user_id != actor.user_id
        || source.source_run_id != source_run_id
    {
        return Ok(false);
    }
    let evidence = crate::services::devrail_workspaces::capture_handoff_evidence(
        workspace_root,
        source_run_id,
        &source.workspace_relative_path,
        source.workspace_base_commit.as_deref(),
    )
    .await?;
    let repository_identity = format!(
        "repository:{}:{}:{}",
        source.repository_id, source.repository_name, source.repository_remote_url
    );
    let repository_identity_digest = digest(&repository_identity);
    let environment_snapshot_digest = digest(&source.environment_snapshot.to_string());
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let (_, created) = devrail_continuations::create_handoff(
        &mut transaction,
        &devrail_continuations::NewRunHandoff {
            actor,
            project_id: source.project_id,
            task_id: source.task_id,
            source_run_id,
            task_snapshot_id: source.task_snapshot_id,
            repository_id: source.repository_id,
            environment_id: source.environment_id,
            task_snapshot_digest: &source.task_snapshot_digest,
            workflow_snapshot_digest: &source.workflow_snapshot_digest,
            environment_snapshot_digest: Some(&environment_snapshot_digest),
            repository_identity: &repository_identity,
            repository_identity_digest: &repository_identity_digest,
            base_commit: &evidence.base_commit,
            head_commit: Some(&evidence.head_commit),
            branch_ref: source.workspace_branch_name.as_deref(),
            changeset_ref: Some(&evidence.changeset_ref),
            changeset_digest: &evidence.changeset_digest,
            tool_versions: &source.tool_versions,
            evidence_status: "available",
            error_code: None,
            validated_at: Some(Utc::now()),
        },
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(created
        || devrail_continuations::has_valid_handoff(pool, actor, source_run_id)
            .await
            .map_err(db_error)?)
}

pub async fn create_user_context(
    pool: &PgPool,
    actor: &ActorContext,
    source_run_id: i64,
    request: &CreateDevRailContinuationRequest,
) -> Result<DevRailContinuationResponse, ApiError> {
    let idempotency_key = request.idempotency_key.trim();
    if !valid_idempotency_key(idempotency_key) {
        return Err(ApiError::validation("幂等键格式无效"));
    }

    let normalized_input = request.input.trim().to_string();
    let input_digest = digest(&normalized_input);

    // Return a previously committed request before policy or input checks.
    // This makes retries safe after the first request has projected the task
    // into continuation_pending and avoids consuming another quota slot.
    if let Some(existing) =
        devrail_continuations::find_by_idempotency(pool, actor, source_run_id, idempotency_key)
            .await
            .map_err(db_error)?
    {
        if existing.input_digest != input_digest {
            return Err(ApiError::conflict("幂等键对应不同 continuation 请求"));
        }
        crate::app_metrics::record_continuation_replay();
        crate::app_metrics::record_continuation_event(
            "created",
            &existing.status,
            &existing.trigger_type,
        );
        return Ok(response(existing));
    }

    if normalized_input.is_empty() {
        return Err(ApiError::validation("追加上下文不能为空"));
    }
    if contains_sensitive_input(&normalized_input) {
        return Err(ApiError::validation("追加上下文包含不允许的敏感信息或路径"));
    }

    let source_run = devrail_runs::find_run(pool, actor, source_run_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("运行不存在或超出数据范围"))?;
    let task = devrail::find_task_by_id(pool, actor, source_run.task_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("任务不存在或超出数据范围"))?;
    let policy = policy(&task)?;
    if !policy.enabled {
        return Err(ApiError::conflict("当前任务未启用 continuation"));
    }
    if !policy
        .allowed_triggers
        .contains(&DevRailContinuationTrigger::UserContext)
    {
        return Err(ApiError::conflict("当前任务不允许用户追加上下文"));
    }
    if normalized_input.len() > policy.max_context_bytes {
        return Err(ApiError::validation("追加上下文超过长度限制"));
    }
    if !matches!(source_run.status.as_str(), "completed" | "failed") {
        return Err(ApiError::conflict("来源运行尚未进入终态"));
    }
    if !matches!(task.status.as_str(), "succeeded" | "failed") {
        return Err(ApiError::conflict("任务当前状态不允许 continuation"));
    }
    let source_turn_id = source_run
        .turn_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("来源运行缺少可恢复 turn"))?;
    if !devrail_continuations::has_valid_handoff(pool, actor, source_run_id)
        .await
        .map_err(db_error)?
    {
        return Err(ApiError::conflict("来源运行缺少可验证交接证据"));
    }
    let root_run_id = source_run.root_run_id.unwrap_or(source_run.id);
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let sequence =
        devrail_continuations::next_sequence_in_connection(&mut transaction, actor, task.id)
            .await
            .map_err(db_error)?;
    if sequence > policy.max_continuations as i16 {
        return Err(ApiError::conflict("continuation 次数已达到策略上限"));
    }
    let chain_depth =
        devrail_continuations::next_chain_depth_in_connection(&mut transaction, actor, root_run_id)
            .await
            .map_err(db_error)?;
    if chain_depth > policy.max_chain_depth as i16 {
        return Err(ApiError::conflict("continuation 链深已达到策略上限"));
    }
    let evidence_ref = format!("user:{}:{}", actor.user_id, idempotency_key);
    let evidence_digest = digest(&format!(
        "{}:{}:{}",
        source_run_id, evidence_ref, input_digest
    ));
    let context_summary = format!("用户追加上下文（{} 字节）", normalized_input.len());
    let policy_snapshot = serde_json::to_value(&policy)
        .map_err(|_| ApiError::conflict("continuation 策略无法固化"))?;
    let input = devrail_continuations::NewContinuation {
        actor,
        project_id: task.project_id,
        task_id: task.id,
        source_run_id,
        root_run_id,
        source_turn_id,
        requested_by_user_id: actor.user_id,
        trigger_type: "user_context",
        evidence_ref: &evidence_ref,
        evidence_digest: &evidence_digest,
        evidence_observed_at: Utc::now(),
        evidence_expires_at: None,
        changeset_digest: None,
        redacted_context: &normalized_input,
        context_summary: &context_summary,
        input_digest: &input_digest,
        idempotency_key,
        continuation_sequence: sequence,
        chain_depth,
        prior_task_status: &task.status,
        expected_task_revision: task.revision,
        policy_version: &task.workflow_version,
        policy_snapshot: &policy_snapshot,
    };
    let (row, _) = devrail_continuations::create(&mut transaction, &input)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => ApiError::not_found("运行不存在或超出数据范围"),
            sqlx::Error::Protocol(message) if message.contains("幂等键") => {
                ApiError::conflict("幂等键对应不同 continuation 请求")
            }
            sqlx::Error::Protocol(message) => ApiError::conflict(message),
            other => db_error(other),
        })?;
    transaction.commit().await.map_err(db_error)?;
    crate::app_metrics::record_continuation_event("created", &row.status, &row.trigger_type);
    Ok(response(row))
}

pub(crate) async fn create_from_trusted_evidence(
    pool: &PgPool,
    actor: &ActorContext,
    source_run_id: i64,
    evidence: &TrustedContinuationEvidence<'_>,
) -> Result<DevRailContinuationResponse, ApiError> {
    if !matches!(actor.actor_type, crate::access::ActorType::System) {
        return Err(ApiError::forbidden(
            "continuation 受信任触发入口仅供后端系统调用",
        ));
    }
    if evidence.trigger == DevRailContinuationTrigger::UserContext {
        return Err(ApiError::forbidden("受信任触发入口不接受用户追加上下文"));
    }
    if !valid_idempotency_key(evidence.stable_evidence_ref)
        || !valid_digest(evidence.changeset_digest)
    {
        return Err(ApiError::validation("continuation 证据身份或摘要格式无效"));
    }
    if evidence.evidence_expires_at <= evidence.evidence_observed_at
        || evidence.evidence_expires_at <= Utc::now()
    {
        return Err(ApiError::conflict("continuation 触发证据已过期"));
    }
    let normalized_context = evidence.redacted_context.trim();
    let summary = evidence.context_summary.trim();
    if normalized_context.is_empty()
        || summary.is_empty()
        || summary.len() > 256
        || contains_sensitive_input(normalized_context)
        || contains_sensitive_input(summary)
    {
        return Err(ApiError::validation(
            "continuation 证据包含无效或未脱敏内容",
        ));
    }
    let trigger = trigger_code(evidence.trigger);
    let idempotency_key = format!("{trigger}:{}", digest(evidence.stable_evidence_ref));
    let input_digest = digest(normalized_context);
    if let Some(existing) =
        devrail_continuations::find_by_idempotency(pool, actor, source_run_id, &idempotency_key)
            .await
            .map_err(db_error)?
    {
        if existing.input_digest != input_digest
            || existing.changeset_digest.as_deref() != Some(evidence.changeset_digest)
        {
            return Err(ApiError::conflict("稳定证据对应不同 continuation 请求"));
        }
        crate::app_metrics::record_continuation_replay();
        crate::app_metrics::record_continuation_event(
            "created",
            &existing.status,
            &existing.trigger_type,
        );
        return Ok(response(existing));
    }

    let source_run = devrail_runs::find_run(pool, actor, source_run_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("运行不存在或超出数据范围"))?;
    let task = devrail::find_task_by_id(pool, actor, source_run.task_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("任务不存在或超出数据范围"))?;
    let policy = policy(&task)?;
    if !policy.enabled || !policy.allowed_triggers.contains(&evidence.trigger) {
        return Err(ApiError::conflict("当前任务不允许该 continuation 触发类型"));
    }
    if normalized_context.len() > policy.max_context_bytes {
        return Err(ApiError::validation("continuation 证据上下文超过长度限制"));
    }
    if !matches!(source_run.status.as_str(), "completed" | "failed")
        || !matches!(task.status.as_str(), "succeeded" | "failed")
    {
        return Err(ApiError::conflict("来源运行或任务状态不允许 continuation"));
    }
    let source_turn_id = source_run
        .turn_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("来源运行缺少可恢复 turn"))?;
    let handoff = devrail_continuations::find_handoff(pool, actor, source_run_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::conflict("来源运行缺少可验证交接证据"))?;
    if handoff.evidence_status != "available"
        || handoff.validated_at.is_none()
        || handoff.changeset_digest != evidence.changeset_digest
    {
        return Err(ApiError::conflict("continuation 证据与当前变更摘要不匹配"));
    }
    let root_run_id = source_run.root_run_id.unwrap_or(source_run.id);
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let sequence =
        devrail_continuations::next_sequence_in_connection(&mut transaction, actor, task.id)
            .await
            .map_err(db_error)?;
    if sequence > policy.max_continuations as i16 {
        return Err(ApiError::conflict("continuation 次数已达到策略上限"));
    }
    let chain_depth =
        devrail_continuations::next_chain_depth_in_connection(&mut transaction, actor, root_run_id)
            .await
            .map_err(db_error)?;
    if chain_depth > policy.max_chain_depth as i16 {
        return Err(ApiError::conflict("continuation 链深已达到策略上限"));
    }
    let evidence_digest = digest(&format!(
        "{}:{}:{}:{}",
        source_run_id, evidence.stable_evidence_ref, evidence.changeset_digest, input_digest
    ));
    let policy_snapshot = serde_json::to_value(&policy)
        .map_err(|_| ApiError::conflict("continuation 策略无法固化"))?;
    let (row, _) = devrail_continuations::create(
        &mut transaction,
        &devrail_continuations::NewContinuation {
            actor,
            project_id: task.project_id,
            task_id: task.id,
            source_run_id,
            root_run_id,
            source_turn_id,
            requested_by_user_id: actor.user_id,
            trigger_type: trigger,
            evidence_ref: evidence.stable_evidence_ref,
            evidence_digest: &evidence_digest,
            evidence_observed_at: evidence.evidence_observed_at,
            evidence_expires_at: Some(evidence.evidence_expires_at),
            changeset_digest: Some(evidence.changeset_digest),
            redacted_context: normalized_context,
            context_summary: summary,
            input_digest: &input_digest,
            idempotency_key: &idempotency_key,
            continuation_sequence: sequence,
            chain_depth,
            prior_task_status: &task.status,
            expected_task_revision: task.revision,
            policy_version: &task.workflow_version,
            policy_snapshot: &policy_snapshot,
        },
    )
    .await
    .map_err(|error| match error {
        sqlx::Error::RowNotFound => ApiError::not_found("运行不存在或超出数据范围"),
        sqlx::Error::Protocol(message) => ApiError::conflict(message),
        other => db_error(other),
    })?;
    transaction.commit().await.map_err(db_error)?;
    crate::app_metrics::record_continuation_event("created", &row.status, &row.trigger_type);
    Ok(response(row))
}

pub async fn get(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<DevRailContinuationResponse, ApiError> {
    devrail_continuations::find_by_id(pool, actor, id)
        .await
        .map_err(db_error)?
        .map(response)
        .ok_or_else(|| ApiError::not_found("continuation 请求不存在或超出数据范围"))
}

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: Option<i64>,
    source_run_id: Option<i64>,
    page: i64,
    page_size: i64,
) -> Result<DevRailContinuationPage, ApiError> {
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);
    let (items, total) =
        devrail_continuations::list(pool, actor, task_id, source_run_id, page, page_size)
            .await
            .map_err(db_error)?;
    Ok(DevRailContinuationPage {
        items: items.into_iter().map(response).collect(),
        total,
        page,
        page_size,
    })
}

pub async fn cancel(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<DevRailContinuationResponse, ApiError> {
    let existing = devrail_continuations::find_by_id(pool, actor, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("continuation 请求不存在或超出数据范围"))?;
    if existing.status == "dispatched" || existing.child_run_id.is_some() {
        return Err(ApiError::conflict("continuation 已派发，请取消 child run"));
    }
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let row = devrail_continuations::cancel(&mut transaction, actor, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::conflict("continuation 已进入不可取消状态"))?;
    transaction.commit().await.map_err(db_error)?;
    crate::app_metrics::record_continuation_event("cancelled", &row.status, &row.trigger_type);
    Ok(response(row))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_validation_never_echoes_sensitive_values() {
        assert!(contains_sensitive_input("password = hidden-value"));
        assert!(contains_sensitive_input("/controlled/workspace"));
        assert!(!contains_sensitive_input("请修复单元测试并重新运行"));
    }

    #[test]
    fn idempotency_keys_are_bounded_and_closed() {
        assert!(valid_idempotency_key("user:42:request-1"));
        assert!(!valid_idempotency_key(""));
        assert!(!valid_idempotency_key(
            &"x".repeat(MAX_IDEMPOTENCY_KEY_BYTES + 1)
        ));
        assert!(!valid_idempotency_key("request with spaces"));
    }

    #[test]
    fn public_request_dto_cannot_forge_trusted_trigger_fields() {
        let forged =
            serde_json::from_value::<CreateDevRailContinuationRequest>(serde_json::json!({
                "idempotencyKey": "forged-trigger",
                "input": "请继续处理",
                "triggerType": "quality_gate",
                "evidenceRef": "gate-result-forged"
            }));
        assert!(forged.is_err());
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::access::{ActorType, DataScope};
    use crate::repositories::devrail_continuations::integration_tests as repository_tests;
    use std::collections::BTreeSet;

    #[tokio::test]
    async fn user_context_enforces_policy_redaction_limits_and_handoff() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let disabled = repository_tests::fixture(&pool).await;
        repository_tests::set_continuation_policy(&pool, &disabled, false, 16_384).await;
        let request = CreateDevRailContinuationRequest {
            idempotency_key: "user-disabled".to_string(),
            input: "请继续修复测试".to_string(),
        };
        assert!(matches!(
            create_user_context(&pool, &disabled.actor, disabled.source_run_id, &request).await,
            Err(ApiError::Conflict(message)) if message.contains("未启用")
        ));

        let limited = repository_tests::fixture(&pool).await;
        repository_tests::set_continuation_policy(&pool, &limited, true, 8).await;
        let oversized = CreateDevRailContinuationRequest {
            idempotency_key: "user-oversized".to_string(),
            input: "这是超过八字节的追加上下文".to_string(),
        };
        assert!(matches!(
            create_user_context(&pool, &limited.actor, limited.source_run_id, &oversized).await,
            Err(ApiError::Validation(message)) if message.contains("长度限制")
        ));
        let secret = CreateDevRailContinuationRequest {
            idempotency_key: "user-secret".to_string(),
            input: "token = hidden-value".to_string(),
        };
        assert!(matches!(
            create_user_context(&pool, &limited.actor, limited.source_run_id, &secret).await,
            Err(ApiError::Validation(message)) if message.contains("敏感信息")
        ));

        let missing = repository_tests::fixture(&pool).await;
        repository_tests::set_continuation_policy(&pool, &missing, true, 16_384).await;
        let valid = CreateDevRailContinuationRequest {
            idempotency_key: "user-missing-handoff".to_string(),
            input: "请继续验证修复".to_string(),
        };
        assert!(matches!(
            create_user_context(&pool, &missing.actor, missing.source_run_id, &valid).await,
            Err(ApiError::Conflict(message)) if message.contains("交接证据")
        ));

        let available = repository_tests::fixture(&pool).await;
        repository_tests::set_continuation_policy(&pool, &available, true, 16_384).await;
        repository_tests::persist_test_handoff(&pool, &available, &"5".repeat(64)).await;
        let accepted = create_user_context(
            &pool,
            &available.actor,
            available.source_run_id,
            &CreateDevRailContinuationRequest {
                idempotency_key: "user-accepted".to_string(),
                input: "  请继续验证修复  ".to_string(),
            },
        )
        .await
        .expect("accepted user continuation");
        assert_eq!(accepted.status, "pending");
        let stored_context = repository_tests::stored_context(&pool, accepted.id).await;
        assert_eq!(stored_context, "请继续验证修复");
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup continuation service schema");
    }

    #[tokio::test]
    async fn trusted_trigger_rejects_forgery_expiry_and_digest_drift() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture = repository_tests::fixture(&pool).await;
        repository_tests::set_continuation_policy(&pool, &fixture, true, 16_384).await;
        let changeset_digest = "8".repeat(64);
        repository_tests::persist_test_handoff(&pool, &fixture, &changeset_digest).await;
        let evidence = TrustedContinuationEvidence {
            trigger: DevRailContinuationTrigger::QualityGate,
            stable_evidence_ref: "gate-result-stable-1",
            evidence_observed_at: Utc::now(),
            evidence_expires_at: Utc::now() + chrono::Duration::minutes(10),
            changeset_digest: &changeset_digest,
            redacted_context: "质量门禁要求修复测试失败",
            context_summary: "质量门禁要求修改",
        };
        assert!(matches!(
            create_from_trusted_evidence(&pool, &fixture.actor, fixture.source_run_id, &evidence,)
                .await,
            Err(ApiError::Forbidden(_))
        ));
        let system_actor = ActorContext {
            actor_type: ActorType::System,
            session_id: 0,
            data_scope: DataScope::All,
            permission_codes: BTreeSet::new(),
            ..fixture.actor.clone()
        };
        let first =
            create_from_trusted_evidence(&pool, &system_actor, fixture.source_run_id, &evidence)
                .await
                .expect("trusted quality gate continuation");
        let replay =
            create_from_trusted_evidence(&pool, &system_actor, fixture.source_run_id, &evidence)
                .await
                .expect("trusted evidence replay");
        assert_eq!(replay.id, first.id);
        cancel(&pool, &fixture.actor, first.id)
            .await
            .expect("cancel trusted continuation");
        let expired = TrustedContinuationEvidence {
            trigger: DevRailContinuationTrigger::ReviewChanges,
            stable_evidence_ref: "review-event-expired",
            evidence_observed_at: Utc::now() - chrono::Duration::minutes(20),
            evidence_expires_at: Utc::now() - chrono::Duration::minutes(10),
            changeset_digest: &changeset_digest,
            redacted_context: "审查要求修改",
            context_summary: "审查证据已过期",
        };
        assert!(matches!(
            create_from_trusted_evidence(
                &pool,
                &system_actor,
                fixture.source_run_id,
                &expired,
            )
            .await,
            Err(ApiError::Conflict(message)) if message.contains("已过期")
        ));
        let drifted_digest = "9".repeat(64);
        let drifted = TrustedContinuationEvidence {
            trigger: DevRailContinuationTrigger::ReviewChanges,
            stable_evidence_ref: "review-event-drifted",
            evidence_observed_at: Utc::now(),
            evidence_expires_at: Utc::now() + chrono::Duration::minutes(10),
            changeset_digest: &drifted_digest,
            redacted_context: "审查要求修改",
            context_summary: "审查摘要漂移",
        };
        assert!(matches!(
            create_from_trusted_evidence(
                &pool,
                &system_actor,
                fixture.source_run_id,
                &drifted,
            )
            .await,
            Err(ApiError::Conflict(message)) if message.contains("摘要不匹配")
        ));
        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup trusted continuation schema");
    }
}
