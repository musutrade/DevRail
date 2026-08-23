use crate::access::ActorContext;
use crate::error::ApiError;
use crate::mfa::MfaConfig;
use crate::models::{DevRailPushDeviceResponse, RegisterDevRailPushDeviceRequest};
use crate::repositories::{audit_logs, devrail_push};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

fn validate_text(value: &str, field: &str, max: usize) -> Result<(), ApiError> {
    if value.trim().is_empty() || value.len() > max {
        return Err(ApiError::validation(format!(
            "{field}不能为空且长度不得超过{max}个字符"
        )));
    }
    Ok(())
}

fn validate_optional(value: Option<&str>, field: &str, max: usize) -> Result<(), ApiError> {
    if let Some(value) = value {
        validate_text(value, field, max)?;
    }
    Ok(())
}

fn fingerprint(endpoint: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(endpoint.as_bytes());
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn response(row: crate::models::DevRailPushDeviceRow) -> DevRailPushDeviceResponse {
    DevRailPushDeviceResponse {
        id: row.id,
        device_name: row.device_name,
        platform: row.platform,
        browser: row.browser,
        timezone: row.timezone,
        client_version: row.client_version,
        endpoint_fingerprint: row.endpoint_fingerprint,
        status: row.status,
        last_active_at: row.last_active_at,
        last_error: row.last_error,
        revoked_at: row.revoked_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
) -> Result<Vec<DevRailPushDeviceResponse>, ApiError> {
    devrail_push::list(pool, actor)
        .await
        .map(|rows| rows.into_iter().map(response).collect())
        .map_err(ApiError::internal)
}

pub async fn register(
    pool: &PgPool,
    actor: &ActorContext,
    mfa: &MfaConfig,
    request: &RegisterDevRailPushDeviceRequest,
) -> Result<DevRailPushDeviceResponse, ApiError> {
    validate_text(&request.device_name, "设备名称", 128)?;
    validate_text(&request.platform, "平台", 32)?;
    validate_text(&request.endpoint, "推送端点", 2048)?;
    validate_text(&request.p256dh, "p256dh 密钥", 512)?;
    validate_text(&request.auth, "auth 密钥", 512)?;
    validate_optional(request.browser.as_deref(), "浏览器", 64)?;
    validate_optional(request.timezone.as_deref(), "时区", 64)?;
    validate_optional(request.client_version.as_deref(), "客户端版本", 64)?;

    let endpoint_fingerprint = fingerprint(&request.endpoint);
    let endpoint_ciphertext = mfa
        .encrypt_value(actor.user_id, request.endpoint.as_bytes())
        .map_err(ApiError::internal)?;
    let keys_json = json!({"p256dh": request.p256dh, "auth": request.auth}).to_string();
    let keys_ciphertext = mfa
        .encrypt_value(actor.user_id, keys_json.as_bytes())
        .map_err(ApiError::internal)?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let fields = devrail_push::request_fields(request);
    let row = devrail_push::register(
        &mut transaction,
        actor,
        &devrail_push::NewPushDevice {
            device_name: fields.0,
            platform: fields.1,
            browser: fields.2,
            timezone: fields.3,
            client_version: fields.4,
            endpoint_ciphertext: &endpoint_ciphertext,
            endpoint_fingerprint: &endpoint_fingerprint,
            keys_ciphertext: &keys_ciphertext,
        },
    )
    .await
    .map_err(ApiError::internal)?;
    audit_logs::record(
        &mut transaction,
        Some(actor.user_id),
        "devrail.push_device.register",
        "devrail_push_device",
        Some(row.id),
        json!({"platform": request.platform, "endpointFingerprint": endpoint_fingerprint}),
    )
    .await
    .map_err(ApiError::internal)?;
    transaction.commit().await.map_err(ApiError::internal)?;
    Ok(response(row))
}

pub async fn revoke(pool: &PgPool, actor: &ActorContext, id: i64) -> Result<(), ApiError> {
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let changed = devrail_push::revoke(&mut transaction, actor, id)
        .await
        .map_err(ApiError::internal)?;
    if !changed {
        return Err(ApiError::not_found("推送设备不存在或已撤销"));
    }
    audit_logs::record(
        &mut transaction,
        Some(actor.user_id),
        "devrail.push_device.revoke",
        "devrail_push_device",
        Some(id),
        json!({}),
    )
    .await
    .map_err(ApiError::internal)?;
    transaction.commit().await.map_err(ApiError::internal)
}

#[cfg(test)]
mod tests {
    use super::fingerprint;

    #[test]
    fn fingerprint_is_stable_and_non_reversible() {
        assert_eq!(fingerprint("https://push.example/a").len(), 64);
        assert_eq!(fingerprint("same"), fingerprint("same"));
        assert_ne!(fingerprint("same"), "same");
    }
}
