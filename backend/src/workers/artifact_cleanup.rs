//! Periodically removes expired artifact bytes while retaining auditable metadata.

use crate::services::devrail_artifacts;
use sqlx::PgPool;
use std::path::PathBuf;
use std::time::Duration;

pub fn spawn(pool: PgPool, artifact_root: PathBuf) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            match devrail_artifacts::cleanup_expired(&pool, &artifact_root, 100).await {
                Ok(cleaned) if cleaned > 0 => {
                    crate::app_metrics::record_reconciliation("artifact_cleanup");
                    tracing::info!(cleaned, "expired DevRail artifacts cleaned");
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(error = %error, "artifact cleanup worker failed");
                    crate::app_metrics::record_reconciliation("artifact_cleanup_failed");
                }
            }
        }
    });
}
