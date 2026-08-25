use crate::repositories::devrail_runs;
use sqlx::PgPool;
use std::time::Duration;

pub fn spawn(pool: PgPool) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            match devrail_runs::expired_branches(&pool).await {
                Ok(rows) => {
                    for (run_id, _, branch) in rows {
                        let mut tx = match pool.begin().await {
                            Ok(tx) => tx,
                            Err(error) => {
                                tracing::error!(error = %error, "branch cleanup transaction failed");
                                continue;
                            }
                        };
                        if let Err(error) =
                            devrail_runs::clear_expired_branch(&mut tx, run_id).await
                        {
                            tracing::error!(run_id, error = %error, "branch expiry cleanup failed");
                            continue;
                        }
                        if let Err(error) = tx.commit().await {
                            tracing::error!(run_id, error = %error, "branch expiry cleanup commit failed");
                        } else {
                            tracing::info!(run_id, branch = %branch, "expired temporary branch binding cleared");
                        }
                    }
                }
                Err(error) => tracing::error!(error = %error, "branch cleanup worker failed"),
            }
        }
    });
}
