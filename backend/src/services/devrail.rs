//! DevRail Phase 0 business validation and transaction orchestration.

use crate::access::ActorContext;
use crate::error::{db_error, ApiError};
use crate::models::*;
use crate::orchestration::workflow::{
    self, PlatformWorkflowPolicy, WorkflowEnvironmentTemplateContext,
    WorkflowRepositoryTemplateContext, WorkflowSnapshot, WorkflowTaskContext,
    WorkflowTaskTemplateContext,
};
use crate::repositories::{self, devrail, devrail_members, devrail_workflows};
use axum::{body::Bytes, http::HeaderMap};
use chrono::Utc;
use reqwest::Client;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;
use url::Url;

const DEFAULT_PAGE: i64 = 1;
const DEFAULT_PAGE_SIZE: i64 = 20;

fn validate_temporary_branch(name: &str, source_sha: &str) -> Result<(), ApiError> {
    if name.trim().is_empty()
        || name.len() > 256
        || ["main", "master", "develop"].contains(&name)
        || name.contains("..")
        || name.starts_with('/')
        || name.ends_with('/')
        || name.contains(['~', '^', ':', '\\'])
        || name.bytes().any(|b| b.is_ascii_whitespace())
    {
        return Err(ApiError::validation("临时分支名称不安全"));
    }
    if !(7..=64).contains(&source_sha.len()) || !source_sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ApiError::validation("来源提交 SHA 无效"));
    }
    Ok(())
}

fn validate_webhook_payload(payload: &DevRailPullRequestWebhookRequest) -> Result<(), ApiError> {
    if payload.number < 1
        || payload.repository_id < 1
        || !["github", "gitlab"].contains(&payload.provider.as_str())
        || payload.status.len() > 32
        || payload.url.len() > 2048
    {
        return Err(ApiError::validation("Webhook 字段无效"));
    }
    Ok(())
}

fn validate_event_id(event_id: Option<&str>) -> Result<(), ApiError> {
    let Some(value) = event_id else {
        return Err(ApiError::validation("Webhook 缺少事件 ID"));
    };
    if value.is_empty()
        || value.len() > 256
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(ApiError::validation("Webhook 事件 ID 无效"));
    }
    Ok(())
}

fn verify_webhook_signature(secret: &str, signature: &str, body: &[u8]) -> bool {
    if secret.trim().is_empty() {
        return false;
    }
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

fn signed_body_event_id(provider: &str, body: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(provider.as_bytes());
    digest.update([0]);
    digest.update(body);
    format!("body:{}", hex::encode(digest.finalize()))
}

fn native_repository_id(value: &Value, provider: &str) -> Option<i64> {
    let candidates = if provider == "github" {
        vec![value.pointer("/repository/id"), value.get("repository_id")]
    } else {
        vec![
            value.pointer("/project/id"),
            value.get("project_id"),
            value.pointer("/object_attributes/source_project_id"),
        ]
    };
    candidates
        .into_iter()
        .flatten()
        .find_map(Value::as_i64)
        .filter(|id| *id > 0)
}

fn optional_body_event_id(value: &Value) -> Option<String> {
    ["event_id", "eventId", "delivery_id", "deliveryId"]
        .into_iter()
        .find_map(|key| value.get(key).and_then(Value::as_str).map(str::to_owned))
}

fn normalize_webhook_payload(
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<DevRailPullRequestWebhookRequest, ApiError> {
    if let Ok(payload) = serde_json::from_slice::<DevRailPullRequestWebhookRequest>(body) {
        validate_webhook_payload(&payload)?;
        validate_event_id(payload.event_id.as_deref())?;
        return Ok(payload);
    }
    let value: Value =
        serde_json::from_slice(body).map_err(|_| ApiError::validation("Webhook payload 无效"))?;
    let provider = if headers.get("x-github-event").is_some() {
        "github"
    } else if headers.get("x-gitlab-event").is_some() {
        "gitlab"
    } else {
        return Err(ApiError::validation("缺少 GitHub/GitLab Webhook 事件头"));
    };
    let repository_id = native_repository_id(&value, provider)
        .ok_or_else(|| ApiError::validation("原生 Webhook 缺少签名仓库 ID"))?;
    if let Some(header_id) = headers
        .get("x-devrail-repository-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
    {
        if header_id != repository_id {
            return Err(ApiError::validation("Webhook 仓库头与签名正文不一致"));
        }
    }
    let (number, url, status) = if provider == "github" {
        let action = value
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let pr = value
            .get("pull_request")
            .ok_or_else(|| ApiError::validation("GitHub payload 缺少 pull_request"))?;
        let status = if action == "closed" {
            if pr.get("merged").and_then(Value::as_bool).unwrap_or(false) {
                "merged"
            } else {
                "closed"
            }
        } else {
            "open"
        };
        (
            pr.get("number").and_then(Value::as_i64).unwrap_or_default(),
            pr.get("html_url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            status.to_string(),
        )
    } else {
        let attrs = value
            .get("object_attributes")
            .ok_or_else(|| ApiError::validation("GitLab payload 缺少 object_attributes"))?;
        let action = attrs
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let state = attrs.get("state").and_then(Value::as_str).unwrap_or("open");
        let status = if action == "merge" || state == "merged" {
            "merged"
        } else if state == "closed" {
            "closed"
        } else {
            "open"
        };
        (
            attrs.get("iid").and_then(Value::as_i64).unwrap_or_default(),
            attrs
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            status.to_string(),
        )
    };
    let payload = DevRailPullRequestWebhookRequest {
        provider: provider.to_string(),
        repository_id,
        number,
        url,
        status,
        event_id: optional_body_event_id(&value)
            .or_else(|| Some(signed_body_event_id(provider, body))),
    };
    validate_webhook_payload(&payload)?;
    Ok(payload)
}

pub async fn handle_pull_request_webhook(
    pool: &PgPool,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<(), ApiError> {
    let secret = std::env::var("DEVRAIL_GIT_WEBHOOK_SECRET")
        .map_err(|_| ApiError::forbidden("Webhook 未配置"))?;
    if secret.trim().is_empty() {
        return Err(ApiError::forbidden("Webhook 未配置"));
    }
    let signature = headers
        .get("x-devrail-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !verify_webhook_signature(&secret, signature, body) {
        return Err(ApiError::forbidden("Webhook 签名无效"));
    }
    let payload = normalize_webhook_payload(headers, body)?;
    validate_event_id(payload.event_id.as_deref())?;
    let mut tx = pool.begin().await.map_err(db_error)?;
    let owner =
        repositories::devrail_pull_requests::repository_owner(&mut tx, payload.repository_id)
            .await
            .map_err(db_error)?
            .ok_or_else(|| ApiError::not_found("仓库不存在或已归档"))?;
    if let Some(event_id) = payload.event_id.as_deref() {
        if !repositories::devrail_pull_requests::claim_event(&mut tx, &payload.provider, event_id)
            .await
            .map_err(db_error)?
        {
            tx.commit().await.map_err(db_error)?;
            return Ok(());
        }
    }
    let updated = repositories::devrail_pull_requests::update_webhook(
        &mut tx,
        owner.0,
        &payload.provider,
        payload.repository_id,
        payload.number,
        &payload.url,
        &payload.status,
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    if updated {
        let (organization_id, department_id, owner_user_id, project_id) = owner;
        let mut tx = pool.begin().await.map_err(db_error)?;
        let source_key = format!(
            "pull_request:{}:{}:{}",
            payload.provider, payload.repository_id, payload.number
        );
        let deep_link = format!(
            "/devrail/projects/{project_id}/repositories/{}",
            payload.repository_id
        );
        let summary = format!(
            "{} 合并请求 #{} 状态：{}",
            payload.provider, payload.number, payload.status
        );
        repositories::devrail_notifications::create(
            &mut tx,
            &repositories::devrail_notifications::NewNotification {
                organization_id,
                department_id,
                recipient_user_id: owner_user_id,
                event_type: "devrail.pull_request.updated",
                level: "info",
                title: "合并请求状态已更新",
                summary: &summary,
                resource_type: Some("devrail_repository"),
                resource_id: Some(payload.repository_id),
                deep_link: Some(&deep_link),
                source_key: &source_key,
            },
        )
        .await
        .map_err(db_error)?;
        repositories::devrail_notifications::outbox(
            &mut tx,
            organization_id,
            "notification.created",
            "devrail_pull_request",
            Some(payload.repository_id),
            &json!({"notificationSource": source_key, "eventType": "devrail.pull_request.updated"}),
        )
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod webhook_tests {
    use super::{
        normalize_webhook_payload, validate_event_id, validate_temporary_branch,
        validate_webhook_payload, verify_webhook_signature,
    };
    use crate::models::DevRailPullRequestWebhookRequest;
    use axum::body::Bytes;

    #[test]
    fn rejects_unknown_provider() {
        let payload = DevRailPullRequestWebhookRequest {
            provider: "bitbucket".into(),
            repository_id: 1,
            number: 1,
            url: "https://example.test/pr/1".into(),
            status: "open".into(),
            event_id: None,
        };
        assert!(validate_webhook_payload(&payload).is_err());
    }

    #[test]
    fn accepts_supported_provider_payload() {
        let payload = DevRailPullRequestWebhookRequest {
            provider: "github".into(),
            repository_id: 7,
            number: 12,
            url: "https://github.com/o/r/pull/12".into(),
            status: "closed".into(),
            event_id: None,
        };
        assert!(validate_webhook_payload(&payload).is_ok());
    }

    #[test]
    fn normalizes_github_native_payload() {
        use axum::http::{HeaderMap, HeaderValue};
        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", HeaderValue::from_static("pull_request"));
        let body = Bytes::from(
            r#"{"action":"closed","repository":{"id":9},"pull_request":{"number":4,"merged":true,"html_url":"https://github.com/o/r/pull/4"}}"#,
        );
        let payload = normalize_webhook_payload(&headers, &body).expect("native payload");
        assert_eq!(payload.status, "merged");
        assert_eq!(payload.number, 4);
    }

    #[test]
    fn rejects_unsigned_repository_header_mismatch() {
        use axum::http::{HeaderMap, HeaderValue};
        let mut headers = HeaderMap::new();
        headers.insert("x-github-event", HeaderValue::from_static("pull_request"));
        headers.insert("x-devrail-repository-id", HeaderValue::from_static("10"));
        let body = Bytes::from(
            r#"{"action":"opened","repository":{"id":9},"pull_request":{"number":4,"html_url":"https://github.com/o/r/pull/4"}}"#,
        );
        assert!(normalize_webhook_payload(&headers, &body).is_err());
    }

    #[test]
    fn normalizes_gitlab_native_payload() {
        use axum::http::{HeaderMap, HeaderValue};
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-gitlab-event",
            HeaderValue::from_static("Merge Request Hook"),
        );
        let body = Bytes::from(
            r#"{"project":{"id":11},"object_attributes":{"action":"update","iid":8,"state":"opened","url":"https://gitlab.com/o/r/-/merge_requests/8"}}"#,
        );
        let payload = normalize_webhook_payload(&headers, &body).expect("native payload");
        assert_eq!(payload.provider, "gitlab");
        assert_eq!(payload.status, "open");
        assert_eq!(payload.number, 8);
        assert!(payload.event_id.is_some());
    }

    #[test]
    fn validates_temporary_branch_names_and_sha() {
        assert!(validate_temporary_branch("codex/run-42", &"a".repeat(40)).is_ok());
        assert!(validate_temporary_branch("main", &"a".repeat(40)).is_err());
        assert!(validate_temporary_branch("bad..name", &"a".repeat(40)).is_err());
        assert!(validate_temporary_branch("codex/run-42", "not-a-sha").is_err());
    }

    #[test]
    fn verifies_webhook_signature_and_rejects_tampering() {
        let body = br#"{"action":"opened"}"#;
        let secret = "test-secret";
        use hmac::{Hmac, Mac};
        use sha2_legacy::Sha256;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac");
        mac.update(body);
        let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        assert!(verify_webhook_signature(secret, &signature, body));
        assert!(!verify_webhook_signature(
            secret,
            &signature,
            br#"{"action":"closed"}"#
        ));
        assert!(!verify_webhook_signature(secret, "sha256=bad", body));
        assert!(!verify_webhook_signature(" ", &signature, body));
    }

    #[test]
    fn validates_webhook_event_id_boundaries() {
        assert!(validate_event_id(Some("github-delivery-1")).is_ok());
        assert!(validate_event_id(None).is_err());
        assert!(validate_event_id(Some(" ")).is_err());
        assert!(validate_event_id(Some(&"x".repeat(257))).is_err());
        assert!(validate_event_id(Some("id\nwith-control")).is_err());
    }
}

fn paging(q: &DevRailListQuery) -> Result<(i64, i64), ApiError> {
    let page = q.page.unwrap_or(DEFAULT_PAGE);
    let size = q.page_size.unwrap_or(DEFAULT_PAGE_SIZE);
    if !(1..=10_000).contains(&page) || !(1..=100).contains(&size) {
        return Err(ApiError::validation("分页参数超出范围"));
    }
    Ok((page, size))
}

fn text(value: &str, field: &str, max: usize) -> Result<String, ApiError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max {
        return Err(ApiError::validation(format!(
            "{field}不能为空且不能超过 {max} 个字符"
        )));
    }
    Ok(value.to_string())
}

fn optional_text(value: Option<&str>, field: &str, max: usize) -> Result<Option<String>, ApiError> {
    value
        .map(|text_value| text(text_value, field, max))
        .transpose()
}

fn slug(value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if !(3..=64).contains(&value.len())
        || !value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        || value.starts_with('-')
        || value.ends_with('-')
    {
        return Err(ApiError::validation(
            "项目标识需为 3-64 位小写字母、数字或连字符",
        ));
    }
    Ok(value.to_string())
}

fn scope_department(actor: &ActorContext, requested: Option<i64>) -> Result<Option<i64>, ApiError> {
    if requested.is_some()
        && matches!(
            actor.data_scope,
            crate::access::DataScope::Department | crate::access::DataScope::SelfOnly
        )
        && requested != actor.department_id
    {
        return Err(ApiError::forbidden("不能将资源写入当前数据范围之外的部门"));
    }
    Ok(requested.or(actor.department_id))
}

fn project_response(row: DevRailProjectRow) -> DevRailProjectResponse {
    DevRailProjectResponse {
        id: row.id,
        organization_id: row.organization_id,
        department_id: row.department_id,
        owner_user_id: row.owner_user_id,
        slug: row.slug,
        name: row.name,
        description: row.description,
        status: row.status,
        default_repository_id: row.default_repository_id,
        default_environment_id: row.default_environment_id,
        notification_policy: row.notification_policy,
        quality_gate_template: row.quality_gate_template,
        created_at: row.created_at,
        updated_at: row.updated_at,
        archived_at: row.archived_at,
    }
}

fn project_policy_response(row: DevRailProjectRow) -> DevRailProjectPolicyResponse {
    DevRailProjectPolicyResponse {
        project_id: row.id,
        notification_policy: row.notification_policy,
        quality_gate_template: row.quality_gate_template,
    }
}

pub async fn get_project_policy(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<DevRailProjectPolicyResponse, ApiError> {
    devrail::find_project(pool, actor, id)
        .await
        .map_err(db_error)?
        .map(project_policy_response)
        .ok_or_else(|| ApiError::not_found("项目不存在或超出数据范围"))
}

pub async fn update_project_policy(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
    req: &UpdateDevRailProjectPolicyRequest,
) -> Result<DevRailProjectPolicyResponse, ApiError> {
    if req.notification_policy.is_none() && req.quality_gate_template.is_none() {
        return Err(ApiError::validation("至少需要提供一个策略字段"));
    }
    let update = UpdateDevRailProjectRequest {
        name: None,
        description: NullablePatch::Missing,
        department_id: NullablePatch::Missing,
        status: None,
        default_repository_id: NullablePatch::Missing,
        default_environment_id: NullablePatch::Missing,
        notification_policy: req.notification_policy.clone(),
        quality_gate_template: req.quality_gate_template.clone(),
    };
    update_project(pool, actor, id, &update).await?;
    get_project_policy(pool, actor, id).await
}
fn repository_response(row: DevRailRepositoryRow) -> DevRailRepositoryResponse {
    DevRailRepositoryResponse {
        id: row.id,
        organization_id: row.organization_id,
        department_id: row.department_id,
        owner_user_id: row.owner_user_id,
        project_id: row.project_id,
        name: row.name,
        remote_url: row.remote_url,
        protocol: row.protocol,
        default_branch: row.default_branch,
        credential_configured: row.credential_ref.is_some(),
        last_sync_status: row.last_sync_status,
        last_head_sha: row.last_head_sha,
        last_remote_branch: row.last_remote_branch,
        last_remote_branch_count: row.last_remote_branch_count,
        created_at: row.created_at,
        updated_at: row.updated_at,
        archived_at: row.archived_at,
    }
}
fn environment_response(row: DevRailEnvironmentRow) -> DevRailEnvironmentResponse {
    let secret_ref_names = row
        .secret_refs
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    DevRailEnvironmentResponse {
        id: row.id,
        organization_id: row.organization_id,
        department_id: row.department_id,
        owner_user_id: row.owner_user_id,
        project_id: row.project_id,
        name: row.name,
        workspace_root: row.workspace_root,
        network_mode: row.network_mode,
        tool_policy: row.tool_policy,
        secret_ref_names,
        max_duration_secs: row.max_duration_secs,
        enabled: row.enabled,
        created_at: row.created_at,
        updated_at: row.updated_at,
        archived_at: row.archived_at,
    }
}
fn task_response(row: DevRailTaskRow, actor: &ActorContext) -> DevRailTaskResponse {
    let continuation_policy = row
        .dispatch_snapshot
        .get("workflow")
        .and_then(|workflow| workflow.get("config"))
        .and_then(|config| config.get("continuation"))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    let repair_policy = row
        .dispatch_snapshot
        .get("workflow")
        .and_then(|workflow| workflow.get("config"))
        .and_then(|config| config.get("repair"))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    DevRailTaskResponse {
        id: row.id,
        organization_id: row.organization_id,
        department_id: row.department_id,
        owner_user_id: row.owner_user_id,
        project_id: row.project_id,
        repository_id: row.repository_id,
        environment_id: row.environment_id,
        assignee_user_id: row.assignee_user_id,
        title: row.title,
        goal: row.goal,
        background: row.background,
        acceptance_criteria: row.acceptance_criteria,
        constraints: row.constraints,
        priority: row.priority,
        status: row.status,
        revision: row.revision,
        workflow_source: row.workflow_source,
        workflow_version: row.workflow_version,
        workflow_digest: row.workflow_digest,
        continuation_policy,
        repair_policy,
        continuation_capabilities: DevRailContinuationCapabilities {
            can_read: actor.has_permission("devrail:continuation:read"),
            can_create: actor.has_permission("devrail:continuation:create"),
            can_cancel: actor.has_permission("devrail:continuation:cancel"),
        },
        scheduler_attempt: row.scheduler_attempt,
        scheduler_retry_count: row.scheduler_retry_count,
        scheduler_max_attempts: row.scheduler_max_attempts,
        scheduler_retry_at: row.scheduler_retry_at,
        scheduler_last_error: row.scheduler_last_error,
        creation_source: row.creation_source,
        source_task_id: row.source_task_id,
        source_run_id: row.source_run_id,
        followup_depth: row.followup_depth,
        blocked_reason: None,
        prerequisites: Vec::new(),
        dependents: Vec::new(),
        labels: row.labels,
        due_at: row.due_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
        archived_at: row.archived_at,
    }
}

fn dependency_response(row: DevRailTaskDependencyRow) -> DevRailTaskDependencyResponse {
    DevRailTaskDependencyResponse {
        id: row.id,
        task_id: row.task_id,
        prerequisite_task_id: row.prerequisite_task_id,
        prerequisite_title: row.prerequisite_title,
        prerequisite_status: row.prerequisite_status,
        failure_action: row.failure_action,
        cancelled_action: row.cancelled_action,
        timeout_action: row.timeout_action,
        creation_source: row.creation_source,
        created_at: row.created_at,
    }
}

fn dependent_response(row: DevRailTaskDependentRow) -> DevRailTaskDependentResponse {
    DevRailTaskDependentResponse {
        id: row.id,
        task_id: row.task_id,
        task_title: row.task_title,
        task_status: row.task_status,
        failure_action: row.failure_action,
        cancelled_action: row.cancelled_action,
        timeout_action: row.timeout_action,
        creation_source: row.creation_source,
        created_at: row.created_at,
    }
}

fn dependency_block_reason(prerequisites: &[DevRailTaskDependencyResponse]) -> Option<String> {
    prerequisites.iter().find_map(|dependency| {
        if dependency.prerequisite_status == "succeeded" {
            None
        } else if dependency.prerequisite_status == "failed" {
            Some(format!("前置任务 {} 已失败", dependency.prerequisite_title))
        } else if dependency.prerequisite_status == "cancelled" {
            Some(format!("前置任务 {} 已取消", dependency.prerequisite_title))
        } else if dependency.prerequisite_status == "skipped" {
            Some(format!("前置任务 {} 已跳过", dependency.prerequisite_title))
        } else {
            Some(format!(
                "正在等待前置任务 {}",
                dependency.prerequisite_title
            ))
        }
    })
}

async fn task_relations(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: i64,
    revision: i64,
) -> Result<DevRailTaskRelationsResponse, ApiError> {
    let started = Instant::now();
    let (prerequisite_rows, dependent_rows) = tokio::try_join!(
        devrail::list_task_dependencies(pool, actor, task_id),
        devrail::list_task_dependents(pool, actor, task_id)
    )
    .map_err(db_error)?;
    let prerequisites = prerequisite_rows
        .into_iter()
        .map(dependency_response)
        .collect::<Vec<_>>();
    crate::app_metrics::record_dependency_query_duration(started.elapsed().as_secs_f64());
    Ok(DevRailTaskRelationsResponse {
        task_id,
        revision,
        blocked_reason: dependency_block_reason(&prerequisites),
        prerequisites,
        dependents: dependent_rows.into_iter().map(dependent_response).collect(),
    })
}

pub async fn list_projects(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
) -> Result<DevRailProjectPage, ApiError> {
    let (page, size) = paging(q)?;
    let (rows, total) = tokio::try_join!(
        devrail::list_projects(pool, actor, q, page, size),
        devrail::count_projects(pool, actor, q)
    )
    .map_err(db_error)?;
    Ok(DevRailProjectPage {
        items: rows.into_iter().map(project_response).collect(),
        total,
        page,
        page_size: size,
    })
}
pub async fn get_project(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<DevRailProjectResponse, ApiError> {
    devrail::find_project(pool, actor, id)
        .await
        .map_err(db_error)?
        .map(project_response)
        .ok_or_else(|| ApiError::not_found("项目不存在或超出数据范围"))
}
pub async fn create_project(
    pool: &PgPool,
    actor: &ActorContext,
    req: &CreateDevRailProjectRequest,
) -> Result<DevRailProjectResponse, ApiError> {
    let slug = slug(&req.slug)?;
    let name = text(&req.name, "项目名称", 128)?;
    let department_id = scope_department(actor, req.department_id)?;
    let notification = req.notification_policy.clone().unwrap_or_else(|| json!({}));
    let quality = req
        .quality_gate_template
        .clone()
        .unwrap_or_else(|| json!({}));
    let mut tx = pool.begin().await.map_err(db_error)?;
    let row = devrail::create_project(
        &mut tx,
        actor,
        &devrail::NewProject {
            slug: &slug,
            name: &name,
            description: req.description.as_deref(),
            department_id,
            notification_policy: &notification,
            quality_gate_template: &quality,
        },
    )
    .await
    .map_err(db_error)?;
    devrail_members::add(&mut tx, actor, row.id, actor.user_id, "owner")
        .await
        .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.project.create",
        "devrail_project",
        Some(row.id),
        json!({"slug":slug,"name":name}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(project_response(row))
}
pub async fn update_project(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
    req: &UpdateDevRailProjectRequest,
) -> Result<DevRailProjectResponse, ApiError> {
    if req.name.is_none()
        && matches!(req.description, NullablePatch::Missing)
        && matches!(req.department_id, NullablePatch::Missing)
        && req.status.is_none()
        && matches!(req.default_repository_id, NullablePatch::Missing)
        && matches!(req.default_environment_id, NullablePatch::Missing)
        && req.notification_policy.is_none()
        && req.quality_gate_template.is_none()
    {
        return Err(ApiError::validation("至少需要提供一个待更新字段"));
    }
    let name = req
        .name
        .as_deref()
        .map(|v| text(v, "项目名称", 128))
        .transpose()?;
    let (department_set, department_id) = nullable_patch(&req.department_id);
    let department_id = if department_set {
        scope_department(actor, department_id)?
    } else {
        None
    };
    let (description_set, description) = nullable_patch(&req.description);
    let (repo_set, repo) = nullable_patch(&req.default_repository_id);
    let (env_set, env) = nullable_patch(&req.default_environment_id);
    if let Some(status) = &req.status {
        if !["active", "archived"].contains(&status.as_str()) {
            return Err(ApiError::validation("项目状态无效"));
        }
    }
    let mut tx = pool.begin().await.map_err(db_error)?;
    let changed = devrail::update_project(
        &mut tx,
        actor,
        id,
        &devrail::ProjectUpdate {
            name: name.as_deref(),
            description_set,
            description: description.as_deref(),
            department_set,
            department_id,
            status: req.status.as_deref(),
            default_repository_set: repo_set,
            default_repository_id: repo,
            default_environment_set: env_set,
            default_environment_id: env,
            notification_policy: req.notification_policy.as_ref(),
            quality_gate_template: req.quality_gate_template.as_ref(),
        },
    )
    .await
    .map_err(db_error)?;
    if !changed {
        return Err(ApiError::not_found("项目不存在或超出数据范围"));
    }
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.project.update",
        "devrail_project",
        Some(id),
        json!({"fields":["project"]}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    get_project(pool, actor, id).await
}
pub async fn archive_project(pool: &PgPool, actor: &ActorContext, id: i64) -> Result<(), ApiError> {
    let mut tx = pool.begin().await.map_err(db_error)?;
    if !devrail::archive_project(&mut tx, actor, id)
        .await
        .map_err(db_error)?
    {
        return Err(ApiError::not_found("项目不存在或超出数据范围"));
    }
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.project.archive",
        "devrail_project",
        Some(id),
        json!({}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)
}

fn remote(value: &str) -> Result<(String, String), ApiError> {
    let value = value.trim();
    if value.starts_with("git@") && value.contains(':') {
        return Ok((value.to_string(), "ssh".to_string()));
    }
    let parsed =
        Url::parse(value).map_err(|_| ApiError::validation("仓库地址必须是 HTTPS 或 SSH 地址"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(ApiError::validation("仓库 HTTPS 地址不允许携带凭据"));
    }
    Ok((value.to_string(), "https".to_string()))
}
pub async fn list_repositories(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
) -> Result<DevRailRepositoryPage, ApiError> {
    let project_id = q
        .project_id
        .ok_or_else(|| ApiError::validation("缺少 projectId"))?;
    let (page, size) = paging(q)?;
    let q2 = DevRailListQuery {
        project_id: Some(project_id),
        ..(*q).clone()
    };
    let (rows, total) = tokio::try_join!(
        devrail::list_repositories(pool, actor, &q2, page, size),
        devrail::count_repositories(pool, actor, &q2)
    )
    .map_err(db_error)?;
    Ok(DevRailRepositoryPage {
        items: rows.into_iter().map(repository_response).collect(),
        total,
        page,
        page_size: size,
    })
}
pub async fn get_repository(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
) -> Result<DevRailRepositoryResponse, ApiError> {
    devrail::find_repository(pool, actor, project_id, id)
        .await
        .map_err(db_error)?
        .map(repository_response)
        .ok_or_else(|| ApiError::not_found("仓库不存在或超出数据范围"))
}
pub async fn get_git_provider(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
) -> Result<DevRailGitProviderResponse, ApiError> {
    let repo = get_repository(pool, actor, project_id, id).await?;
    let (provider, host) = if repo.remote_url.contains("github.com") {
        ("github", "github.com")
    } else if repo.remote_url.contains("gitlab.com") {
        ("gitlab", "gitlab.com")
    } else {
        return Ok(DevRailGitProviderResponse {
            repository_id: repo.id,
            provider: "unknown".to_string(),
            owner: String::new(),
            repository: String::new(),
            default_branch: repo.default_branch,
            credential_configured: repo.credential_configured,
            compare_url: None,
            pull_request_url: None,
        });
    };
    let tail = repo
        .remote_url
        .split(host)
        .nth(1)
        .unwrap_or_default()
        .trim_matches(&['/', ':'][..])
        .trim_end_matches(".git");
    let mut parts = tail.rsplitn(2, '/');
    let name = parts.next().unwrap_or_default();
    let owner = parts.next().unwrap_or_default();
    if owner.is_empty() || name.is_empty() {
        return Err(ApiError::validation("Git 仓库地址格式无效"));
    }
    let base = format!("https://{host}/{owner}/{name}");
    Ok(DevRailGitProviderResponse {
        repository_id: repo.id,
        provider: provider.to_string(),
        owner: owner.to_string(),
        repository: name.to_string(),
        default_branch: repo.default_branch.clone(),
        credential_configured: repo.credential_configured,
        compare_url: Some(format!("{base}/compare/{}...HEAD", repo.default_branch)),
        pull_request_url: Some(format!(
            "{base}/compare/{}...HEAD?create=1",
            repo.default_branch
        )),
    })
}

pub async fn create_pull_request(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
    req: &CreateDevRailPullRequestRequest,
) -> Result<DevRailPullRequestResponse, ApiError> {
    let title = text(&req.title, "标题", 256)?;
    let source = text(&req.source_branch, "源分支", 256)?;
    let provider = get_git_provider(pool, actor, project_id, id).await?;
    let target = text(
        req.target_branch
            .as_deref()
            .unwrap_or(&provider.default_branch),
        "目标分支",
        256,
    )?;
    if source == target {
        return Err(ApiError::validation("源分支和目标分支不能相同"));
    }
    if provider.provider == "unknown" {
        return Err(ApiError::validation("暂不支持该 Git 平台"));
    }
    if !provider.credential_configured {
        return Err(ApiError::conflict("未配置 Git 平台凭据"));
    }
    let row = devrail::find_repository(pool, actor, project_id, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("仓库不存在或超出数据范围"))?;
    let env_name = row
        .credential_ref
        .as_deref()
        .unwrap_or(match provider.provider.as_str() {
            "github" => "DEVRAIL_GITHUB_TOKEN",
            _ => "DEVRAIL_GITLAB_TOKEN",
        });
    let token =
        std::env::var(env_name).map_err(|_| ApiError::conflict("Git 平台凭据未在服务端配置"))?;
    if token.trim().is_empty() {
        return Err(ApiError::conflict("Git 平台凭据未在服务端配置"));
    }
    let client = Client::builder()
        .user_agent("DevRail/1.0")
        .build()
        .map_err(ApiError::internal)?;
    let body = req.body.as_deref().unwrap_or("");
    let (url, number, status) = if provider.provider == "github" {
        let endpoint = format!(
            "https://api.github.com/repos/{}/{}/pulls",
            provider.owner, provider.repository
        );
        let response = client
            .post(endpoint)
            .bearer_auth(&token)
            .json(&json!({"title": title, "body": body, "head": source, "base": target}))
            .send()
            .await
            .map_err(|_| ApiError::conflict("创建 GitHub Pull Request 失败"))?;
        if !response.status().is_success() {
            return Err(ApiError::conflict("GitHub 拒绝创建 Pull Request"));
        }
        let data: Value = response
            .json()
            .await
            .map_err(|_| ApiError::conflict("GitHub 返回无效响应"))?;
        (
            data.get("html_url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            data.get("number").and_then(Value::as_i64),
            "open".to_string(),
        )
    } else {
        let project_path = format!("{}/{}", provider.owner, provider.repository);
        let project = urlencoding::encode(&project_path);
        let endpoint = format!("https://gitlab.com/api/v4/projects/{project}/merge_requests");
        let response = client.post(endpoint).header("PRIVATE-TOKEN", &token).json(&json!({"title": title, "description": body, "source_branch": source, "target_branch": target})).send().await.map_err(|_| ApiError::conflict("创建 GitLab Merge Request 失败"))?;
        if !response.status().is_success() {
            return Err(ApiError::conflict("GitLab 拒绝创建 Merge Request"));
        }
        let data: Value = response
            .json()
            .await
            .map_err(|_| ApiError::conflict("GitLab 返回无效响应"))?;
        (
            data.get("web_url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            data.get("iid").and_then(Value::as_i64),
            "open".to_string(),
        )
    };
    if url.is_empty() {
        return Err(ApiError::conflict("Git 平台未返回合并请求地址"));
    }
    let mut tx = pool.begin().await.map_err(db_error)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.repository.pull_request.create",
        "devrail_repository",
        Some(id),
        json!({"provider": provider.provider, "number": number, "status": status}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    if let Some(number) = number {
        let mut tx = pool.begin().await.map_err(db_error)?;
        repositories::devrail_pull_requests::upsert(
            &mut tx,
            actor.organization_id,
            id,
            &provider.provider,
            number,
            &url,
            &status,
        )
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
    }
    Ok(DevRailPullRequestResponse {
        repository_id: id,
        provider: provider.provider,
        number,
        url,
        status,
    })
}

pub async fn create_branch(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
    req: &CreateDevRailBranchRequest,
) -> Result<DevRailBranchResponse, ApiError> {
    let name = text(&req.name, "临时分支名称", 256)?;
    let source_sha = text(&req.source_sha, "来源提交 SHA", 128)?;
    validate_temporary_branch(&name, &source_sha)?;
    let provider = get_git_provider(pool, actor, project_id, id).await?;
    if !provider.credential_configured || provider.provider == "unknown" {
        return Err(ApiError::conflict("Git 平台凭据未配置或平台不受支持"));
    }
    let row = devrail::find_repository(pool, actor, project_id, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("仓库不存在或超出数据范围"))?;
    let env_name = row
        .credential_ref
        .as_deref()
        .unwrap_or(if provider.provider == "github" {
            "DEVRAIL_GITHUB_TOKEN"
        } else {
            "DEVRAIL_GITLAB_TOKEN"
        });
    let token =
        std::env::var(env_name).map_err(|_| ApiError::conflict("Git 平台凭据未在服务端配置"))?;
    let client = Client::builder()
        .user_agent("DevRail/1.0")
        .build()
        .map_err(ApiError::internal)?;
    let url = if provider.provider == "github" {
        let endpoint = format!(
            "https://api.github.com/repos/{}/{}/git/refs",
            provider.owner, provider.repository
        );
        let response = client
            .post(endpoint)
            .bearer_auth(&token)
            .json(&json!({"ref": format!("refs/heads/{name}"), "sha": source_sha}))
            .send()
            .await
            .map_err(|_| ApiError::conflict("创建 GitHub 临时分支失败"))?;
        if !response.status().is_success() {
            return Err(ApiError::conflict("GitHub 拒绝创建临时分支"));
        }
        response
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v.get("url").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_else(|| {
                format!(
                    "https://github.com/{}/{}/tree/{name}",
                    provider.owner, provider.repository
                )
            })
    } else {
        let project_path = format!("{}/{}", provider.owner, provider.repository);
        let project = urlencoding::encode(&project_path);
        let endpoint = format!("https://gitlab.com/api/v4/projects/{project}/repository/branches");
        let response = client
            .post(endpoint)
            .header("PRIVATE-TOKEN", &token)
            .query(&[("branch", name.as_str()), ("ref", source_sha.as_str())])
            .send()
            .await
            .map_err(|_| ApiError::conflict("创建 GitLab 临时分支失败"))?;
        if !response.status().is_success() {
            return Err(ApiError::conflict("GitLab 拒绝创建临时分支"));
        }
        response
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v.get("web_url").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_else(|| {
                format!(
                    "https://gitlab.com/{}/{}/-/tree/{name}",
                    provider.owner, provider.repository
                )
            })
    };
    let mut tx = pool.begin().await.map_err(db_error)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.repository.branch.create",
        "devrail_repository",
        Some(id),
        json!({"provider": provider.provider, "branch": name, "sourceSha": source_sha}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(DevRailBranchResponse {
        repository_id: id,
        provider: provider.provider,
        name,
        source_sha,
        url,
    })
}

pub async fn delete_branch(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
    req: &DeleteDevRailBranchRequest,
) -> Result<(), ApiError> {
    let name = text(&req.name, "临时分支名称", 256)?;
    if name == "main" || name == "master" || name == "develop" || name.contains("..") {
        return Err(ApiError::validation("禁止删除受保护分支"));
    }
    let provider = get_git_provider(pool, actor, project_id, id).await?;
    let row = devrail::find_repository(pool, actor, project_id, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("仓库不存在或超出数据范围"))?;
    let env_name = row
        .credential_ref
        .as_deref()
        .unwrap_or(if provider.provider == "github" {
            "DEVRAIL_GITHUB_TOKEN"
        } else {
            "DEVRAIL_GITLAB_TOKEN"
        });
    let token =
        std::env::var(env_name).map_err(|_| ApiError::conflict("Git 平台凭据未在服务端配置"))?;
    let client = Client::builder()
        .user_agent("DevRail/1.0")
        .build()
        .map_err(ApiError::internal)?;
    let response = if provider.provider == "github" {
        let endpoint = format!(
            "https://api.github.com/repos/{}/{}/git/refs/heads/{name}",
            provider.owner, provider.repository
        );
        client
            .delete(endpoint)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|_| ApiError::conflict("删除 GitHub 临时分支失败"))?
    } else {
        let project_path = format!("{}/{}", provider.owner, provider.repository);
        let project = urlencoding::encode(&project_path);
        let endpoint =
            format!("https://gitlab.com/api/v4/projects/{project}/repository/branches/{name}");
        client
            .delete(endpoint)
            .header("PRIVATE-TOKEN", &token)
            .send()
            .await
            .map_err(|_| ApiError::conflict("删除 GitLab 临时分支失败"))?
    };
    if !response.status().is_success() {
        return Err(ApiError::conflict("Git 平台拒绝删除临时分支"));
    }
    let mut tx = pool.begin().await.map_err(db_error)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.repository.branch.delete",
        "devrail_repository",
        Some(id),
        json!({"provider": provider.provider, "branch": name}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)
}

pub async fn sync_pull_request(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
    number: i64,
) -> Result<DevRailPullRequestResponse, ApiError> {
    if number < 1 {
        return Err(ApiError::validation("合并请求编号无效"));
    }
    let provider = get_git_provider(pool, actor, project_id, id).await?;
    if provider.provider == "unknown" {
        return Err(ApiError::validation("暂不支持该 Git 平台"));
    }
    if !provider.credential_configured {
        return Err(ApiError::conflict("未配置 Git 平台凭据"));
    }
    let row = devrail::find_repository(pool, actor, project_id, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("仓库不存在或超出数据范围"))?;
    let env_name = row
        .credential_ref
        .as_deref()
        .unwrap_or(if provider.provider == "github" {
            "DEVRAIL_GITHUB_TOKEN"
        } else {
            "DEVRAIL_GITLAB_TOKEN"
        });
    let token =
        std::env::var(env_name).map_err(|_| ApiError::conflict("Git 平台凭据未在服务端配置"))?;
    let client = Client::builder()
        .user_agent("DevRail/1.0")
        .build()
        .map_err(ApiError::internal)?;
    let (url, status) = if provider.provider == "github" {
        let endpoint = format!(
            "https://api.github.com/repos/{}/{}/pulls/{number}",
            provider.owner, provider.repository
        );
        let response = client
            .get(endpoint)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|_| ApiError::conflict("同步 GitHub Pull Request 失败"))?;
        if !response.status().is_success() {
            return Err(ApiError::conflict("GitHub 拒绝读取 Pull Request"));
        }
        let data: Value = response
            .json()
            .await
            .map_err(|_| ApiError::conflict("GitHub 返回无效响应"))?;
        (
            data.get("html_url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            data.get("state")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
        )
    } else {
        let project_path = format!("{}/{}", provider.owner, provider.repository);
        let project = urlencoding::encode(&project_path);
        let endpoint =
            format!("https://gitlab.com/api/v4/projects/{project}/merge_requests/{number}");
        let response = client
            .get(endpoint)
            .header("PRIVATE-TOKEN", &token)
            .send()
            .await
            .map_err(|_| ApiError::conflict("同步 GitLab Merge Request 失败"))?;
        if !response.status().is_success() {
            return Err(ApiError::conflict("GitLab 拒绝读取 Merge Request"));
        }
        let data: Value = response
            .json()
            .await
            .map_err(|_| ApiError::conflict("GitLab 返回无效响应"))?;
        (
            data.get("web_url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            data.get("state")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
        )
    };
    if url.is_empty() {
        return Err(ApiError::conflict("Git 平台未返回合并请求地址"));
    }
    let mut tx = pool.begin().await.map_err(db_error)?;
    repositories::devrail_pull_requests::upsert(
        &mut tx,
        actor.organization_id,
        id,
        &provider.provider,
        number,
        &url,
        &status,
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(DevRailPullRequestResponse {
        repository_id: id,
        provider: provider.provider,
        number: Some(number),
        url,
        status,
    })
}
pub async fn create_repository(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    req: &CreateDevRailRepositoryRequest,
) -> Result<DevRailRepositoryResponse, ApiError> {
    get_project(pool, actor, project_id).await?;
    let name = text(&req.name, "仓库名称", 128)?;
    let (url, protocol) = remote(&req.remote_url)?;
    let branch = text(
        req.default_branch.as_deref().unwrap_or("main"),
        "默认分支",
        128,
    )?;
    let department_id = scope_department(actor, req.department_id)?;
    let mut tx = pool.begin().await.map_err(db_error)?;
    let row = devrail::create_repository(
        &mut tx,
        actor,
        &devrail::NewRepository {
            project_id,
            name: &name,
            remote_url: &url,
            protocol: &protocol,
            default_branch: &branch,
            credential_ref: req.credential_ref.as_deref(),
            department_id,
        },
    )
    .await
    .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.repository.create",
        "devrail_repository",
        Some(row.id),
        json!({"projectId":project_id,"name":name,"protocol":protocol}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(repository_response(row))
}
pub async fn update_repository(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
    req: &UpdateDevRailRepositoryRequest,
) -> Result<DevRailRepositoryResponse, ApiError> {
    let (credential_set, credential) = nullable_patch(&req.credential_ref);
    let remote_value = req.remote_url.as_deref().map(remote).transpose()?;
    let name = req
        .name
        .as_deref()
        .map(|v| text(v, "仓库名称", 128))
        .transpose()?;
    let branch = req
        .default_branch
        .as_deref()
        .map(|v| text(v, "默认分支", 128))
        .transpose()?;
    if req.name.is_none()
        && req.remote_url.is_none()
        && req.default_branch.is_none()
        && !credential_set
        && req.status.is_none()
    {
        return Err(ApiError::validation("至少需要提供一个待更新字段"));
    }
    let mut tx = pool.begin().await.map_err(db_error)?;
    if !devrail::update_repository(
        &mut tx,
        actor,
        project_id,
        id,
        &devrail::RepositoryUpdate {
            name: name.as_deref(),
            remote_url: remote_value.as_ref().map(|v| v.0.as_str()),
            protocol: remote_value.as_ref().map(|v| v.1.as_str()),
            default_branch: branch.as_deref(),
            credential_set,
            credential_ref: credential.as_deref(),
            status: req.status.as_deref(),
        },
    )
    .await
    .map_err(db_error)?
    {
        return Err(ApiError::not_found("仓库不存在或超出数据范围"));
    }
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.repository.update",
        "devrail_repository",
        Some(id),
        json!({"projectId":project_id}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    get_repository(pool, actor, project_id, id).await
}

pub async fn sync_repository(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
) -> Result<DevRailRepositoryResponse, ApiError> {
    let repository = get_repository(pool, actor, project_id, id).await?;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        Command::new("git")
            .arg("-c")
            .arg("credential.helper=")
            .arg("ls-remote")
            .arg(&repository.remote_url)
            .arg("HEAD")
            .env_clear()
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output(),
    )
    .await;
    let (status, head_sha, remote_branch) = match result {
        Ok(Ok(output)) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            let sha = text
                .lines()
                .find_map(|line| {
                    line.split_whitespace().find(|value| {
                        value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit())
                    })
                })
                .filter(|v| v.len() == 40 && v.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(str::to_owned);
            let branch = text
                .lines()
                .find_map(|line| {
                    line.strip_prefix("ref: refs/heads/")?
                        .split_whitespace()
                        .next()
                })
                .filter(|v| v.len() <= 128 && !v.contains(".."))
                .map(str::to_owned);
            if sha.is_some() && branch.is_some() {
                ("synced", sha, branch)
            } else {
                ("failed", None, None)
            }
        }
        _ => ("failed", None, None),
    };
    let branch_count = if status == "synced" {
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            Command::new("git")
                .arg("-c")
                .arg("credential.helper=")
                .arg("ls-remote")
                .arg("--heads")
                .arg(&repository.remote_url)
                .env_clear()
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output(),
        )
        .await;
        match result {
            Ok(Ok(output)) if output.status.success() => {
                Some(String::from_utf8_lossy(&output.stdout).lines().count() as i64)
            }
            _ => None,
        }
    } else {
        None
    };
    let mut tx = pool.begin().await.map_err(db_error)?;
    if !devrail::update_repository_sync(
        &mut tx,
        actor,
        &devrail::RepositorySyncUpdate {
            project_id,
            id,
            status,
            head_sha: head_sha.as_deref(),
            remote_branch: remote_branch.as_deref(),
            remote_branch_count: branch_count,
        },
    )
    .await
    .map_err(db_error)?
    {
        return Err(ApiError::not_found("仓库不存在或超出数据范围"));
    }
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.repository.sync",
        "devrail_repository",
        Some(id),
        json!({"projectId": project_id, "status": status}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    get_repository(pool, actor, project_id, id).await
}

async fn git_output(root: &str, args: &[&str]) -> Result<String, ApiError> {
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env_clear()
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| ApiError::internal("工作树检查超时"))?
    .map_err(ApiError::internal)?;
    if !output.status.success() {
        return Err(ApiError::validation("环境工作区不是可读取的 Git 工作树"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub async fn get_repository_sync(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    repository_id: i64,
    environment_id: Option<i64>,
    controlled_workspace_root: &Path,
) -> Result<DevRailRepositorySyncResponse, ApiError> {
    let repository = get_repository(pool, actor, project_id, repository_id).await?;
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        Command::new("git")
            .arg("-c")
            .arg("credential.helper=")
            .arg("ls-remote")
            .arg("--heads")
            .arg(&repository.remote_url)
            .env_clear()
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| ApiError::internal("远端分支同步超时"))?
    .map_err(ApiError::internal)?;
    if !output.status.success() {
        return Err(ApiError::validation("无法读取仓库远端分支"));
    }
    let mut branches = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let sha = fields.next()?.trim();
            let reference = fields.next()?.strip_prefix("refs/heads/")?;
            if sha.len() != 40 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return None;
            }
            Some(DevRailRepositoryBranchResponse {
                name: reference.chars().take(128).collect(),
                sha: sha.to_owned(),
            })
        })
        .take(500)
        .collect::<Vec<_>>();
    branches.sort_by(|left, right| left.name.cmp(&right.name));

    let mut commits = Vec::new();
    if let Some(environment_id) = environment_id {
        let environment = get_environment(pool, actor, project_id, environment_id).await?;
        let controlled_root = tokio::fs::canonicalize(controlled_workspace_root)
            .await
            .map_err(ApiError::internal)?;
        let workspace = tokio::fs::canonicalize(environment.workspace_root.trim())
            .await
            .map_err(|_| ApiError::validation("环境工作区不存在或不可访问"))?;
        if !workspace.starts_with(controlled_root) {
            return Err(ApiError::validation("环境工作区不在受控根目录内"));
        }
        let root = workspace
            .to_str()
            .ok_or_else(|| ApiError::validation("环境工作区路径无效"))?;
        let log = git_output(root, &["log", "-20", "--format=%H%x00%s"]).await?;
        for line in log.lines() {
            let mut fields = line.splitn(2, '\0');
            let sha = fields.next().unwrap_or_default();
            let summary = fields.next().unwrap_or_default().trim();
            if sha.len() == 40 && !summary.is_empty() {
                commits.push(DevRailRepositoryCommitResponse {
                    sha: sha.to_owned(),
                    summary: summary.chars().take(200).collect(),
                });
            }
        }
    }
    Ok(DevRailRepositorySyncResponse {
        repository_id,
        status: "synced".to_string(),
        default_branch: repository.default_branch,
        branches,
        commits,
        synced_at: chrono::Utc::now(),
    })
}

pub async fn inspect_repository_worktree(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    repository_id: i64,
    environment_id: i64,
    controlled_workspace_root: &Path,
) -> Result<DevRailWorktreeResponse, ApiError> {
    let repository = get_repository(pool, actor, project_id, repository_id).await?;
    let environment = get_environment(pool, actor, project_id, environment_id).await?;
    let controlled_root = tokio::fs::canonicalize(controlled_workspace_root)
        .await
        .map_err(ApiError::internal)?;
    let workspace = tokio::fs::canonicalize(environment.workspace_root.trim())
        .await
        .map_err(|_| ApiError::validation("环境工作区不存在或不可访问"))?;
    if !workspace.starts_with(controlled_root) {
        return Err(ApiError::validation("环境工作区不在受控根目录内"));
    }
    let root = workspace
        .to_str()
        .ok_or_else(|| ApiError::validation("环境工作区路径无效"))?;
    let origin = git_output(root, &["remote", "get-url", "origin"]).await?;
    if origin.trim() != repository.remote_url.trim() {
        return Err(ApiError::validation("工作区远端与仓库配置不一致"));
    }
    let status_output = git_output(root, &["status", "--porcelain=v1", "-b"]).await?;
    let mut lines = status_output.lines();
    let branch = lines.next().and_then(|line| {
        line.strip_prefix("## ")
            .and_then(|value| value.split("...").next())
            .map(str::to_owned)
    });
    let mut changed_files = Vec::new();
    for line in lines.take(200) {
        if line.len() < 3 {
            continue;
        }
        let status = line[..2].trim().to_owned();
        let path = line[3..].split(" -> ").last().unwrap_or(&line[3..]).trim();
        if !path.is_empty() && path.len() <= 512 {
            changed_files.push(DevRailWorktreeFileResponse {
                status,
                path: path.to_owned(),
            });
        }
    }
    let commit = git_output(root, &["log", "-1", "--format=%H%x00%s"]).await?;
    let mut parts = commit.trim_end().splitn(2, '\0');
    let head_sha = parts
        .next()
        .filter(|value| value.len() == 40)
        .map(str::to_owned);
    let commit_summary = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(200).collect());
    Ok(DevRailWorktreeResponse {
        repository_id,
        environment_id,
        status: if changed_files.is_empty() {
            "clean"
        } else {
            "dirty"
        }
        .to_string(),
        branch,
        head_sha,
        commit_summary,
        changed_files,
        checked_at: chrono::Utc::now(),
    })
}

#[cfg(test)]
mod repository_sync_tests {
    fn parse_remote_head(text: &str) -> (Option<String>, Option<String>) {
        let sha = text
            .lines()
            .find_map(|line| {
                line.split_whitespace()
                    .find(|value| value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit()))
            })
            .filter(|v| v.len() == 40 && v.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(str::to_owned);
        let branch = text
            .lines()
            .find_map(|line| {
                line.strip_prefix("ref: refs/heads/")?
                    .split_whitespace()
                    .next()
            })
            .filter(|v| v.len() <= 128 && !v.contains(".."))
            .map(str::to_owned);
        (sha, branch)
    }

    #[test]
    fn parses_remote_head_symref_without_persisting_output() {
        let output = format!("ref: refs/heads/main\tHEAD\n{}\tHEAD\n", "a".repeat(40));
        let (sha, branch) = parse_remote_head(&output);
        assert_eq!(
            sha.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(branch.as_deref(), Some("main"));
    }

    #[test]
    fn worktree_status_is_bounded_and_does_not_include_file_contents() {
        let line = " M src/main.rs";
        assert_eq!(line[..2].trim(), "M");
        assert_eq!(&line[3..], "src/main.rs");
    }
}

fn workspace(value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if !value.starts_with('/') || value.contains("..") || value.len() > 512 {
        return Err(ApiError::validation("工作区必须是受控根目录下的绝对路径"));
    }
    Ok(value.to_string())
}
fn network(value: &str) -> Result<String, ApiError> {
    if ["off", "allowlist"].contains(&value) {
        Ok(value.to_string())
    } else {
        Err(ApiError::validation("网络模式只能是 off 或 allowlist"))
    }
}
fn duration(value: i64) -> Result<i64, ApiError> {
    if (60..=86400).contains(&value) {
        Ok(value)
    } else {
        Err(ApiError::validation("最大运行时长必须在 60-86400 秒之间"))
    }
}
fn environment_health(
    enabled: bool,
    metadata: Option<&std::fs::Metadata>,
) -> (bool, bool, bool, bool, String) {
    let workspace_exists = metadata.is_some();
    let workspace_is_directory = metadata.as_ref().is_some_and(|value| value.is_dir());
    let workspace_writable = metadata
        .as_ref()
        .is_some_and(|value| !value.permissions().readonly());
    let healthy = enabled && workspace_exists && workspace_is_directory && workspace_writable;
    let message = if !enabled {
        "环境已禁用"
    } else if !workspace_exists {
        "工作区不存在"
    } else if !workspace_is_directory {
        "工作区不是目录"
    } else if !workspace_writable {
        "工作区不可写"
    } else {
        "环境健康"
    };
    (
        healthy,
        workspace_exists,
        workspace_is_directory,
        workspace_writable,
        message.to_string(),
    )
}
pub async fn list_environments(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
) -> Result<DevRailEnvironmentPage, ApiError> {
    let project_id = q
        .project_id
        .ok_or_else(|| ApiError::validation("缺少 projectId"))?;
    let (page, size) = paging(q)?;
    let q2 = DevRailListQuery {
        project_id: Some(project_id),
        ..(*q).clone()
    };
    let (rows, total) = tokio::try_join!(
        devrail::list_environments(pool, actor, &q2, page, size),
        devrail::count_environments(pool, actor, &q2)
    )
    .map_err(db_error)?;
    Ok(DevRailEnvironmentPage {
        items: rows.into_iter().map(environment_response).collect(),
        total,
        page,
        page_size: size,
    })
}
pub async fn get_environment(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
) -> Result<DevRailEnvironmentResponse, ApiError> {
    devrail::find_environment(pool, actor, project_id, id)
        .await
        .map_err(db_error)?
        .map(environment_response)
        .ok_or_else(|| ApiError::not_found("环境不存在或超出数据范围"))
}
pub async fn create_environment(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    req: &CreateDevRailEnvironmentRequest,
) -> Result<DevRailEnvironmentResponse, ApiError> {
    get_project(pool, actor, project_id).await?;
    let name = text(&req.name, "环境名称", 128)?;
    let root = workspace(&req.workspace_root)?;
    let network_mode = network(req.network_mode.as_deref().unwrap_or("off"))?;
    let max = duration(req.max_duration_secs.unwrap_or(3600))?;
    let refs = json!(req.secret_ref_names.clone().unwrap_or_default());
    let department_id = scope_department(actor, req.department_id)?;
    let policy = req.tool_policy.clone().unwrap_or_else(|| json!({}));
    let mut tx = pool.begin().await.map_err(db_error)?;
    let row = devrail::create_environment(
        &mut tx,
        actor,
        &devrail::NewEnvironment {
            project_id,
            name: &name,
            workspace_root: &root,
            network_mode: &network_mode,
            tool_policy: &policy,
            secret_refs: &refs,
            max_duration_secs: max,
            enabled: req.enabled.unwrap_or(true),
            department_id,
        },
    )
    .await
    .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.environment.create",
        "devrail_environment",
        Some(row.id),
        json!({"projectId":project_id,"name":name}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(environment_response(row))
}
pub async fn update_environment(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
    req: &UpdateDevRailEnvironmentRequest,
) -> Result<DevRailEnvironmentResponse, ApiError> {
    let name = req
        .name
        .as_deref()
        .map(|v| text(v, "环境名称", 128))
        .transpose()?;
    let root = req.workspace_root.as_deref().map(workspace).transpose()?;
    let network_mode = req.network_mode.as_deref().map(network).transpose()?;
    let max = req.max_duration_secs.map(duration).transpose()?;
    let refs = req.secret_ref_names.as_ref().map(|v| json!(v));
    if name.is_none()
        && root.is_none()
        && network_mode.is_none()
        && req.tool_policy.is_none()
        && refs.is_none()
        && max.is_none()
        && req.enabled.is_none()
    {
        return Err(ApiError::validation("至少需要提供一个待更新字段"));
    }
    let mut tx = pool.begin().await.map_err(db_error)?;
    if !devrail::update_environment(
        &mut tx,
        actor,
        project_id,
        id,
        &devrail::EnvironmentUpdate {
            name: name.as_deref(),
            workspace_root: root.as_deref(),
            network_mode: network_mode.as_deref(),
            tool_policy: req.tool_policy.as_ref(),
            secret_refs: refs.as_ref(),
            max_duration_secs: max,
            enabled: req.enabled,
        },
    )
    .await
    .map_err(db_error)?
    {
        return Err(ApiError::not_found("环境不存在或超出数据范围"));
    }
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.environment.update",
        "devrail_environment",
        Some(id),
        json!({"projectId":project_id}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    get_environment(pool, actor, project_id, id).await
}

pub async fn health_check_environment(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
) -> Result<DevRailEnvironmentHealthResponse, ApiError> {
    let environment = get_environment(pool, actor, project_id, id).await?;
    let metadata = tokio::fs::metadata(&environment.workspace_root).await.ok();
    let (healthy, workspace_exists, workspace_is_directory, workspace_writable, message) =
        environment_health(environment.enabled, metadata.as_ref());
    let response = DevRailEnvironmentHealthResponse {
        environment_id: id,
        status: if healthy { "healthy" } else { "unhealthy" }.to_string(),
        enabled: environment.enabled,
        workspace_exists,
        workspace_is_directory,
        workspace_writable,
        message,
    };
    let mut tx = pool.begin().await.map_err(db_error)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.environment.health_check",
        "devrail_environment",
        Some(id),
        json!({
            "projectId": project_id,
            "status": response.status,
            "workspaceExists": response.workspace_exists,
            "workspaceIsDirectory": response.workspace_is_directory,
            "workspaceWritable": response.workspace_writable,
        }),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(response)
}

fn priority(value: &str) -> Result<String, ApiError> {
    if ["low", "normal", "high", "urgent"].contains(&value) {
        Ok(value.to_string())
    } else {
        Err(ApiError::validation("任务优先级无效"))
    }
}
fn task_status(value: &str) -> Result<String, ApiError> {
    if [
        "draft",
        "queued",
        "running",
        "awaiting_approval",
        "continuation_pending",
        "succeeded",
        "failed",
        "cancelled",
        "skipped",
        "archived",
    ]
    .contains(&value)
    {
        Ok(value.to_string())
    } else {
        Err(ApiError::validation("任务状态无效"))
    }
}
pub async fn list_tasks(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
) -> Result<DevRailTaskPage, ApiError> {
    let project_id = q
        .project_id
        .ok_or_else(|| ApiError::validation("缺少 projectId"))?;
    let (page, size) = paging(q)?;
    let q2 = DevRailListQuery {
        project_id: Some(project_id),
        ..(*q).clone()
    };
    let (rows, total) = tokio::try_join!(
        devrail::list_tasks(pool, actor, &q2, page, size),
        devrail::count_tasks(pool, actor, &q2)
    )
    .map_err(db_error)?;
    Ok(DevRailTaskPage {
        items: rows
            .into_iter()
            .map(|row| task_response(row, actor))
            .collect(),
        total,
        page,
        page_size: size,
    })
}
pub async fn get_task(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
) -> Result<DevRailTaskResponse, ApiError> {
    let row = devrail::find_task(pool, actor, project_id, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("任务不存在或超出数据范围"))?;
    let relations = task_relations(pool, actor, id, row.revision).await?;
    let mut response = task_response(row, actor);
    response.blocked_reason = relations.blocked_reason;
    response.prerequisites = relations.prerequisites;
    response.dependents = relations.dependents;
    Ok(response)
}

pub async fn get_task_relations(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    task_id: i64,
) -> Result<DevRailTaskRelationsResponse, ApiError> {
    let task = devrail::find_task(pool, actor, project_id, task_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("任务不存在或超出数据范围"))?;
    task_relations(pool, actor, task_id, task.revision).await
}

pub async fn get_task_relations_by_id(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: i64,
) -> Result<DevRailTaskRelationsResponse, ApiError> {
    let task = devrail::find_task_by_id(pool, actor, task_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("任务不存在或超出数据范围"))?;
    task_relations(pool, actor, task_id, task.revision).await
}

fn dependency_error(error: sqlx::Error) -> ApiError {
    match error {
        sqlx::Error::RowNotFound => ApiError::not_found("任务不存在或超出数据范围"),
        sqlx::Error::Protocol(message)
            if message.contains("形成环")
                || message.contains("幂等键")
                || message.contains("版本已变化") =>
        {
            let outcome = if message.contains("形成环") {
                "cycle"
            } else if message.contains("幂等键") {
                "idempotency"
            } else {
                "revision"
            };
            crate::app_metrics::record_dependency_conflict(outcome);
            ApiError::conflict(message)
        }
        other => db_error(other),
    }
}

fn dependency_action(action: Option<&DevRailDependencyAction>) -> &'static str {
    match action {
        Some(DevRailDependencyAction::Skip) => "skip",
        Some(DevRailDependencyAction::Fail) => "fail",
        Some(DevRailDependencyAction::Wait) | None => "wait",
    }
}

fn validate_idempotency_key(value: &str) -> Result<&str, ApiError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(ApiError::validation("幂等键格式无效"));
    }
    Ok(value)
}

pub async fn replace_task_dependencies(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    task_id: i64,
    request: &ReplaceDevRailTaskDependenciesRequest,
) -> Result<DevRailTaskRelationsResponse, ApiError> {
    if !actor.has_permission("devrail:task_dependency:write") {
        return Err(ApiError::forbidden("缺少管理任务依赖权限"));
    }
    let task = devrail::find_task(pool, actor, project_id, task_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("任务不存在或超出数据范围"))?;
    // A replay of an already committed mutation must remain idempotent even
    // when the task reached a terminal state between attempts. New mutations
    // still require draft/queued status and the repository revision check.
    if !matches!(task.status.as_str(), "draft" | "queued") && request.revision >= task.revision {
        return Err(ApiError::conflict("只有草稿或排队任务可以修改依赖"));
    }
    if request.revision <= 0 || request.dependencies.len() > 32 {
        return Err(ApiError::validation("任务版本或依赖数量无效"));
    }
    let idempotency_key = validate_idempotency_key(&request.idempotency_key)?;
    let mut dependencies = request.dependencies.clone();
    dependencies.sort_by_key(|dependency| dependency.prerequisite_task_id);
    if dependencies
        .iter()
        .any(|dependency| dependency.prerequisite_task_id == task_id)
        || dependencies
            .windows(2)
            .any(|pair| pair[0].prerequisite_task_id == pair[1].prerequisite_task_id)
    {
        return Err(ApiError::validation("依赖不能指向自身或重复任务"));
    }
    let digest_value = serde_json::to_value(&dependencies)
        .map_err(|_| ApiError::internal("依赖请求序列化失败"))?;
    let request_digest = workflow::snapshot_digest(&digest_value)
        .map_err(|_| ApiError::internal("依赖请求摘要计算失败"))?;
    let created_by_type = match actor.actor_type {
        crate::access::ActorType::User => "user",
        crate::access::ActorType::System => "system",
    };
    let created_by_user_id =
        matches!(actor.actor_type, crate::access::ActorType::User).then_some(actor.user_id);
    let inputs = dependencies
        .iter()
        .map(|dependency| devrail::NewTaskDependency {
            task_id,
            prerequisite_task_id: dependency.prerequisite_task_id,
            failure_action: dependency_action(dependency.failure_action.as_ref()),
            cancelled_action: dependency_action(dependency.cancelled_action.as_ref()),
            timeout_action: dependency_action(dependency.timeout_action.as_ref()),
            creation_source: "manual",
            created_by_type,
            created_by_user_id,
        })
        .collect::<Vec<_>>();
    let mut tx = pool.begin().await.map_err(db_error)?;
    devrail::replace_task_dependencies(
        &mut tx,
        actor,
        task_id,
        request.revision,
        &devrail::DependencyMutation {
            idempotency_key,
            request_digest: &request_digest,
        },
        &inputs,
    )
    .await
    .map_err(dependency_error)?;
    let event_key = format!("dependency-mutation:{idempotency_key}");
    devrail::append_task_event(
        &mut tx,
        &task,
        "task.dependencies.changed",
        &event_key,
        &json!({"taskId":task_id,"dependencyCount":inputs.len()}),
        "任务依赖已更新",
    )
    .await
    .map_err(db_error)?;
    repositories::audit_logs::record_actor(
        &mut tx,
        actor,
        "devrail.task.dependencies.replace",
        "devrail_task",
        Some(task_id),
        json!({"projectId":project_id,"dependencyCount":inputs.len()}),
    )
    .await
    .map_err(db_error)?;
    repositories::devrail_notifications::outbox(
        &mut tx,
        actor.organization_id,
        "task.dependencies.changed",
        "devrail_task",
        Some(task_id),
        &json!({"taskId":task_id,"eventType":"task.dependencies.changed"}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    let refreshed = devrail::find_task(pool, actor, project_id, task_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("任务不存在或超出数据范围"))?;
    task_relations(pool, actor, task_id, refreshed.revision).await
}

pub async fn list_task_events(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: i64,
    after_cursor: i64,
    limit: i64,
) -> Result<DevRailTaskEventPage, ApiError> {
    devrail::find_task_by_id(pool, actor, task_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("任务不存在或超出数据范围"))?;
    let limit = limit.clamp(1, 200);
    let rows = devrail::list_task_events(pool, actor, task_id, after_cursor, limit + 1)
        .await
        .map_err(db_error)?;
    let next_cursor = (rows.len() as i64 > limit).then(|| rows[limit as usize - 1].cursor);
    let items = rows
        .into_iter()
        .take(limit as usize)
        .map(|row| DevRailTaskEventResponse {
            cursor: row.cursor,
            event_type: row.event_type,
            payload: row.payload,
            summary: row.summary,
            occurred_at: row.occurred_at,
        })
        .collect();
    Ok(DevRailTaskEventPage { items, next_cursor })
}

fn followup_error(error: sqlx::Error) -> ApiError {
    match error {
        sqlx::Error::Protocol(message) if message.contains("配额") => ApiError::rate_limited(60),
        sqlx::Error::Protocol(message)
            if message.contains("幂等键") || message.contains("正在处理") =>
        {
            ApiError::conflict(message)
        }
        sqlx::Error::RowNotFound => ApiError::not_found("来源 run 不存在或超出数据范围"),
        other => db_error(other),
    }
}

pub async fn create_followup_task(
    pool: &PgPool,
    actor: &ActorContext,
    source_run_id: i64,
    request: &CreateDevRailFollowupTaskRequest,
) -> Result<DevRailFollowupTaskResponse, ApiError> {
    if !matches!(actor.actor_type, crate::access::ActorType::System)
        || !actor.has_permission("devrail:followup:create")
    {
        return Err(ApiError::forbidden("缺少创建后续任务权限"));
    }
    let idempotency_key = validate_idempotency_key(&request.idempotency_key)?;
    let run = repositories::devrail_runs::find_run(pool, actor, source_run_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("来源 run 不存在或超出数据范围"))?;
    let completed_recently = run.completed_at.is_some_and(|completed_at| {
        Utc::now().signed_duration_since(completed_at) <= chrono::Duration::minutes(15)
    });
    if !matches!(
        run.status.as_str(),
        "starting" | "active" | "awaiting_approval"
    ) && !(run.status == "completed" && completed_recently)
    {
        return Err(ApiError::conflict("来源 run 当前不能创建后续任务"));
    }
    let source_task = devrail::find_task_by_id(pool, actor, run.task_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("来源任务不存在或超出数据范围"))?;
    if source_task.followup_depth >= 8 {
        return Err(ApiError::conflict("后续任务层级已达到上限"));
    }
    let title = text(&request.title, "任务标题", 200)?;
    let goal = text(&request.goal, "任务目标", 4000)?;
    let background = optional_text(request.background.as_deref(), "任务背景", 16_000)?;
    let acceptance_criteria =
        optional_text(request.acceptance_criteria.as_deref(), "验收标准", 16_000)?;
    let constraints = optional_text(request.constraints.as_deref(), "任务约束", 16_000)?;
    let priority = priority(request.priority.as_deref().unwrap_or("normal"))?;
    let labels = json!(request.labels.clone().unwrap_or_default());
    let mut requested_dependencies = request.dependencies.clone().unwrap_or_default();
    if requested_dependencies.len() >= 16 {
        return Err(ApiError::validation("后续任务依赖数量超出上限"));
    }
    if !requested_dependencies
        .iter()
        .any(|dependency| dependency.prerequisite_task_id == source_task.id)
    {
        requested_dependencies.push(DevRailTaskDependencyInput {
            prerequisite_task_id: source_task.id,
            failure_action: None,
            cancelled_action: None,
            timeout_action: None,
        });
    }
    requested_dependencies.sort_by_key(|dependency| dependency.prerequisite_task_id);
    if requested_dependencies
        .windows(2)
        .any(|pair| pair[0].prerequisite_task_id == pair[1].prerequisite_task_id)
    {
        return Err(ApiError::validation("后续任务依赖不能重复"));
    }
    for dependency in &requested_dependencies {
        let prerequisite = devrail::find_task_by_id(pool, actor, dependency.prerequisite_task_id)
            .await
            .map_err(db_error)?
            .ok_or_else(|| ApiError::not_found("前置任务不存在或超出数据范围"))?;
        if prerequisite.project_id != source_task.project_id {
            return Err(ApiError::not_found("前置任务不存在或超出数据范围"));
        }
    }
    let mut digest_request = request.clone();
    digest_request.dependencies = Some(requested_dependencies.clone());
    let request_digest = workflow::snapshot_digest(
        &serde_json::to_value(&digest_request)
            .map_err(|_| ApiError::internal("后续任务请求序列化失败"))?,
    )
    .map_err(|_| ApiError::internal("后续任务请求摘要计算失败"))?;
    let dependency_inputs = requested_dependencies
        .iter()
        .map(|dependency| devrail::NewTaskDependency {
            task_id: 0,
            prerequisite_task_id: dependency.prerequisite_task_id,
            failure_action: dependency_action(dependency.failure_action.as_ref()),
            cancelled_action: dependency_action(dependency.cancelled_action.as_ref()),
            timeout_action: dependency_action(dependency.timeout_action.as_ref()),
            creation_source: "agent_followup",
            created_by_type: "agent",
            created_by_user_id: None,
        })
        .collect::<Vec<_>>();
    let mut tx = pool.begin().await.map_err(db_error)?;
    let (request_id, task, replayed) = devrail::create_followup_task(
        &mut tx,
        actor,
        &devrail::NewFollowup {
            department_id: source_task.department_id,
            owner_user_id: source_task.owner_user_id,
            source_task_id: source_task.id,
            source_run_id,
            idempotency_key,
            request_digest: &request_digest,
            task: devrail::NewTask {
                owner_user_id: source_task.owner_user_id,
                project_id: source_task.project_id,
                repository_id: source_task.repository_id,
                environment_id: source_task.environment_id,
                assignee_user_id: None,
                title: &title,
                goal: &goal,
                background: background.as_deref(),
                acceptance_criteria: acceptance_criteria.as_deref(),
                constraints: constraints.as_deref(),
                priority: &priority,
                labels: &labels,
                due_at: request.due_at,
                department_id: source_task.department_id,
                creation_source: "agent_followup",
                source_task_id: Some(source_task.id),
                source_run_id: Some(source_run_id),
                followup_depth: source_task.followup_depth + 1,
            },
            dependencies: &dependency_inputs,
        },
        8,
    )
    .await
    .map_err(followup_error)?;
    if !replayed {
        let source_key = format!("followup:{request_id}:source");
        devrail::append_task_event(
            &mut tx,
            &source_task,
            "task.followup.created",
            &source_key,
            &json!({"sourceRunId":source_run_id,"resultTaskId":task.id}),
            "Agent 已创建后续任务",
        )
        .await
        .map_err(db_error)?;
        let result_key = format!("followup:{request_id}:result");
        devrail::append_task_event(
            &mut tx,
            &task,
            "task.created.from_followup",
            &result_key,
            &json!({"sourceTaskId":source_task.id,"sourceRunId":source_run_id}),
            "任务由 Agent 后续任务提议创建",
        )
        .await
        .map_err(db_error)?;
        repositories::audit_logs::record_actor(
            &mut tx,
            actor,
            "devrail.task.followup.create",
            "devrail_task",
            Some(task.id),
            json!({"sourceTaskId":source_task.id,"sourceRunId":source_run_id,"dependencyCount":dependency_inputs.len()}),
        )
        .await
        .map_err(db_error)?;
        repositories::devrail_notifications::outbox(
            &mut tx,
            actor.organization_id,
            "task.followup.created",
            "devrail_task",
            Some(task.id),
            &json!({"taskId":task.id,"eventType":"task.followup.created"}),
        )
        .await
        .map_err(db_error)?;
    }
    tx.commit().await.map_err(db_error)?;
    let relations = task_relations(pool, actor, task.id, task.revision).await?;
    let mut task_response = task_response(task, actor);
    task_response.blocked_reason = relations.blocked_reason;
    task_response.prerequisites = relations.prerequisites;
    task_response.dependents = relations.dependents;
    Ok(DevRailFollowupTaskResponse {
        request_id,
        source_task_id: source_task.id,
        source_run_id,
        task: task_response,
        replayed,
    })
}

pub async fn record_followup_rejection(
    pool: &PgPool,
    actor: &ActorContext,
    source_run_id: i64,
    reason: &'static str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    repositories::audit_logs::record_actor(
        &mut tx,
        actor,
        "devrail.task.followup.reject",
        "devrail_run",
        Some(source_run_id),
        json!({"reason": reason}),
    )
    .await?;
    tx.commit().await
}

async fn validate_task_resources(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    repository_id: Option<i64>,
    environment_id: Option<i64>,
) -> Result<(), ApiError> {
    if let Some(repository_id) = repository_id {
        if get_repository(pool, actor, project_id, repository_id)
            .await
            .is_err()
        {
            return Err(ApiError::validation(
                "任务仓库必须属于当前项目且在数据范围内",
            ));
        }
    }
    if let Some(environment_id) = environment_id {
        if get_environment(pool, actor, project_id, environment_id)
            .await
            .is_err()
        {
            return Err(ApiError::validation(
                "任务环境必须属于当前项目且在数据范围内",
            ));
        }
    }
    Ok(())
}

async fn validate_optional_user_scope(
    pool: &PgPool,
    actor: &ActorContext,
    user_id: Option<i64>,
    field_name: &str,
) -> Result<(), ApiError> {
    let Some(user_id) = user_id else {
        return Ok(());
    };
    if repositories::users::find_by_id_for_actor(pool, actor, user_id)
        .await
        .map_err(db_error)?
        .is_none()
    {
        return Err(ApiError::validation(format!(
            "{field_name}必须属于当前数据范围"
        )));
    }
    Ok(())
}

#[derive(Debug)]
struct QueueTaskDraft {
    project_id: i64,
    repository_id: Option<i64>,
    environment_id: i64,
    task_id: i64,
    revision: i64,
    title: String,
    goal: String,
    background: Option<String>,
    acceptance_criteria: Option<String>,
    constraints: Option<String>,
    labels: Value,
}

struct QueueTaskArtifacts {
    workflow: WorkflowSnapshot,
    dispatch_snapshot: Value,
    dispatch_snapshot_digest: String,
    department_id: Option<i64>,
    owner_user_id: i64,
}

async fn build_queue_task_artifacts(
    pool: &PgPool,
    actor: &ActorContext,
    controlled_workspace_root: &Path,
    draft: &QueueTaskDraft,
) -> Result<QueueTaskArtifacts, ApiError> {
    let environment = get_environment(pool, actor, draft.project_id, draft.environment_id).await?;
    if !environment.enabled {
        return Err(ApiError::conflict("运行环境已禁用"));
    }
    let repository = match draft.repository_id {
        Some(repository_id) => {
            Some(get_repository(pool, actor, draft.project_id, repository_id).await?)
        }
        None => None,
    };
    let mut platform_policy = PlatformWorkflowPolicy::secure_default(environment.max_duration_secs);
    platform_policy.network_allowed = environment.network_mode == "allowlist";
    if let Some(allowed_tools) = environment
        .tool_policy
        .get("allowedTools")
        .and_then(Value::as_array)
    {
        let environment_tools = allowed_tools
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<std::collections::BTreeSet<_>>();
        platform_policy
            .allowed_tools
            .retain(|tool| environment_tools.contains(tool));
    }
    let workflow = match workflow::load_repository_workflow(
        controlled_workspace_root,
        Path::new(&environment.workspace_root),
        &platform_policy,
    )
    .await
    {
        Ok(workflow) => workflow,
        Err(load_error) => {
            let Some(last_known_good) =
                devrail_workflows::last_known_good(pool, actor, draft.environment_id)
                    .await
                    .map_err(db_error)?
            else {
                return Err(ApiError::validation(load_error.to_string()));
            };
            let restored: WorkflowSnapshot =
                serde_json::from_value(last_known_good.normalized_snapshot).map_err(|_| {
                    ApiError::conflict("持久化 workflow 快照无效，无法安全创建任务快照")
                })?;
            if restored.digest != last_known_good.digest
                || restored.source.as_str() != last_known_good.source
                || restored.declared_version != last_known_good.declared_version
            {
                return Err(ApiError::conflict(
                    "持久化 workflow 身份不一致，无法安全创建任务快照",
                ));
            }
            restored
        }
    };
    let rendered_workflow = workflow::render_workflow(
        &workflow,
        &WorkflowTaskContext {
            task: WorkflowTaskTemplateContext {
                id: draft.task_id,
                title: &draft.title,
                goal: &draft.goal,
                background: draft.background.as_deref(),
                acceptance_criteria: draft.acceptance_criteria.as_deref(),
                constraints: draft.constraints.as_deref(),
            },
            repository: WorkflowRepositoryTemplateContext {
                name: repository.as_ref().map(|value| value.name.as_str()),
                default_branch: repository
                    .as_ref()
                    .map(|value| value.default_branch.as_str()),
            },
            environment: WorkflowEnvironmentTemplateContext {
                name: &environment.name,
                workspace_root: &environment.workspace_root,
            },
        },
    )
    .map_err(|error| ApiError::validation(error.to_string()))?;
    let task_snapshot = json!({
        "taskId": draft.task_id,
        "projectId": draft.project_id,
        "repositoryId": draft.repository_id,
        "environmentId": draft.environment_id,
        "title": draft.title,
        "goal": draft.goal,
        "background": draft.background,
        "acceptanceCriteria": draft.acceptance_criteria,
        "constraints": draft.constraints,
        "labels": draft.labels,
        "workspaceRoot": environment.workspace_root,
        "networkMode": environment.network_mode,
        "toolPolicy": environment.tool_policy,
    });
    let dispatch_snapshot =
        workflow::task_dispatch_snapshot(&task_snapshot, draft.revision, &rendered_workflow);
    let dispatch_snapshot_digest = workflow::snapshot_digest(&dispatch_snapshot)
        .map_err(|error| ApiError::validation(error.to_string()))?;
    Ok(QueueTaskArtifacts {
        workflow,
        dispatch_snapshot,
        dispatch_snapshot_digest,
        department_id: environment.department_id,
        owner_user_id: environment.owner_user_id,
    })
}

fn task_transition_allowed(current: &str, next: &str) -> bool {
    current == next
        || matches!(
            (current, next),
            ("draft", "queued" | "cancelled" | "archived")
                | ("queued", "cancelled" | "failed")
                | (
                    "running",
                    "awaiting_approval" | "succeeded" | "failed" | "cancelled"
                )
                | (
                    "awaiting_approval",
                    "running" | "succeeded" | "failed" | "cancelled"
                )
                | ("succeeded" | "failed" | "cancelled", "archived")
                | ("skipped", "archived")
                | ("failed", "queued")
        )
}

pub async fn create_task(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    req: &CreateDevRailTaskRequest,
) -> Result<DevRailTaskResponse, ApiError> {
    get_project(pool, actor, project_id).await?;
    let title = text(&req.title, "任务标题", 200)?;
    let goal = text(&req.goal, "任务目标", 4000)?;
    let priority = priority(req.priority.as_deref().unwrap_or("normal"))?;
    let labels = json!(req.labels.clone().unwrap_or_default());
    let department_id = scope_department(actor, req.department_id)?;
    validate_task_resources(
        pool,
        actor,
        project_id,
        req.repository_id,
        req.environment_id,
    )
    .await?;
    validate_optional_user_scope(pool, actor, req.assignee_user_id, "任务负责人").await?;
    let mut tx = pool.begin().await.map_err(db_error)?;
    if let Some(assignee_user_id) = req.assignee_user_id {
        if repositories::users::find_by_id_for_actor_in_connection(&mut tx, actor, assignee_user_id)
            .await
            .map_err(db_error)?
            .is_none()
        {
            return Err(ApiError::validation("任务负责人必须属于当前数据范围"));
        }
    }
    let row = devrail::create_task(
        &mut tx,
        actor,
        &devrail::NewTask {
            owner_user_id: actor.user_id,
            project_id,
            repository_id: req.repository_id,
            environment_id: req.environment_id,
            assignee_user_id: req.assignee_user_id,
            title: &title,
            goal: &goal,
            background: req.background.as_deref(),
            acceptance_criteria: req.acceptance_criteria.as_deref(),
            constraints: req.constraints.as_deref(),
            priority: &priority,
            labels: &labels,
            due_at: req.due_at,
            department_id,
            creation_source: "manual",
            source_task_id: None,
            source_run_id: None,
            followup_depth: 0,
        },
    )
    .await
    .map_err(|error| match error {
        sqlx::Error::RowNotFound => ApiError::validation("任务负责人必须属于当前数据范围"),
        other => db_error(other),
    })?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.task.create",
        "devrail_task",
        Some(row.id),
        json!({"projectId":project_id,"title":title,"repositoryId":req.repository_id,"environmentId":req.environment_id}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(task_response(row, actor))
}
pub async fn update_task(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
    req: &UpdateDevRailTaskRequest,
    controlled_workspace_root: &Path,
) -> Result<DevRailTaskResponse, ApiError> {
    let title = req
        .title
        .as_deref()
        .map(|v| text(v, "任务标题", 200))
        .transpose()?;
    let goal = req
        .goal
        .as_deref()
        .map(|v| text(v, "任务目标", 4000))
        .transpose()?;
    let priority = req.priority.as_deref().map(priority).transpose()?;
    let status = req.status.as_deref().map(task_status).transpose()?;
    let (background_set, background) = nullable_patch(&req.background);
    let (acceptance_set, acceptance_criteria) = nullable_patch(&req.acceptance_criteria);
    let (constraints_set, constraints) = nullable_patch(&req.constraints);
    let (assignee_set, assignee_user_id) = nullable_patch(&req.assignee_user_id);
    let (due_at_set, due_at) = nullable_patch(&req.due_at);
    let (repository_set, repository_id) = nullable_patch(&req.repository_id);
    let (environment_set, environment_id) = nullable_patch(&req.environment_id);
    if title.is_none()
        && goal.is_none()
        && !background_set
        && !acceptance_set
        && !constraints_set
        && priority.is_none()
        && status.is_none()
        && !assignee_set
        && req.labels.is_none()
        && !due_at_set
        && !repository_set
        && !environment_set
    {
        return Err(ApiError::validation("至少需要提供一个待更新字段"));
    }
    let labels = req.labels.as_ref().map(|v| json!(v));
    let current = get_task(pool, actor, project_id, id).await?;
    if let Some(next_status) = status.as_deref() {
        if !task_transition_allowed(&current.status, next_status) {
            return Err(ApiError::conflict(format!(
                "任务不能从 {} 转换为 {}",
                current.status, next_status
            )));
        }
    }
    let next_repository_id = if repository_set {
        repository_id
    } else {
        current.repository_id
    };
    let next_environment_id = if environment_set {
        environment_id
    } else {
        current.environment_id
    };
    validate_task_resources(
        pool,
        actor,
        project_id,
        next_repository_id,
        next_environment_id,
    )
    .await?;
    let next_assignee_user_id = if assignee_set {
        assignee_user_id
    } else {
        current.assignee_user_id
    };
    validate_optional_user_scope(pool, actor, next_assignee_user_id, "任务负责人").await?;
    let next_title = title.clone().unwrap_or_else(|| current.title.clone());
    let next_goal = goal.clone().unwrap_or_else(|| current.goal.clone());
    let next_background = if background_set {
        background.clone()
    } else {
        current.background.clone()
    };
    let next_acceptance_criteria = if acceptance_set {
        acceptance_criteria.clone()
    } else {
        current.acceptance_criteria.clone()
    };
    let next_constraints = if constraints_set {
        constraints.clone()
    } else {
        current.constraints.clone()
    };
    let dispatch_input_changed = next_title != current.title
        || next_goal != current.goal
        || next_background != current.background
        || next_acceptance_criteria != current.acceptance_criteria
        || next_constraints != current.constraints
        || next_repository_id != current.repository_id
        || next_environment_id != current.environment_id;
    if matches!(
        current.status.as_str(),
        "queued" | "running" | "awaiting_approval"
    ) && dispatch_input_changed
    {
        return Err(ApiError::conflict(
            "已排队或运行中的任务输入不可原地修改；请取消后重建任务",
        ));
    }
    let queue_artifacts = if status.as_deref() == Some("queued") && current.status != "queued" {
        let environment_id =
            next_environment_id.ok_or_else(|| ApiError::validation("排队任务必须关联运行环境"))?;
        Some(
            build_queue_task_artifacts(
                pool,
                actor,
                controlled_workspace_root,
                &QueueTaskDraft {
                    project_id,
                    repository_id: next_repository_id,
                    environment_id,
                    task_id: id,
                    revision: current.revision + i64::from(dispatch_input_changed),
                    title: next_title,
                    goal: next_goal,
                    background: next_background,
                    acceptance_criteria: next_acceptance_criteria,
                    constraints: next_constraints,
                    labels: labels.clone().unwrap_or_else(|| current.labels.clone()),
                },
            )
            .await?,
        )
    } else {
        None
    };
    let mut tx = pool.begin().await.map_err(db_error)?;
    if let Some(assignee_user_id) = next_assignee_user_id {
        if repositories::users::find_by_id_for_actor_in_connection(&mut tx, actor, assignee_user_id)
            .await
            .map_err(db_error)?
            .is_none()
        {
            return Err(ApiError::validation("任务负责人必须属于当前数据范围"));
        }
    }
    if let Some(artifacts) = queue_artifacts.as_ref() {
        let queue_environment_id =
            next_environment_id.ok_or_else(|| ApiError::validation("排队任务必须关联运行环境"))?;
        let normalized_snapshot = serde_json::to_value(&artifacts.workflow)
            .map_err(|_| ApiError::internal("workflow 快照序列化失败"))?;
        devrail_workflows::accept_version(
            &mut tx,
            &devrail_workflows::NewWorkflowVersion {
                organization_id: actor.organization_id,
                department_id: artifacts.department_id,
                owner_user_id: artifacts.owner_user_id,
                environment_id: queue_environment_id,
                source: artifacts.workflow.source.as_str(),
                declared_version: &artifacts.workflow.declared_version,
                digest: &artifacts.workflow.digest,
                normalized_snapshot: &normalized_snapshot,
                prompt_body: &artifacts.workflow.prompt_template,
            },
        )
        .await
        .map_err(db_error)?;
    }
    if !devrail::update_task(
        &mut tx,
        actor,
        project_id,
        id,
        &devrail::TaskUpdate {
            title: title.as_deref(),
            goal: goal.as_deref(),
            background_set,
            background: background.as_deref(),
            acceptance_set,
            acceptance_criteria: acceptance_criteria.as_deref(),
            constraints_set,
            constraints: constraints.as_deref(),
            priority: priority.as_deref(),
            status: status.as_deref(),
            assignee_set,
            assignee_user_id,
            labels: labels.as_ref(),
            due_at_set,
            due_at,
            repository_set,
            repository_id,
            environment_set,
            environment_id,
            queue_snapshot: queue_artifacts
                .as_ref()
                .map(|artifacts| &artifacts.dispatch_snapshot),
            queue_snapshot_digest: queue_artifacts
                .as_ref()
                .map(|artifacts| artifacts.dispatch_snapshot_digest.as_str()),
            workflow_source: queue_artifacts
                .as_ref()
                .map(|artifacts| artifacts.workflow.source.as_str()),
            workflow_version: queue_artifacts
                .as_ref()
                .map(|artifacts| artifacts.workflow.declared_version.as_str()),
            workflow_digest: queue_artifacts
                .as_ref()
                .map(|artifacts| artifacts.workflow.digest.as_str()),
            queue_max_attempts: queue_artifacts
                .as_ref()
                .map(|artifacts| artifacts.workflow.config.retry.max_attempts),
        },
    )
    .await
    .map_err(db_error)?
    {
        return Err(ApiError::not_found("任务不存在或超出数据范围"));
    }
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.task.update",
        "devrail_task",
        Some(id),
        json!({"projectId":project_id,"repositoryId":next_repository_id,"environmentId":next_environment_id,"workflowDigest":queue_artifacts.as_ref().map(|artifacts| artifacts.workflow.digest.as_str())}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    get_task(pool, actor, project_id, id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_project_slugs_and_remote_credentials() {
        assert!(slug("devrail-core").is_ok());
        assert!(slug("DevRail").is_err());
        assert_eq!(remote("git@example.com:team/repo.git").unwrap().1, "ssh");
        let credential_url = [
            "https",
            "://",
            "user",
            ":",
            "credential",
            "@example.com/repo.git",
        ]
        .concat();
        assert!(remote(&credential_url).is_err());
    }

    #[test]
    fn rejects_uncontrolled_workspace_and_invalid_limits() {
        assert!(workspace("/srv/devrail/workspaces/project").is_ok());
        assert!(workspace("relative/path").is_err());
        assert!(workspace("/srv/../etc").is_err());
        assert!(duration(3600).is_ok());
        assert!(duration(30).is_err());
    }

    #[test]
    fn reports_environment_health_for_disabled_and_missing_workspaces() {
        let disabled = environment_health(false, None);
        assert!(!disabled.0);
        assert_eq!(disabled.4, "环境已禁用");
        let missing = environment_health(true, None);
        assert!(!missing.0);
        assert_eq!(missing.4, "工作区不存在");
    }

    #[test]
    fn validates_dependency_idempotency_keys_and_actions() {
        assert_eq!(validate_idempotency_key("  edge-42 ").unwrap(), "edge-42");
        assert!(validate_idempotency_key(" ").is_err());
        assert!(validate_idempotency_key("edge key").is_err());
        assert_eq!(dependency_action(None), "wait");
        assert_eq!(
            dependency_action(Some(&DevRailDependencyAction::Skip)),
            "skip"
        );
        assert_eq!(
            dependency_action(Some(&DevRailDependencyAction::Fail)),
            "fail"
        );
    }

    #[test]
    fn dependency_block_reason_is_safe_and_deterministic() {
        let dependency = DevRailTaskDependencyResponse {
            id: 1,
            task_id: 2,
            prerequisite_task_id: 3,
            prerequisite_title: "前置任务".to_owned(),
            prerequisite_status: "failed".to_owned(),
            failure_action: "fail".to_owned(),
            cancelled_action: "wait".to_owned(),
            timeout_action: "wait".to_owned(),
            creation_source: "manual".to_owned(),
            created_at: Utc::now(),
        };
        assert_eq!(
            dependency_block_reason(&[dependency]).as_deref(),
            Some("前置任务 前置任务 已失败")
        );
    }
}
