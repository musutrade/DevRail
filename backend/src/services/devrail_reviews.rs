use crate::{
    access::ActorContext,
    error::{db_error, ApiError},
    models::*,
    repositories::{self, devrail_review_comments, devrail_reviews},
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
fn comment_response(r: DevRailReviewCommentRow) -> DevRailReviewCommentResponse {
    DevRailReviewCommentResponse {
        id: r.id,
        review_id: r.review_id,
        author_user_id: r.author_user_id,
        file_path: r.file_path,
        line_start: r.line_start,
        line_end: r.line_end,
        body: r.body,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}
pub async fn list_comments(
    pool: &PgPool,
    actor: &ActorContext,
    review_id: i64,
) -> Result<Vec<DevRailReviewCommentResponse>, ApiError> {
    devrail_review_comments::list(pool, actor, review_id)
        .await
        .map_err(db_error)
        .map(|rows| rows.into_iter().map(comment_response).collect())
}
pub async fn create_comment(
    pool: &PgPool,
    actor: &ActorContext,
    review_id: i64,
    req: &CreateDevRailReviewCommentRequest,
) -> Result<DevRailReviewCommentResponse, ApiError> {
    if req.file_path.trim().is_empty() || req.body.trim().is_empty() {
        return Err(ApiError::validation("文件路径和审查意见不能为空"));
    }
    if req.file_path.len() > 1024 || req.body.len() > 10000 {
        return Err(ApiError::validation("文件路径或审查意见过长"));
    }
    if req
        .line_start
        .zip(req.line_end)
        .is_some_and(|(start, end)| end < start)
    {
        return Err(ApiError::validation("行号范围无效"));
    }
    let mut tx = pool.begin().await.map_err(db_error)?;
    let row = devrail_review_comments::create(&mut tx, actor, review_id, req)
        .await
        .map_err(|e| {
            if matches!(e, sqlx::Error::RowNotFound) {
                ApiError::not_found("审查不存在或无权访问")
            } else {
                db_error(e)
            }
        })?;
    repositories::audit_logs::record(&mut tx,Some(actor.user_id),"devrail.review.comment.create","devrail_review_comment",Some(row.id),json!({"reviewId":review_id,"filePath":row.file_path,"lineStart":row.line_start,"lineEnd":row.line_end})).await.map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(comment_response(row))
}
pub async fn update_comment(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
    req: &UpdateDevRailReviewCommentRequest,
) -> Result<DevRailReviewCommentResponse, ApiError> {
    if req.body.trim().is_empty() || req.body.len() > 10000 {
        return Err(ApiError::validation("审查意见不能为空或过长"));
    }
    let mut tx = pool.begin().await.map_err(db_error)?;
    let row = devrail_review_comments::update(&mut tx, actor, id, &req.body)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("审查意见不存在或无权编辑"))?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.review.comment.update",
        "devrail_review_comment",
        Some(id),
        json!({"reviewId":row.review_id}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(comment_response(row))
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
