use crate::access::ActorContext;
use crate::models::DevRailPushDispatchRow;
use crate::models::{DevRailPushDeviceRow, RegisterDevRailPushDeviceRequest};
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

const COLUMNS: &str = "id, device_name, platform, browser, timezone, client_version, endpoint_fingerprint, status, last_active_at, last_error, revoked_at, created_at, updated_at";

pub(crate) struct NewPushDevice<'a> {
    pub device_name: &'a str,
    pub platform: &'a str,
    pub browser: Option<&'a str>,
    pub timezone: Option<&'a str>,
    pub client_version: Option<&'a str>,
    pub endpoint_ciphertext: &'a [u8],
    pub endpoint_fingerprint: &'a str,
    pub keys_ciphertext: &'a [u8],
}

pub(crate) async fn list(
    pool: &PgPool,
    actor: &ActorContext,
) -> Result<Vec<DevRailPushDeviceRow>, sqlx::Error> {
    sqlx::query_as::<_, DevRailPushDeviceRow>(AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM devrail_push_devices WHERE organization_id=$1 AND user_id=$2 ORDER BY created_at DESC, id DESC"
    )))
    .bind(actor.organization_id)
    .bind(actor.user_id)
    .fetch_all(pool)
    .await
}

pub(crate) async fn register(
    c: &mut PgConnection,
    actor: &ActorContext,
    device: &NewPushDevice<'_>,
) -> Result<DevRailPushDeviceRow, sqlx::Error> {
    sqlx::query_as::<_, DevRailPushDeviceRow>(AssertSqlSafe(format!(
        "INSERT INTO devrail_push_devices (organization_id, user_id, device_name, platform, browser, timezone, client_version, endpoint_ciphertext, endpoint_fingerprint, keys_ciphertext, status, last_active_at, last_error, revoked_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'active',now(),NULL,NULL) ON CONFLICT (organization_id,user_id,endpoint_fingerprint) DO UPDATE SET device_name=EXCLUDED.device_name, platform=EXCLUDED.platform, browser=EXCLUDED.browser, timezone=EXCLUDED.timezone, client_version=EXCLUDED.client_version, endpoint_ciphertext=EXCLUDED.endpoint_ciphertext, keys_ciphertext=EXCLUDED.keys_ciphertext, status='active', last_active_at=now(), last_error=NULL, revoked_at=NULL, updated_at=now() RETURNING {COLUMNS}"
    )))
    .bind(actor.organization_id)
    .bind(actor.user_id)
    .bind(device.device_name)
    .bind(device.platform)
    .bind(device.browser)
    .bind(device.timezone)
    .bind(device.client_version)
    .bind(device.endpoint_ciphertext)
    .bind(device.endpoint_fingerprint)
    .bind(device.keys_ciphertext)
    .fetch_one(c)
    .await
}

pub(crate) async fn revoke(
    c: &mut PgConnection,
    actor: &ActorContext,
    id: i64,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query("UPDATE devrail_push_devices SET status='revoked', revoked_at=COALESCE(revoked_at,now()), updated_at=now() WHERE id=$1 AND organization_id=$2 AND user_id=$3 AND status <> 'revoked'")
        .bind(id)
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .execute(c)
        .await?
        .rows_affected() == 1)
}

pub(crate) fn request_fields(
    request: &RegisterDevRailPushDeviceRequest,
) -> (&str, &str, Option<&str>, Option<&str>, Option<&str>) {
    (
        &request.device_name,
        &request.platform,
        request.browser.as_deref(),
        request.timezone.as_deref(),
        request.client_version.as_deref(),
    )
}

pub(crate) async fn claim_dispatches(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<DevRailPushDispatchRow>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("WITH candidates AS (SELECT o.id FROM devrail_outbox_events o WHERE o.event_type='notification.created' AND o.status IN ('pending','processing') AND o.available_at<=now() ORDER BY o.id FOR UPDATE SKIP LOCKED LIMIT $1) UPDATE devrail_outbox_events o SET status='processing', attempts=o.attempts+1 WHERE o.id IN (SELECT id FROM candidates)")
        .bind(limit).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO devrail_push_deliveries (outbox_event_id,push_device_id,status) SELECT o.id,d.id,'pending' FROM devrail_outbox_events o JOIN devrail_notifications n ON n.organization_id=o.organization_id AND n.source_key=o.payload->>'notificationSource' JOIN devrail_notification_preferences p ON p.organization_id=n.organization_id AND p.user_id=n.recipient_user_id AND p.push_enabled=true JOIN devrail_push_devices d ON d.organization_id=n.organization_id AND d.user_id=n.recipient_user_id AND d.status='active' WHERE o.status='processing' ON CONFLICT (outbox_event_id,push_device_id) DO NOTHING")
        .execute(&mut *tx).await?;
    let rows = sqlx::query_as::<_, DevRailPushDispatchRow>("SELECT d.id AS delivery_id,d.outbox_event_id,pd.id AS device_id,pd.user_id,pd.endpoint_ciphertext,pd.keys_ciphertext,n.title,n.summary,n.deep_link,n.id AS notification_id FROM devrail_push_deliveries d JOIN devrail_push_devices pd ON pd.id=d.push_device_id JOIN devrail_outbox_events o ON o.id=d.outbox_event_id JOIN devrail_notifications n ON n.organization_id=o.organization_id AND n.source_key=o.payload->>'notificationSource' WHERE d.status IN ('pending','retrying') AND d.available_at<=now() AND o.status='processing' ORDER BY d.id FOR UPDATE SKIP LOCKED LIMIT $1")
        .bind(limit).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(rows)
}

pub(crate) async fn mark_delivery_sent(
    pool: &PgPool,
    delivery_id: i64,
    outbox_id: i64,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE devrail_push_deliveries SET status='sent', sent_at=now(), last_error=NULL, updated_at=now() WHERE id=$1")
        .bind(delivery_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE devrail_outbox_events SET status='published', published_at=now(), last_error=NULL WHERE id=$1")
        .bind(outbox_id).execute(&mut *tx).await?;
    tx.commit().await
}

pub(crate) async fn mark_delivery_failure(
    pool: &PgPool,
    delivery_id: i64,
    outbox_id: i64,
    device_id: i64,
    invalid: bool,
    error: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let status = if invalid { "invalid" } else { "retrying" };
    sqlx::query("UPDATE devrail_push_deliveries SET status=$2, attempts=attempts+1, available_at=now()+interval '60 seconds', last_error=$3, updated_at=now() WHERE id=$1")
        .bind(delivery_id).bind(status).bind(error).execute(&mut *tx).await?;
    if invalid {
        sqlx::query("UPDATE devrail_push_devices SET status='invalid', last_error=$2, updated_at=now() WHERE id=$1").bind(device_id).bind(error).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE devrail_outbox_events SET status='pending', available_at=now()+interval '60 seconds', last_error=$2 WHERE id=$1")
        .bind(outbox_id).bind(error).execute(&mut *tx).await?;
    tx.commit().await
}
