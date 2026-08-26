//! Reconciles repository WORKFLOW.md files into persisted last-known-good versions.

use crate::access::{ActorContext, ActorType, DataScope};
use crate::orchestration::workflow::{
    self, PlatformWorkflowPolicy, WorkflowError, WorkflowSnapshot,
};
use crate::repositories::{audit_logs, devrail_workflows};
use serde_json::json;
use sqlx::PgPool;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy)]
pub struct WorkflowReloaderPolicy {
    pub poll_interval: Duration,
    pub jitter_percent: u8,
}

impl Default for WorkflowReloaderPolicy {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(15),
            jitter_percent: 20,
        }
    }
}

pub fn spawn(
    pool: PgPool,
    controlled_workspace_root: PathBuf,
    policy: WorkflowReloaderPolicy,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let tick_started = Instant::now();
            match reload_once(&pool, &controlled_workspace_root).await {
                Ok(environment_count) => {
                    crate::app_metrics::record_workflow_reload_health(true);
                    tracing::debug!(
                        environments = environment_count,
                        "DevRail workflow reload reconciliation completed"
                    );
                }
                Err(error) => {
                    crate::app_metrics::record_workflow_reload_health(false);
                    tracing::error!(error = %error, "DevRail workflow reload tick failed");
                }
            }
            crate::app_metrics::record_workflow_reload_duration(tick_started.elapsed());
            let delay = jittered_delay(policy, random_seed());
            tokio::select! {
                biased;
                _ = shutdown.cancelled() => {
                    tracing::info!("DevRail workflow reloader stopped gracefully");
                    break;
                }
                _ = tokio::time::sleep(delay) => {}
            }
        }
    })
}

pub(crate) async fn reload_once(
    pool: &PgPool,
    controlled_workspace_root: &std::path::Path,
) -> Result<usize, sqlx::Error> {
    let organizations = devrail_workflows::list_target_organizations(pool).await?;
    let mut environment_count = 0_usize;
    for organization_id in organizations {
        let targets =
            devrail_workflows::list_targets_for_organization(pool, organization_id).await?;
        environment_count = environment_count.saturating_add(targets.len());
        for target in targets {
            let mut platform_policy =
                PlatformWorkflowPolicy::secure_default(target.max_duration_secs);
            platform_policy.network_allowed = target.network_mode == "allowlist";
            if let Some(allowed_tools) = target
                .tool_policy
                .get("allowedTools")
                .and_then(serde_json::Value::as_array)
            {
                let environment_tools = allowed_tools
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<BTreeSet<_>>();
                platform_policy
                    .allowed_tools
                    .retain(|tool| environment_tools.contains(tool.as_str()));
            }
            match workflow::load_repository_workflow(
                controlled_workspace_root,
                std::path::Path::new(&target.workspace_root),
                &platform_policy,
            )
            .await
            {
                Ok(snapshot) => accept_candidate(pool, &target, &snapshot).await?,
                Err(error) => record_invalid_candidate(pool, &target, &error).await?,
            }
        }
    }
    Ok(environment_count)
}

async fn accept_candidate(
    pool: &PgPool,
    target: &devrail_workflows::WorkflowEnvironmentTarget,
    snapshot: &WorkflowSnapshot,
) -> Result<(), sqlx::Error> {
    let previous = devrail_workflows::last_known_good_for_target(
        pool,
        target.organization_id,
        target.environment_id,
    )
    .await?;
    let changed = previous.as_ref().is_none_or(|version| {
        version.digest != snapshot.digest
            || version.source != snapshot.source.as_str()
            || version.declared_version != snapshot.declared_version
    });
    let normalized_snapshot = serde_json::to_value(snapshot).map_err(|error| {
        sqlx::Error::Protocol(format!("workflow snapshot serialization failed: {error}"))
    })?;
    let mut tx = pool.begin().await?;
    let accepted = devrail_workflows::accept_version(
        &mut tx,
        &devrail_workflows::NewWorkflowVersion {
            organization_id: target.organization_id,
            department_id: target.department_id,
            owner_user_id: target.owner_user_id,
            environment_id: target.environment_id,
            source: snapshot.source.as_str(),
            declared_version: &snapshot.declared_version,
            digest: &snapshot.digest,
            normalized_snapshot: &normalized_snapshot,
            prompt_body: &snapshot.prompt_template,
        },
    )
    .await?;
    if changed {
        audit_logs::record_actor(
            &mut tx,
            &system_actor(target),
            "devrail.workflow.accept",
            "devrail_environment",
            Some(target.environment_id),
            json!({
                "actorType": "system",
                "workflowVersionId": accepted.id,
                "workflowVersion": snapshot.declared_version,
                "workflowDigest": snapshot.digest,
                "workflowSource": snapshot.source.as_str(),
                "policyVersion": "devrail-policy-v1"
            }),
        )
        .await?;
    }
    tx.commit().await?;
    crate::app_metrics::record_workflow_reload(if changed { "accepted" } else { "unchanged" });
    tracing::debug!(
        organization_id = target.organization_id,
        environment_id = target.environment_id,
        workflow_digest = %short_digest(&accepted.digest),
        outcome = if changed { "accepted" } else { "unchanged" },
        "repository workflow reconciled"
    );
    Ok(())
}

async fn record_invalid_candidate(
    pool: &PgPool,
    target: &devrail_workflows::WorkflowEnvironmentTarget,
    error: &WorkflowError,
) -> Result<(), sqlx::Error> {
    let fallback_key = format!("{}:{}", target.environment_id, error.kind().as_str());
    let candidate_digest = error
        .candidate_digest()
        .map(str::to_string)
        .unwrap_or_else(|| workflow::candidate_digest(fallback_key.as_bytes()));
    let fallback = devrail_workflows::last_known_good_for_target(
        pool,
        target.organization_id,
        target.environment_id,
    )
    .await?;
    let mut tx = pool.begin().await?;
    let first_occurrence = devrail_workflows::record_reload_failure(
        &mut tx,
        target,
        &candidate_digest,
        error.kind().as_str(),
    )
    .await?;
    if first_occurrence {
        audit_logs::record_actor(
            &mut tx,
            &system_actor(target),
            "devrail.workflow.reject",
            "devrail_environment",
            Some(target.environment_id),
            json!({
                "actorType": "system",
                "candidateDigest": candidate_digest,
                "errorKind": error.kind().as_str(),
                "fallbackAvailable": fallback.is_some(),
                "policyVersion": "devrail-policy-v1"
            }),
        )
        .await?;
    }
    tx.commit().await?;
    let outcome = if fallback.is_some() {
        "rejected_with_fallback"
    } else {
        "rejected_without_fallback"
    };
    crate::app_metrics::record_workflow_reload(outcome);
    tracing::warn!(
        organization_id = target.organization_id,
        environment_id = target.environment_id,
        candidate_digest = %short_digest(&candidate_digest),
        error_kind = error.kind().as_str(),
        fallback_available = fallback.is_some(),
        first_occurrence,
        "repository workflow candidate rejected"
    );
    Ok(())
}

fn system_actor(target: &devrail_workflows::WorkflowEnvironmentTarget) -> ActorContext {
    ActorContext {
        actor_type: ActorType::System,
        user_id: target.owner_user_id,
        session_id: 0,
        organization_id: target.organization_id,
        department_id: target.department_id,
        data_scope: DataScope::Organization,
        permission_codes: BTreeSet::new(),
    }
}

fn short_digest(digest: &str) -> &str {
    digest.get(..12).unwrap_or("invalid")
}

fn random_seed() -> u64 {
    let mut random = [0_u8; 8];
    if getrandom::fill(&mut random).is_ok() {
        u64::from_le_bytes(random)
    } else {
        0
    }
}

fn jittered_delay(policy: WorkflowReloaderPolicy, seed: u64) -> Duration {
    let base_millis = policy.poll_interval.as_millis().min(u128::from(u64::MAX)) as u64;
    let jitter_max = base_millis.saturating_mul(u64::from(policy.jitter_percent.min(100))) / 100;
    let jitter = if jitter_max == 0 {
        0
    } else {
        seed % jitter_max.saturating_add(1)
    };
    Duration::from_millis(base_millis.saturating_add(jitter))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_jitter_is_bounded() {
        let policy = WorkflowReloaderPolicy {
            poll_interval: Duration::from_secs(10),
            jitter_percent: 20,
        };
        assert_eq!(jittered_delay(policy, 0), Duration::from_secs(10));
        assert!(jittered_delay(policy, u64::MAX) <= Duration::from_secs(12));
    }

    #[test]
    fn logged_digest_is_bounded() {
        assert_eq!(short_digest(&"a".repeat(64)), "aaaaaaaaaaaa");
        assert_eq!(short_digest("bad"), "invalid");
    }
}
