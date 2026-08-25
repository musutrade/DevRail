use crate::access::ActorContext;
use crate::models::{DevRailNotificationPreferencesRow, DevRailNotificationRow};
use serde_json::Value;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

const COLUMNS: &str = "id, organization_id, department_id, recipient_user_id, event_type, level, title, summary, resource_type, resource_id, deep_link, source_key, read_at, expires_at, created_at";

pub(crate) struct NewNotification<'a> {
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub recipient_user_id: i64,
    pub event_type: &'a str,
    pub level: &'a str,
    pub title: &'a str,
    pub summary: &'a str,
    pub resource_type: Option<&'a str>,
    pub resource_id: Option<i64>,
    pub deep_link: Option<&'a str>,
    pub source_key: &'a str,
}

pub(crate) async fn create(
    c: &mut PgConnection,
    n: &NewNotification<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO devrail_notifications (organization_id, department_id, recipient_user_id, event_type, level, title, summary, resource_type, resource_id, deep_link, source_key) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT (recipient_user_id, source_key) DO NOTHING")
        .bind(n.organization_id).bind(n.department_id).bind(n.recipient_user_id).bind(n.event_type).bind(n.level).bind(n.title).bind(n.summary).bind(n.resource_type).bind(n.resource_id).bind(n.deep_link).bind(n.source_key).execute(c).await.map(|_| ())
}
pub(crate) async fn outbox(
    c: &mut PgConnection,
    organization_id: i64,
    event_type: &str,
    aggregate_type: &str,
    aggregate_id: Option<i64>,
    payload: &Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO devrail_outbox_events (organization_id, event_type, aggregate_type, aggregate_id, payload) VALUES ($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING")
        .bind(organization_id).bind(event_type).bind(aggregate_type).bind(aggregate_id).bind(payload).execute(c).await.map(|_| ())
}
pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    page: i64,
    size: i64,
) -> Result<Vec<DevRailNotificationRow>, sqlx::Error> {
    sqlx::query_as::<_, DevRailNotificationRow>(AssertSqlSafe(format!("SELECT {COLUMNS} FROM devrail_notifications WHERE organization_id=$1 AND recipient_user_id=$2 AND (expires_at IS NULL OR expires_at > now()) ORDER BY created_at DESC, id DESC LIMIT $3 OFFSET $4")))
        .bind(actor.organization_id).bind(actor.user_id).bind(size).bind((page - 1) * size).fetch_all(pool).await
}
pub async fn count(pool: &PgPool, actor: &ActorContext) -> Result<(i64, i64), sqlx::Error> {
    sqlx::query_as::<_, (i64, i64)>("SELECT count(*) FILTER (WHERE expires_at IS NULL OR expires_at > now()), count(*) FILTER (WHERE read_at IS NULL AND (expires_at IS NULL OR expires_at > now())) FROM devrail_notifications WHERE organization_id=$1 AND recipient_user_id=$2")
        .bind(actor.organization_id).bind(actor.user_id).fetch_one(pool).await
}
pub async fn mark_read(pool: &PgPool, actor: &ActorContext, id: i64) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query("UPDATE devrail_notifications SET read_at=COALESCE(read_at,now()) WHERE id=$1 AND organization_id=$2 AND recipient_user_id=$3").bind(id).bind(actor.organization_id).bind(actor.user_id).execute(pool).await?.rows_affected() == 1)
}
pub async fn mark_all_read(pool: &PgPool, actor: &ActorContext) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query("UPDATE devrail_notifications SET read_at=now() WHERE organization_id=$1 AND recipient_user_id=$2 AND read_at IS NULL").bind(actor.organization_id).bind(actor.user_id).execute(pool).await?.rows_affected())
}

pub async fn preferences(
    pool: &PgPool,
    actor: &ActorContext,
) -> Result<DevRailNotificationPreferencesRow, sqlx::Error> {
    sqlx::query("INSERT INTO devrail_notification_preferences (organization_id, user_id) VALUES ($1,$2) ON CONFLICT (organization_id,user_id) DO NOTHING")
        .bind(actor.organization_id).bind(actor.user_id).execute(pool).await?;
    sqlx::query_as::<_, DevRailNotificationPreferencesRow>("SELECT organization_id, user_id, in_app_enabled, push_enabled, event_types, quiet_hours, updated_at FROM devrail_notification_preferences WHERE organization_id=$1 AND user_id=$2")
        .bind(actor.organization_id).bind(actor.user_id).fetch_one(pool).await
}

pub async fn update_preferences(
    pool: &PgPool,
    actor: &ActorContext,
    request: &crate::models::UpdateDevRailNotificationPreferencesRequest,
) -> Result<DevRailNotificationPreferencesRow, sqlx::Error> {
    sqlx::query_as::<_, DevRailNotificationPreferencesRow>("INSERT INTO devrail_notification_preferences (organization_id,user_id,in_app_enabled,push_enabled,event_types,quiet_hours) VALUES ($1,$2,COALESCE($3,TRUE),COALESCE($4,FALSE),COALESCE($5,'[]'::jsonb),COALESCE($6,'{}'::jsonb)) ON CONFLICT (organization_id,user_id) DO UPDATE SET in_app_enabled=COALESCE($3,devrail_notification_preferences.in_app_enabled), push_enabled=COALESCE($4,devrail_notification_preferences.push_enabled), event_types=COALESCE($5,devrail_notification_preferences.event_types), quiet_hours=COALESCE($6,devrail_notification_preferences.quiet_hours), updated_at=now() RETURNING organization_id, user_id, in_app_enabled, push_enabled, event_types, quiet_hours, updated_at")
        .bind(actor.organization_id).bind(actor.user_id).bind(request.in_app_enabled).bind(request.push_enabled).bind(request.event_types.as_ref().map(|v| serde_json::json!(v))).bind(request.quiet_hours.as_ref()).fetch_one(pool).await
}
