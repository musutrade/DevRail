use crate::access::ActorContext;
use crate::error::ApiError;
use crate::models::*;
use crate::repositories::{self, devrail_approvals, devrail_runs};
use crate::workers::harness_supervisor::HarnessSupervisor;
use chrono::{Duration, Utc};
use serde_json::json;
use sqlx::PgPool;

fn notification_copy(status: &str) -> (&'static str, &'static str, &'static str) {
    match status {
        "approved" => ("success", "审批已批准", "devrail.approval.approved"),
        "rejected" => ("error", "审批已拒绝", "devrail.approval.rejected"),
        "cancelled" => ("warning", "审批已撤回", "devrail.approval.cancelled"),
        "expired" => ("warning", "审批已过期", "devrail.approval.expired"),
        _ => ("info", "需要处理审批", "devrail.approval.requested"),
    }
}

async fn add_notification(
    tx: &mut sqlx::PgConnection,
    approval: &DevRailApprovalRow,
    status: &str,
    summary: &str,
) -> Result<(), sqlx::Error> {
    let (level, title, event_type) = notification_copy(status);
    let source_key = format!("approval:{}:{}", approval.id, status);
    let deep_link = format!("/devrail/approvals/{}", approval.id);
    repositories::devrail_notifications::create(
        tx,
        &repositories::devrail_notifications::NewNotification {
            organization_id: approval.organization_id,
            department_id: approval.department_id,
            recipient_user_id: approval.requested_by,
            event_type,
            level,
            title,
            summary,
            resource_type: Some("devrail_approval"),
            resource_id: Some(approval.id),
            deep_link: Some(&deep_link),
            source_key: &source_key,
        },
    )
    .await?;
    repositories::devrail_notifications::outbox(
        tx,
        approval.organization_id,
        "notification.created",
        &format!("devrail_approval:{status}"),
        Some(approval.id),
        &json!({"notificationSource": source_key, "eventType": event_type}),
    )
    .await
}

fn db_error(error: sqlx::Error) -> ApiError {
    ApiError::internal(error)
}
fn response(row: DevRailApprovalRow) -> DevRailApprovalResponse {
    DevRailApprovalResponse {
        id: row.id,
        run_id: row.run_id,
        event_id: row.event_id,
        idempotency_key: row.idempotency_key,
        tool_name: row.tool_name,
        args_summary: row.args_summary,
        cwd: row.cwd,
        impact_scope: row.impact_scope,
        risk_level: row.risk_level,
        requested_by: row.requested_by,
        decided_by: row.decided_by,
        status: row.status,
        decision_reason: row.decision_reason,
        expires_at: row.expires_at,
        policy_version: row.policy_version,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

pub struct HarnessApprovalRequest {
    pub run_id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub tool_name: String,
    pub args_summary: serde_json::Value,
    pub cwd: String,
    pub risk_level: String,
    pub idempotency_key: String,
}

pub async fn request_from_harness(
    pool: &PgPool,
    request: HarnessApprovalRequest,
) -> Result<i64, sqlx::Error> {
    let task_id = devrail_runs::task_id_for_run(pool, request.run_id).await?;
    let expires_at = Utc::now() + Duration::minutes(15);
    let mut tx = pool.begin().await?;
    let policy_version = devrail_runs::policy_version_for_run(pool, request.run_id)
        .await?
        .or_else(|| Some("devrail-policy-v1".to_string()));
    let row = devrail_approvals::create_pending(
        &mut tx,
        &devrail_approvals::NewApproval {
            organization_id: request.organization_id,
            department_id: request.department_id,
            owner_user_id: request.owner_user_id,
            run_id: request.run_id,
            event_id: None,
            idempotency_key: &request.idempotency_key,
            tool_name: &request.tool_name,
            args_summary: &request.args_summary,
            cwd: &request.cwd,
            impact_scope: None,
            risk_level: &request.risk_level,
            requested_by: request.owner_user_id,
            expires_at,
            policy_version: policy_version.as_deref(),
        },
    )
    .await?;
    devrail_approvals::mark_waiting(&mut tx, request.run_id, task_id).await?;
    repositories::audit_logs::record(
        &mut tx,
        Some(request.owner_user_id),
        "devrail.approval.request",
        "devrail_approval",
        Some(row.id),
        json!({"runId":request.run_id,"tool":request.tool_name,"riskLevel":request.risk_level}),
    )
    .await?;
    add_notification(
        &mut tx,
        &row,
        "requested",
        "Agent 请求执行高风险工具，请及时处理审批。",
    )
    .await?;
    tx.commit().await?;
    Ok(row.id)
}

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    page: i64,
    size: i64,
) -> Result<DevRailApprovalPage, ApiError> {
    let (items, total) = tokio::try_join!(
        devrail_approvals::list(pool, actor, page, size),
        devrail_approvals::count(pool, actor)
    )
    .map_err(db_error)?;
    Ok(DevRailApprovalPage {
        items: items.into_iter().map(response).collect(),
        total,
        page,
        page_size: size,
    })
}
pub async fn get(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<DevRailApprovalResponse, ApiError> {
    devrail_approvals::find(pool, actor, id)
        .await
        .map_err(db_error)?
        .map(response)
        .ok_or_else(|| ApiError::not_found("审批不存在或超出数据范围"))
}

async fn decide(
    pool: &PgPool,
    actor: &ActorContext,
    supervisor: &HarnessSupervisor,
    id: i64,
    decision: &str,
    reason: Option<&str>,
) -> Result<DevRailApprovalResponse, ApiError> {
    let approval = devrail_approvals::find(pool, actor, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("审批不存在或超出数据范围"))?;
    if approval.status != "pending" {
        return Err(ApiError::conflict("审批已处理，不能重复决策"));
    }
    let current_policy = devrail_runs::policy_version_for_run(pool, approval.run_id)
        .await
        .map_err(db_error)?;
    if approval.policy_version != current_policy {
        return Err(ApiError::conflict("审批策略已更新，不能使用旧策略决策"));
    }
    let mut tx = pool.begin().await.map_err(db_error)?;
    let task_id = devrail_runs::task_id_for_run(pool, approval.run_id)
        .await
        .map_err(db_error)?;
    let row = devrail_approvals::decide(
        &mut tx,
        &devrail_approvals::ApprovalDecision {
            actor,
            id,
            decision,
            reason,
        },
    )
    .await
    .map_err(db_error)?
    .ok_or_else(|| ApiError::conflict("审批已过期或已被其他人处理"))?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        &format!("devrail.approval.{decision}"),
        "devrail_approval",
        Some(id),
        json!({"runId":approval.run_id,"reason":reason}),
    )
    .await
    .map_err(db_error)?;
    if decision == "approved" {
        devrail_approvals::mark_resumed(&mut tx, approval.run_id, task_id)
            .await
            .map_err(db_error)?;
    }
    if decision == "rejected" {
        let trace = uuid::Uuid::new_v4().to_string();
        devrail_runs::update_run_terminal(
            &mut tx,
            &devrail_runs::TerminalRunUpdate {
                run_id: approval.run_id,
                status: "failed",
                exit_reason: "approval_rejected",
                exit_code: None,
                stderr_summary: None,
                trace_id: &trace,
                recovery_suggestion: Some("审批被拒绝；可在任务详情中重新发起 run"),
            },
        )
        .await
        .map_err(db_error)?;
        devrail_runs::update_task_status(&mut tx, task_id, "failed")
            .await
            .map_err(db_error)?;
    }
    add_notification(
        &mut tx,
        &row,
        decision,
        reason.unwrap_or(match decision {
            "approved" => "审批已批准，运行将继续。",
            _ => "审批已拒绝，运行已停止。",
        }),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    if decision == "approved" {
        supervisor
            .resolve_approval(approval.run_id, id, &approval.idempotency_key, true)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?;
    }
    Ok(response(row))
}

pub async fn approve(
    pool: &PgPool,
    actor: &ActorContext,
    supervisor: &HarnessSupervisor,
    id: i64,
    reason: Option<&str>,
) -> Result<DevRailApprovalResponse, ApiError> {
    decide(pool, actor, supervisor, id, "approved", reason).await
}

pub async fn recover(
    pool: &PgPool,
    actor: &ActorContext,
    supervisor: &HarnessSupervisor,
    id: i64,
) -> Result<DevRailApprovalResponse, ApiError> {
    let approval = devrail_approvals::find(pool, actor, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("审批不存在或超出数据范围"))?;
    if approval.status != "pending" {
        return Err(ApiError::conflict("审批已处理，不能恢复"));
    }
    supervisor
        .recover_run(approval.run_id)
        .await
        .map_err(|error| ApiError::conflict(format!("运行无法恢复：{error}")))?;
    let mut tx = pool.begin().await.map_err(db_error)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.approval.recover",
        "devrail_approval",
        Some(id),
        json!({"runId": approval.run_id}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(response(approval))
}
pub async fn reject(
    pool: &PgPool,
    actor: &ActorContext,
    supervisor: &HarnessSupervisor,
    id: i64,
    reason: Option<&str>,
) -> Result<DevRailApprovalResponse, ApiError> {
    let result = decide(pool, actor, supervisor, id, "rejected", reason).await?;
    supervisor
        .resolve_approval(result.run_id, id, &result.idempotency_key, false)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(result)
}

pub async fn withdraw(
    pool: &PgPool,
    actor: &ActorContext,
    supervisor: &HarnessSupervisor,
    id: i64,
    reason: Option<&str>,
) -> Result<DevRailApprovalResponse, ApiError> {
    let approval = devrail_approvals::find(pool, actor, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("审批不存在或超出数据范围"))?;
    let mut tx = pool.begin().await.map_err(db_error)?;
    let row = devrail_approvals::withdraw(
        &mut tx,
        &devrail_approvals::ApprovalDecision {
            actor,
            id,
            decision: "cancelled",
            reason,
        },
    )
    .await
    .map_err(db_error)?
    .ok_or_else(|| ApiError::conflict("审批不是待处理状态或只能撤回自己的审批"))?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.approval.cancelled",
        "devrail_approval",
        Some(id),
        json!({"runId": approval.run_id, "reason": reason}),
    )
    .await
    .map_err(db_error)?;
    add_notification(
        &mut tx,
        &row,
        "cancelled",
        reason.unwrap_or("审批请求已撤回。"),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    supervisor
        .cancel_approval(approval.run_id, id, &approval.idempotency_key)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(response(row))
}

pub async fn expire_due(
    pool: &PgPool,
    supervisor: &HarnessSupervisor,
) -> Result<usize, sqlx::Error> {
    let rows = devrail_approvals::expire_due(pool).await?;
    for row in &rows {
        let task_id = devrail_runs::task_id_for_run(pool, row.run_id).await?;
        let mut tx = pool.begin().await?;
        let trace = uuid::Uuid::new_v4().to_string();
        devrail_runs::update_run_terminal(
            &mut tx,
            &devrail_runs::TerminalRunUpdate {
                run_id: row.run_id,
                status: "failed",
                exit_reason: "approval_expired",
                exit_code: None,
                stderr_summary: None,
                trace_id: &trace,
                recovery_suggestion: Some("审批已过期；请重新发起 run"),
            },
        )
        .await?;
        devrail_runs::update_task_status(&mut tx, task_id, "failed").await?;
        add_notification(
            &mut tx,
            row,
            "expired",
            "审批超过有效期，运行已停止；请重新发起 run。",
        )
        .await?;
        tx.commit().await?;
        let _ = supervisor
            .cancel_approval(row.run_id, row.id, &row.idempotency_key)
            .await;
    }
    Ok(rows.len())
}

pub async fn expire(
    pool: &PgPool,
    actor: &ActorContext,
    supervisor: &HarnessSupervisor,
    id: i64,
) -> Result<DevRailApprovalResponse, ApiError> {
    reject(pool, actor, supervisor, id, Some("审批已过期")).await
}

#[cfg(test)]
mod tests {
    use super::notification_copy;

    #[test]
    fn notification_copy_maps_all_approval_states() {
        assert_eq!(
            notification_copy("requested").2,
            "devrail.approval.requested"
        );
        assert_eq!(notification_copy("approved").0, "success");
        assert_eq!(notification_copy("rejected").0, "error");
        assert_eq!(notification_copy("cancelled").0, "warning");
        assert_eq!(notification_copy("expired").2, "devrail.approval.expired");
    }
}
