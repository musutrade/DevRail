use crate::services::devrail_approvals;
use crate::workers::harness_supervisor::HarnessSupervisor;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;

pub fn spawn(pool: PgPool, supervisor: Arc<HarnessSupervisor>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            match devrail_approvals::expire_due(&pool, &supervisor).await {
                Ok(count) if count > 0 => {
                    tracing::info!(expired_approvals = count, "expired DevRail approvals")
                }
                Ok(_) => {}
                Err(error) => tracing::error!(error = %error, "approval expiry worker failed"),
            }
        }
    });
}
