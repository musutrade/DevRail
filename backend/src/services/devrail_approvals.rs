use crate::access::ActorContext;
use crate::error::ApiError;
use crate::models::*;
use crate::repositories::{self, devrail_approvals, devrail_runs};
use crate::workers::harness_supervisor::HarnessSupervisor;
use chrono::{Duration, Utc};
use serde_json::json;
use sqlx::PgPool;

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
    tx.commit().await.map_err(db_error)?;
    if decision == "approved" {
        supervisor
            .resolve_approval(approval.run_id, id, true)
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
pub async fn reject(
    pool: &PgPool,
    actor: &ActorContext,
    supervisor: &HarnessSupervisor,
    id: i64,
    reason: Option<&str>,
) -> Result<DevRailApprovalResponse, ApiError> {
    let result = decide(pool, actor, supervisor, id, "rejected", reason).await?;
    supervisor
        .resolve_approval(result.run_id, id, false)
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
    tx.commit().await.map_err(db_error)?;
    supervisor
        .resolve_approval(approval.run_id, id, false)
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
        tx.commit().await?;
        let _ = supervisor.resolve_approval(row.run_id, row.id, false).await;
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
