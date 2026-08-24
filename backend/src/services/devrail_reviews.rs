use crate::{
    access::ActorContext,
    error::{db_error, ApiError},
    models::*,
    repositories::{self, devrail_reviews},
};
use serde_json::json;
use sqlx::PgPool;
fn response(r: DevRailReviewRow) -> DevRailReviewResponse {
    DevRailReviewResponse {
        id: r.id,
        task_id: r.task_id,
        run_id: r.run_id,
        requested_by: r.requested_by,
        reviewer_user_id: r.reviewer_user_id,
        status: r.status,
        summary: r.summary,
        decision_reason: r.decision_reason,
        decided_at: r.decided_at,
        created_at: r.created_at,
    }
}
pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    page: i64,
    size: i64,
) -> Result<DevRailReviewPage, ApiError> {
    let (items, total) = devrail_reviews::list(pool, actor, page, size)
        .await
        .map_err(db_error)?;
    Ok(DevRailReviewPage {
        items: items.into_iter().map(response).collect(),
        total,
        page,
        page_size: size,
    })
}
pub async fn create(
    pool: &PgPool,
    actor: &ActorContext,
    req: &CreateDevRailReviewRequest,
) -> Result<DevRailReviewResponse, ApiError> {
    if req.reviewer_user_id == actor.user_id {
        return Err(ApiError::validation("审查人不能是任务发起人"));
    }
    let mut tx = pool.begin().await.map_err(db_error)?;
    let row = devrail_reviews::create(
        &mut tx,
        actor,
        req.run_id,
        req.reviewer_user_id,
        req.summary.as_deref(),
    )
    .await
    .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.review.create",
        "devrail_review",
        Some(row.id),
        json!({"runId":req.run_id,"reviewerUserId":req.reviewer_user_id}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(response(row))
}
pub async fn decide(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
    req: &DecideDevRailReviewRequest,
) -> Result<DevRailReviewResponse, ApiError> {
    if !matches!(req.decision.as_str(), "approved" | "rejected") {
        return Err(ApiError::validation("审查结论无效"));
    }
    let mut tx = pool.begin().await.map_err(db_error)?;
    let row = devrail_reviews::decide(&mut tx, actor, id, &req.decision, req.reason.as_deref())
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::conflict("审查不存在、无权处理或已完成"))?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        &format!("devrail.review.{}", req.decision),
        "devrail_review",
        Some(id),
        json!({"runId":row.run_id,"reason":req.reason}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(response(row))
}
