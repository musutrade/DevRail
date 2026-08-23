use crate::access::ActorContext;
use crate::error::ApiError;
use crate::models::*;
use crate::repositories::devrail_notifications;
use sqlx::PgPool;

fn preferences_response(
    row: DevRailNotificationPreferencesRow,
) -> DevRailNotificationPreferencesResponse {
    DevRailNotificationPreferencesResponse {
        in_app_enabled: row.in_app_enabled,
        push_enabled: false,
        push_supported: false,
        event_types: row
            .event_types
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        quiet_hours: row.quiet_hours,
        updated_at: row.updated_at,
    }
}

pub async fn get_preferences(
    pool: &PgPool,
    actor: &ActorContext,
) -> Result<DevRailNotificationPreferencesResponse, ApiError> {
    devrail_notifications::preferences(pool, actor)
        .await
        .map(preferences_response)
        .map_err(ApiError::internal)
}

pub async fn update_preferences(
    pool: &PgPool,
    actor: &ActorContext,
    request: &UpdateDevRailNotificationPreferencesRequest,
) -> Result<DevRailNotificationPreferencesResponse, ApiError> {
    if request
        .event_types
        .as_ref()
        .is_some_and(|items| items.len() > 32 || items.iter().any(|item| item.len() > 96))
    {
        return Err(ApiError::validation("通知类型设置超出范围"));
    }
    devrail_notifications::update_preferences(pool, actor, request)
        .await
        .map(preferences_response)
        .map_err(ApiError::internal)
}

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
