//! Asynchronous Web Push delivery from the transactional outbox.

use crate::app_metrics;
use crate::mfa::MfaConfig;
use crate::repositories::devrail_push;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use web_push::{
    ContentEncoding, IsahcWebPushClient, SubscriptionInfo, VapidSignatureBuilder, WebPushClient,
    WebPushMessageBuilder,
};

#[derive(Deserialize)]
struct Keys {
    p256dh: String,
    auth: String,
}

pub fn spawn(
    pool: PgPool,
    mfa: Arc<MfaConfig>,
    private_key: Option<String>,
    subject: Option<String>,
) {
    tokio::spawn(async move {
        let Some(private_key) = private_key else {
            return;
        };
        let Some(subject) = subject else { return };
        let client = match IsahcWebPushClient::new() {
            Ok(client) => client,
            Err(_) => return,
        };
        loop {
            if let Ok((backlog, invalid_devices)) = devrail_push::delivery_metrics(&pool).await {
                app_metrics::record_push_backlog(backlog);
                app_metrics::record_push_invalid_devices(invalid_devices);
            }
            if let Ok(rows) = devrail_push::claim_dispatches(&pool, 20).await {
                for row in rows {
                    let result = send(&client, &mfa, &private_key, &subject, &row).await;
                    match result {
                        Ok(()) => {
                            app_metrics::record_push_delivery("sent");
                            let _ = devrail_push::mark_delivery_sent(
                                &pool,
                                row.delivery_id,
                                row.outbox_event_id,
                            )
                            .await;
                        }
                        Err(error) => {
                            let text = error.to_string().chars().take(500).collect::<String>();
                            let invalid = text.contains("404") || text.contains("410");
                            app_metrics::record_push_delivery(if invalid {
                                "invalid"
                            } else {
                                "retrying"
                            });
                            let _ = devrail_push::mark_delivery_failure(
                                &pool,
                                row.delivery_id,
                                row.outbox_event_id,
                                row.device_id,
                                invalid,
                                &text,
                            )
                            .await;
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

async fn send(
    client: &IsahcWebPushClient,
    mfa: &MfaConfig,
    private_key: &str,
    subject: &str,
    row: &crate::models::DevRailPushDispatchRow,
) -> anyhow::Result<()> {
    let endpoint = String::from_utf8(mfa.decrypt_value(row.user_id, &row.endpoint_ciphertext)?)?;
    let keys: Keys =
        serde_json::from_slice(&mfa.decrypt_value(row.user_id, &row.keys_ciphertext)?)?;
    let subscription = SubscriptionInfo::new(endpoint, keys.p256dh, keys.auth);
    let mut message = WebPushMessageBuilder::new(&subscription);
    let payload = serde_json::to_vec(
        &json!({"notificationId":row.notification_id,"title":row.title,"summary":row.summary,"deepLink":row.deep_link}),
    )?;
    message.set_payload(ContentEncoding::Aes128Gcm, &payload);
    let mut vapid = VapidSignatureBuilder::from_base64(private_key, &subscription)?;
    vapid.add_claim("sub", subject);
    message.set_vapid_signature(vapid.build()?);
    client.send(message.build()?).await?;
    Ok(())
}
