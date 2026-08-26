//! Storage-independent task access used by the Symphony scheduler.

use crate::access::ActorContext;
use crate::models::DevRailTaskRow;
use crate::repositories::devrail::{self, SchedulerReconciliation};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrackerErrorKind {
    Transient,
    Conflict,
    NotFound,
    Permanent,
}

#[derive(Debug, thiserror::Error)]
#[error("任务存储操作失败（{kind:?}）")]
pub(crate) struct TrackerError {
    kind: TrackerErrorKind,
}

impl TrackerError {
    pub const fn kind(&self) -> TrackerErrorKind {
        self.kind
    }
}

impl From<sqlx::Error> for TrackerError {
    fn from(error: sqlx::Error) -> Self {
        let kind = match error {
            sqlx::Error::RowNotFound => TrackerErrorKind::NotFound,
            sqlx::Error::Database(ref database_error)
                if database_error.is_unique_violation()
                    || database_error.is_check_violation()
                    || database_error.is_foreign_key_violation() =>
            {
                TrackerErrorKind::Conflict
            }
            sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::Io(_)
            | sqlx::Error::Tls(_) => TrackerErrorKind::Transient,
            _ => TrackerErrorKind::Permanent,
        };
        Self { kind }
    }
}

#[async_trait]
pub(crate) trait TaskTracker: Send + Sync {
    async fn find_task(
        &self,
        actor: &ActorContext,
        task_id: i64,
    ) -> Result<Option<DevRailTaskRow>, TrackerError>;

    async fn claim_dispatch_candidates(
        &self,
        claim_token: Uuid,
        limit: i64,
        claim_lease_seconds: i64,
        priority_aging_seconds: i64,
    ) -> Result<Vec<DevRailTaskRow>, TrackerError>;

    async fn renew_claim(
        &self,
        task_id: i64,
        claim_token: Uuid,
        claim_lease_seconds: i64,
    ) -> Result<bool, TrackerError>;

    async fn release_claim(&self, task_id: i64, claim_token: Uuid) -> Result<bool, TrackerError>;

    async fn schedule_retry(
        &self,
        task_id: i64,
        claim_token: Uuid,
        retry_at: DateTime<Utc>,
        reason: &str,
    ) -> Result<bool, TrackerError>;

    async fn fail_task(
        &self,
        task_id: i64,
        claim_token: Uuid,
        reason: &str,
    ) -> Result<bool, TrackerError>;

    async fn reconcile(
        &self,
        running_run_ids: &[i64],
        stale_timeout_seconds: i64,
    ) -> Result<SchedulerReconciliation, TrackerError>;
}

#[derive(Debug, Clone)]
pub(crate) struct PostgresTaskTracker {
    pool: PgPool,
}

impl PostgresTaskTracker {
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TaskTracker for PostgresTaskTracker {
    async fn find_task(
        &self,
        actor: &ActorContext,
        task_id: i64,
    ) -> Result<Option<DevRailTaskRow>, TrackerError> {
        devrail::find_task_by_id(&self.pool, actor, task_id)
            .await
            .map_err(Into::into)
    }

    async fn claim_dispatch_candidates(
        &self,
        claim_token: Uuid,
        limit: i64,
        claim_lease_seconds: i64,
        priority_aging_seconds: i64,
    ) -> Result<Vec<DevRailTaskRow>, TrackerError> {
        devrail::claim_scheduler_tasks(
            &self.pool,
            claim_token,
            limit,
            claim_lease_seconds,
            priority_aging_seconds,
        )
        .await
        .map_err(Into::into)
    }

    async fn renew_claim(
        &self,
        task_id: i64,
        claim_token: Uuid,
        claim_lease_seconds: i64,
    ) -> Result<bool, TrackerError> {
        devrail::renew_scheduler_claim(&self.pool, task_id, claim_token, claim_lease_seconds)
            .await
            .map_err(Into::into)
    }

    async fn release_claim(&self, task_id: i64, claim_token: Uuid) -> Result<bool, TrackerError> {
        devrail::release_scheduler_claim(&self.pool, task_id, claim_token)
            .await
            .map_err(Into::into)
    }

    async fn schedule_retry(
        &self,
        task_id: i64,
        claim_token: Uuid,
        retry_at: DateTime<Utc>,
        reason: &str,
    ) -> Result<bool, TrackerError> {
        devrail::schedule_retry(&self.pool, task_id, claim_token, retry_at, reason)
            .await
            .map_err(Into::into)
    }

    async fn fail_task(
        &self,
        task_id: i64,
        claim_token: Uuid,
        reason: &str,
    ) -> Result<bool, TrackerError> {
        devrail::fail_scheduler_task(&self.pool, task_id, claim_token, reason)
            .await
            .map_err(Into::into)
    }

    async fn reconcile(
        &self,
        running_run_ids: &[i64],
        stale_timeout_seconds: i64,
    ) -> Result<SchedulerReconciliation, TrackerError> {
        devrail::reconcile_scheduler_state(&self.pool, running_run_ids, stale_timeout_seconds)
            .await
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_errors_are_classified_without_exposing_details() {
        let not_found = TrackerError::from(sqlx::Error::RowNotFound);
        assert_eq!(not_found.kind(), TrackerErrorKind::NotFound);
        assert!(!not_found.to_string().contains("SELECT"));
        let timeout = TrackerError::from(sqlx::Error::PoolTimedOut);
        assert_eq!(timeout.kind(), TrackerErrorKind::Transient);
    }
}
