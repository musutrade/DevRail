//! Symphony-style scheduler for queued DevRail tasks.
//!
//! The scheduler is deliberately small: PostgreSQL owns queue ordering and
//! leases, while the Harness Supervisor remains the only component that can
//! start a Codex process.

use crate::access::{ActorContext, DataScope};
use crate::error::ApiError;
use crate::models::{CreateDevRailRunRequest, DevRailTaskRow};
use crate::repositories::devrail;
use crate::services::devrail_runs;
use crate::workers::harness_supervisor::HarnessSupervisor;
use sqlx::PgPool;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const POLL_INTERVAL: Duration = Duration::from_secs(10);
const CLAIM_BATCH_SIZE: i64 = 16;

pub fn spawn(pool: PgPool, supervisor: Arc<HarnessSupervisor>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(POLL_INTERVAL);
        loop {
            interval.tick().await;
            if let Err(error) = run_tick(&pool, &supervisor).await {
                tracing::error!(error = %error, "DevRail task scheduler tick failed");
            }
        }
    });
}

async fn run_tick(pool: &PgPool, supervisor: &HarnessSupervisor) -> Result<(), sqlx::Error> {
    let claim_token = Uuid::new_v4();
    let tasks = devrail::claim_scheduler_tasks(pool, claim_token, CLAIM_BATCH_SIZE).await?;
    if tasks.is_empty() {
        return Ok(());
    }
    tracing::debug!(
        claimed_tasks = tasks.len(),
        "DevRail scheduler claimed queued tasks"
    );
    for task in tasks {
        if let Err(error) = dispatch_task(pool, supervisor, &task, claim_token).await {
            tracing::warn!(task_id = task.id, error = %error, "DevRail scheduler could not dispatch task");
            // A database/service failure before the task transitions out of
            // queued must not strand its lease. Terminal run failures already
            // clear the claim through update_task_status.
            let _ = devrail::release_scheduler_claim(pool, task.id, claim_token).await;
        }
    }
    Ok(())
}

async fn dispatch_task(
    pool: &PgPool,
    supervisor: &HarnessSupervisor,
    task: &DevRailTaskRow,
    claim_token: Uuid,
) -> Result<(), ApiError> {
    let environment_id = task
        .environment_id
        .ok_or_else(|| ApiError::validation("排队任务缺少运行环境"))?;
    let actor = scheduler_actor(task);
    let request = CreateDevRailRunRequest {
        environment_id,
        idempotency_key: scheduler_idempotency_key(task.id, claim_token),
        model_id: None,
        input: None,
        branch_name: None,
    };
    match devrail_runs::create_run(pool, &actor, supervisor, task.id, &request).await {
        Ok(run) => {
            tracing::info!(
                task_id = task.id,
                run_id = run.id,
                "DevRail scheduler started task"
            );
            Ok(())
        }
        Err(ApiError::Conflict(message)) if message == "Harness 并发额度已用尽" => {
            // Capacity is transient. The claim is released and the task stays
            // queued for the next tick rather than being reported as failed.
            Err(ApiError::conflict(message))
        }
        Err(error) => Err(error),
    }
}

fn scheduler_actor(task: &DevRailTaskRow) -> ActorContext {
    ActorContext {
        user_id: task.owner_user_id,
        session_id: 0,
        organization_id: task.organization_id,
        department_id: task.department_id,
        data_scope: DataScope::Organization,
        permission_codes: BTreeSet::from([
            "devrail:task:read".to_string(),
            "devrail:run:execute".to_string(),
        ]),
    }
}

fn scheduler_idempotency_key(task_id: i64, claim_token: Uuid) -> String {
    format!("scheduler:{task_id}:{claim_token}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    fn task() -> DevRailTaskRow {
        DevRailTaskRow {
            id: 42,
            organization_id: 7,
            department_id: Some(3),
            owner_user_id: 9,
            project_id: 11,
            repository_id: None,
            environment_id: Some(13),
            assignee_user_id: None,
            title: "任务".to_string(),
            goal: "目标".to_string(),
            background: None,
            acceptance_criteria: None,
            constraints: None,
            priority: "normal".to_string(),
            status: "queued".to_string(),
            labels: json!([]),
            due_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            archived_at: None,
        }
    }

    #[test]
    fn scheduler_key_is_bounded_and_reproducible() {
        let token = Uuid::nil();
        let first = scheduler_idempotency_key(42, token);
        assert_eq!(first, scheduler_idempotency_key(42, token));
        assert!(first.len() <= 128);
        assert!(first.starts_with("scheduler:42:"));
    }

    #[test]
    fn scheduler_actor_uses_task_scope_without_a_user_session() {
        let actor = scheduler_actor(&task());
        assert_eq!(actor.session_id, 0);
        assert_eq!(actor.organization_id, 7);
        assert_eq!(actor.data_scope, DataScope::Organization);
        assert!(actor.has_permission("devrail:run:execute"));
    }
}
