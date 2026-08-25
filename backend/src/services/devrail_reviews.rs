use crate::{
    access::ActorContext,
    error::{db_error, ApiError},
    models::*,
    repositories::{self, devrail_review_comments, devrail_reviews},
};
use reqwest::Client;
use serde_json::json;
use serde_json::Value;
use sqlx::PgPool;
use urlencoding::encode;
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

pub async fn list_external_comments(
    pool: &PgPool,
    actor: &ActorContext,
    review_id: i64,
) -> Result<Vec<DevRailExternalReviewCommentResponse>, ApiError> {
    repositories::devrail_external_review_comments::list(pool, actor, review_id)
        .await
        .map_err(db_error)
}

pub async fn sync_external_comments(
    pool: &PgPool,
    actor: &ActorContext,
    review_id: i64,
    req: &SyncDevRailExternalReviewRequest,
) -> Result<Vec<DevRailExternalReviewCommentResponse>, ApiError> {
    if req.number < 1 {
        return Err(ApiError::validation("合并请求编号无效"));
    }
    let provider =
        crate::services::devrail::get_git_provider(pool, actor, req.project_id, req.repository_id)
            .await?;
    let row =
        repositories::devrail::find_repository(pool, actor, req.project_id, req.repository_id)
            .await
            .map_err(db_error)?
            .ok_or_else(|| ApiError::not_found("仓库不存在"))?;
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
    let (provider_name, values) = if provider.provider == "github" {
        let endpoint = format!(
            "https://api.github.com/repos/{}/{}/pulls/{}/comments",
            provider.owner, provider.repository, req.number
        );
        let response = client
            .get(endpoint)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|_| ApiError::conflict("同步 GitHub 审查意见失败"))?;
        if !response.status().is_success() {
            return Err(ApiError::conflict("GitHub 拒绝读取审查意见"));
        }
        (
            "github",
            response
                .json::<Vec<Value>>()
                .await
                .map_err(|_| ApiError::conflict("GitHub 返回无效响应"))?,
        )
    } else {
        let project_path = format!("{}/{}", provider.owner, provider.repository);
        let project = encode(&project_path);
        let endpoint = format!(
            "https://gitlab.com/api/v4/projects/{project}/merge_requests/{}/discussions",
            req.number
        );
        let response = client
            .get(endpoint)
            .header("PRIVATE-TOKEN", &token)
            .send()
            .await
            .map_err(|_| ApiError::conflict("同步 GitLab 审查意见失败"))?;
        if !response.status().is_success() {
            return Err(ApiError::conflict("GitLab 拒绝读取审查意见"));
        }
        (
            "gitlab",
            response
                .json::<Vec<Value>>()
                .await
                .map_err(|_| ApiError::conflict("GitLab 返回无效响应"))?,
        )
    };
    let mut tx = pool.begin().await.map_err(db_error)?;
    let mut external_ids = Vec::new();
    for value in values {
        let (id, body, author, path, line_start, line_end, resolved, deleted) =
            if provider_name == "github" {
                (
                    value
                        .get("id")
                        .and_then(Value::as_i64)
                        .unwrap_or_default()
                        .to_string(),
                    value
                        .get("body")
                        .and_then(Value::as_str)
                        .unwrap_or("[评论已删除]")
                        .to_string(),
                    value
                        .pointer("/user/login")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string(),
                    value
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or("(general)")
                        .to_string(),
                    value.get("line").and_then(Value::as_i64).map(|v| v as i32),
                    value.get("line").and_then(Value::as_i64).map(|v| v as i32),
                    false,
                    value.get("body").is_none() || value.get("body").is_some_and(Value::is_null),
                )
            } else {
                let note = value.pointer("/notes/0").unwrap_or(&value);
                let position = value
                    .get("position")
                    .or_else(|| note.get("position"))
                    .unwrap_or(&Value::Null);
                let path = position
                    .get("new_path")
                    .or_else(|| position.get("old_path"))
                    .and_then(Value::as_str)
                    .unwrap_or("(general)")
                    .to_string();
                let line_start = position
                    .get("new_line")
                    .or_else(|| position.get("old_line"))
                    .and_then(Value::as_i64)
                    .map(|v| v as i32);
                let line_end = line_start;
                (
                    note.get("id")
                        .and_then(Value::as_i64)
                        .unwrap_or_default()
                        .to_string(),
                    note.get("body")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    note.pointer("/author/username")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string(),
                    path,
                    line_start,
                    line_end,
                    value
                        .get("resolved")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    note.get("body").is_none() || note.get("body").is_some_and(Value::is_null),
                )
            };
        external_ids.push(id.clone());
        repositories::devrail_external_review_comments::upsert(
            &mut tx,
            &repositories::devrail_external_review_comments::ExternalReviewCommentInput {
                review_id,
                provider: provider_name,
                external_id: &id,
                file_path: &path,
                line_start,
                line_end,
                body: &body,
                author_name: &author,
                external_created_at: None,
                resolved,
                deleted,
            },
        )
        .await
        .map_err(db_error)?;
    }
    repositories::devrail_external_review_comments::mark_missing_deleted(
        &mut tx,
        review_id,
        provider_name,
        &external_ids,
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    list_external_comments(pool, actor, review_id).await
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
