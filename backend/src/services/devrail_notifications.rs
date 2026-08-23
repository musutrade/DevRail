use crate::access::ActorContext;
use crate::error::ApiError;
use crate::models::*;
use crate::repositories::devrail_notifications;
use sqlx::PgPool;

fn response(row: DevRailNotificationRow) -> DevRailNotificationResponse {
    DevRailNotificationResponse {
        id: row.id,
        event_type: row.event_type,
        level: row.level,
        title: row.title,
        summary: row.summary,
        resource_type: row.resource_type,
        resource_id: row.resource_id,
        deep_link: row.deep_link,
        read_at: row.read_at,
        expires_at: row.expires_at,
        created_at: row.created_at,
    }
}
pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    page: i64,
    size: i64,
) -> Result<DevRailNotificationPage, ApiError> {
    let (items, (total, unread)) = tokio::try_join!(
        devrail_notifications::list(pool, actor, page, size),
        devrail_notifications::count(pool, actor)
    )
    .map_err(ApiError::internal)?;
    Ok(DevRailNotificationPage {
        items: items.into_iter().map(response).collect(),
        total,
        unread,
        page,
        page_size: size,
    })
}
pub async fn mark_read(pool: &PgPool, actor: &ActorContext, id: i64) -> Result<(), ApiError> {
    if devrail_notifications::mark_read(pool, actor, id)
        .await
        .map_err(ApiError::internal)?
    {
        Ok(())
    } else {
        Err(ApiError::not_found("通知不存在或无权访问"))
    }
}
pub async fn mark_all_read(pool: &PgPool, actor: &ActorContext) -> Result<(), ApiError> {
    devrail_notifications::mark_all_read(pool, actor)
        .await
        .map_err(ApiError::internal)
        .map(|_| ())
}
