use crate::access::ActorContext;
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
