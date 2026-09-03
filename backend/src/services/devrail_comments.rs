use crate::access::ActorContext;
use crate::error::{db_error, ApiError};
use crate::models::*;
use crate::repositories::{audit_logs, devrail_comments, devrail_notifications};
use serde_json::json;
use sqlx::PgPool;

fn valid_body(body: &str) -> Result<String, ApiError> {
    let body = body.trim();
    if body.is_empty() || body.chars().count() > 10_000 {
        return Err(ApiError::validation("评论不能为空且不能超过 10000 个字符"));
    }
    Ok(body.to_string())
}
fn mentions(body: &str) -> Vec<String> {
    body.split_whitespace()
        .filter_map(|word| word.strip_prefix('@'))
        .filter(|name| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
        .map(str::to_string)
        .collect()
}
fn response(row: DevRailTaskCommentRow) -> DevRailTaskCommentResponse {
    DevRailTaskCommentResponse {
        id: row.id,
        task_id: row.task_id,
        author_user_id: row.author_user_id,
        author_username: row.author_username,
        author_display_name: row.author_display_name,
        body: row.body,
        mentions: row
            .mentions
            .as_array()
            .map(|v| {
                v.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        created_at: row.created_at,
        edited_at: row.edited_at,
        deleted: row.deleted_at.is_some(),
    }
}

pub async fn update(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
    request: &UpdateDevRailTaskCommentRequest,
) -> Result<DevRailTaskCommentResponse, ApiError> {
    let body = valid_body(&request.body)?;
    let names = mentions(&body);
    let mut tx = pool.begin().await.map_err(db_error)?;
    let row = devrail_comments::update(&mut tx, actor, id, &body, &json!(names))
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("评论不存在、已删除或无权修改"))?;
    audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.comment.update",
        "devrail_task_comment",
        Some(id),
        json!({"taskId": row.task_id, "mentionCount": names.len()}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(response(row))
}

pub async fn delete(pool: &PgPool, actor: &ActorContext, id: i64) -> Result<(), ApiError> {
    let mut tx = pool.begin().await.map_err(db_error)?;
    if !devrail_comments::soft_delete(&mut tx, actor, id)
        .await
        .map_err(db_error)?
    {
        return Err(ApiError::not_found("评论不存在、已删除或无权删除"));
    }
    audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.comment.delete",
        "devrail_task_comment",
        Some(id),
        json!({"redacted": true}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(())
}
pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: i64,
    page: i64,
    size: i64,
) -> Result<DevRailTaskCommentPage, ApiError> {
    let (items, total) = devrail_comments::list(pool, actor, task_id, page, size)
        .await
        .map_err(db_error)?;
    Ok(DevRailTaskCommentPage {
        items: items.into_iter().map(response).collect(),
        total,
        page,
        page_size: size,
    })
}
pub async fn create(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: i64,
    request: &CreateDevRailTaskCommentRequest,
) -> Result<DevRailTaskCommentResponse, ApiError> {
    let body = valid_body(&request.body)?;
    let names = mentions(&body);
    let request = CreateDevRailTaskCommentRequest { body };
    let mut tx = pool.begin().await.map_err(db_error)?;
    let value = json!(names);
    let row = devrail_comments::create(&mut tx, actor, task_id, &request, &value)
        .await
        .map_err(db_error)?;
    let project_id = devrail_comments::task_project_id(&mut tx, actor.organization_id, task_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("任务不存在或超出数据范围"))?;
    let users = devrail_comments::mentioned_users(&mut tx, actor.organization_id, &names)
        .await
        .map_err(db_error)?;
    for (user_id, username) in users {
        if user_id == actor.user_id {
            continue;
        }
        let source = format!("task-comment:{}:mention:{}", row.id, user_id);
        devrail_notifications::create(
            &mut tx,
            &devrail_notifications::NewNotification {
                organization_id: actor.organization_id,
                department_id: actor.department_id,
                recipient_user_id: user_id,
                event_type: "devrail.comment.mentioned",
                level: "info",
                title: "你被任务评论提及",
                summary: &format!("@{username}，你在任务评论中被提及"),
                resource_type: Some("devrail_task"),
                resource_id: Some(task_id),
                deep_link: Some(&format!("/devrail/projects/{project_id}/tasks/{task_id}")),
                source_key: &source,
            },
        )
        .await
        .map_err(db_error)?;
        devrail_notifications::outbox(
            &mut tx,
            actor.organization_id,
            "notification.created",
            "devrail_comment",
            Some(row.id),
            &json!({"notificationSource": source, "eventType": "devrail.comment.mentioned"}),
        )
        .await
        .map_err(db_error)?;
    }
    let hydrated = devrail_comments::hydrate(&mut tx, row.id)
        .await
        .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(response(hydrated))
}
