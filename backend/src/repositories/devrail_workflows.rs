//! Scoped persistence for accepted repository workflows and reload failures.

use crate::access::ActorContext;
use serde_json::Value;
use sqlx::{PgConnection, PgPool};

#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct WorkflowEnvironmentTarget {
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub environment_id: i64,
    pub workspace_root: String,
    pub network_mode: String,
    pub tool_policy: Value,
    pub max_duration_secs: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct WorkflowVersionRow {
    pub id: i64,
    pub source: String,
    pub declared_version: String,
    pub digest: String,
    pub normalized_snapshot: Value,
}

pub(crate) struct NewWorkflowVersion<'a> {
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub environment_id: i64,
    pub source: &'a str,
    pub declared_version: &'a str,
    pub digest: &'a str,
    pub normalized_snapshot: &'a Value,
    pub prompt_body: &'a str,
}

pub(crate) async fn list_target_organizations(pool: &PgPool) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT DISTINCT organization_id
         FROM devrail_environments
         WHERE enabled AND archived_at IS NULL
         ORDER BY organization_id",
    )
    .fetch_all(pool)
    .await
}

pub(crate) async fn list_targets_for_organization(
    pool: &PgPool,
    organization_id: i64,
) -> Result<Vec<WorkflowEnvironmentTarget>, sqlx::Error> {
    sqlx::query_as(
        "SELECT organization_id, department_id, owner_user_id,
                id AS environment_id, workspace_root,
                network_mode, tool_policy, max_duration_secs
         FROM devrail_environments
         WHERE organization_id = $1 AND enabled AND archived_at IS NULL
         ORDER BY id",
    )
    .bind(organization_id)
    .fetch_all(pool)
    .await
}

pub(crate) async fn accept_version(
    connection: &mut PgConnection,
    input: &NewWorkflowVersion<'_>,
) -> Result<WorkflowVersionRow, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO devrail_workflow_versions
             (organization_id, department_id, owner_user_id, environment_id,
              source, declared_version, digest, normalized_snapshot, prompt_body)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
         ON CONFLICT (organization_id, environment_id, source, digest)
         DO UPDATE SET digest = EXCLUDED.digest
         RETURNING id, source, declared_version, digest,
                   normalized_snapshot",
    )
    .bind(input.organization_id)
    .bind(input.department_id)
    .bind(input.owner_user_id)
    .bind(input.environment_id)
    .bind(input.source)
    .bind(input.declared_version)
    .bind(input.digest)
    .bind(input.normalized_snapshot)
    .bind(input.prompt_body)
    .fetch_one(connection)
    .await
}

pub(crate) async fn last_known_good(
    pool: &PgPool,
    actor: &ActorContext,
    environment_id: i64,
) -> Result<Option<WorkflowVersionRow>, sqlx::Error> {
    sqlx::query_as(
        "WITH RECURSIVE visible_departments AS (
             SELECT id FROM departments
             WHERE id=$4 AND organization_id=$2
             UNION
             SELECT child.id FROM departments child
             JOIN visible_departments parent ON child.parent_id=parent.id
             WHERE child.organization_id=$2
         )
         SELECT id, source, declared_version, digest,
                normalized_snapshot
         FROM devrail_workflow_versions w
         WHERE w.environment_id=$5 AND w.organization_id=$2
           AND ($1='all' OR $1='organization'
                OR ($1='self' AND w.owner_user_id=$3)
                OR ($1='department' AND w.department_id=$4)
                OR ($1='department_and_children'
                    AND w.department_id IN (SELECT id FROM visible_departments)))
         ORDER BY accepted_at DESC, id DESC
         LIMIT 1",
    )
    .bind(actor.data_scope.as_str())
    .bind(actor.organization_id)
    .bind(actor.user_id)
    .bind(actor.department_id)
    .bind(environment_id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn last_known_good_for_target(
    pool: &PgPool,
    organization_id: i64,
    environment_id: i64,
) -> Result<Option<WorkflowVersionRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, source, declared_version, digest,
                normalized_snapshot
         FROM devrail_workflow_versions
         WHERE organization_id=$1 AND environment_id=$2
         ORDER BY accepted_at DESC, id DESC
         LIMIT 1",
    )
    .bind(organization_id)
    .bind(environment_id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn record_reload_failure(
    connection: &mut PgConnection,
    target: &WorkflowEnvironmentTarget,
    candidate_digest: &str,
    error_kind: &str,
) -> Result<bool, sqlx::Error> {
    let inserted = sqlx::query_scalar::<_, i64>(
        "INSERT INTO devrail_workflow_reload_failures
             (organization_id, department_id, owner_user_id, environment_id,
              candidate_digest, error_kind)
         VALUES ($1,$2,$3,$4,$5,$6)
         ON CONFLICT (organization_id, environment_id, candidate_digest, error_kind)
         DO NOTHING
         RETURNING id",
    )
    .bind(target.organization_id)
    .bind(target.department_id)
    .bind(target.owner_user_id)
    .bind(target.environment_id)
    .bind(candidate_digest)
    .bind(error_kind)
    .fetch_optional(&mut *connection)
    .await?;
    if inserted.is_some() {
        return Ok(true);
    }
    sqlx::query(
        "UPDATE devrail_workflow_reload_failures
         SET occurrence_count=occurrence_count+1, last_seen_at=now()
         WHERE organization_id=$1 AND environment_id=$2
           AND candidate_digest=$3 AND error_kind=$4",
    )
    .bind(target.organization_id)
    .bind(target.environment_id)
    .bind(candidate_digest)
    .bind(error_kind)
    .execute(connection)
    .await?;
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::{ActorContext, ActorType, DataScope};
    use crate::db::test_schema_pool;
    use serde_json::json;
    use std::collections::BTreeSet;
    use uuid::Uuid;

    async fn workflow_target(pool: &PgPool) -> WorkflowEnvironmentTarget {
        let (owner_user_id, organization_id, department_id) =
            sqlx::query_as::<_, (i64, i64, Option<i64>)>(
                "SELECT id, organization_id, department_id FROM users ORDER BY id LIMIT 1",
            )
            .fetch_one(pool)
            .await
            .expect("seeded user");
        let suffix = Uuid::new_v4().simple().to_string();
        let project_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO devrail_projects
                 (organization_id, department_id, owner_user_id, slug, name)
             VALUES ($1,$2,$3,$4,$5) RETURNING id",
        )
        .bind(organization_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(format!("workflow-{suffix}"))
        .bind("Workflow 持久化测试")
        .fetch_one(pool)
        .await
        .expect("create project");
        let environment_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO devrail_environments
                 (organization_id, department_id, owner_user_id, project_id,
                  name, workspace_root)
             VALUES ($1,$2,$3,$4,$5,$6) RETURNING id",
        )
        .bind(organization_id)
        .bind(department_id)
        .bind(owner_user_id)
        .bind(project_id)
        .bind(format!("workflow-{suffix}"))
        .bind(format!("/tmp/devrail-workflow-{suffix}"))
        .fetch_one(pool)
        .await
        .expect("create environment");
        WorkflowEnvironmentTarget {
            organization_id,
            department_id,
            owner_user_id,
            environment_id,
            workspace_root: format!("/tmp/devrail-workflow-{suffix}"),
            network_mode: "off".to_string(),
            tool_policy: json!({}),
            max_duration_secs: 3_600,
        }
    }

    #[tokio::test]
    async fn workflow_versions_and_failures_are_deduplicated_and_scoped() {
        let Ok(fixture) = test_schema_pool().await else {
            return;
        };
        let pool = fixture.pool();
        let target = workflow_target(pool).await;
        let snapshot = json!({"source":"repository","declaredVersion":"v1","digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"});
        let input = NewWorkflowVersion {
            organization_id: target.organization_id,
            department_id: target.department_id,
            owner_user_id: target.owner_user_id,
            environment_id: target.environment_id,
            source: "repository",
            declared_version: "v1",
            digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            normalized_snapshot: &snapshot,
            prompt_body: "执行测试",
        };
        let mut tx = pool.begin().await.expect("begin workflow transaction");
        let first = accept_version(&mut tx, &input)
            .await
            .expect("first version");
        let duplicate = accept_version(&mut tx, &input)
            .await
            .expect("duplicate version");
        assert_eq!(first.id, duplicate.id);
        assert!(record_reload_failure(
            &mut tx,
            &target,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "schema",
        )
        .await
        .expect("first failure"));
        assert!(!record_reload_failure(
            &mut tx,
            &target,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "schema",
        )
        .await
        .expect("duplicate failure"));
        tx.commit().await.expect("commit workflow transaction");

        let actor = ActorContext {
            actor_type: ActorType::System,
            user_id: target.owner_user_id,
            session_id: 0,
            organization_id: target.organization_id,
            department_id: target.department_id,
            data_scope: DataScope::Organization,
            permission_codes: BTreeSet::new(),
        };
        let found = last_known_good(pool, &actor, target.environment_id)
            .await
            .expect("scoped workflow lookup")
            .expect("workflow exists");
        assert_eq!(found.digest, input.digest);
        let other_actor = ActorContext {
            organization_id: target.organization_id + 1,
            ..actor
        };
        assert!(last_known_good(pool, &other_actor, target.environment_id)
            .await
            .expect("cross organization lookup")
            .is_none());
        let occurrence_count = sqlx::query_scalar::<_, i64>(
            "SELECT occurrence_count FROM devrail_workflow_reload_failures
             WHERE organization_id=$1 AND environment_id=$2",
        )
        .bind(target.organization_id)
        .bind(target.environment_id)
        .fetch_one(pool)
        .await
        .expect("failure occurrence count");
        assert_eq!(occurrence_count, 2);
        fixture.cleanup().await.expect("cleanup workflow schema");
    }

    #[tokio::test]
    async fn reload_persists_last_known_good_and_deduplicates_failures() {
        let Ok(fixture) = test_schema_pool().await else {
            return;
        };
        let pool = fixture.pool();
        let target = workflow_target(pool).await;
        let controlled_root =
            std::env::temp_dir().join(format!("devrail-reloader-{}", Uuid::new_v4()));
        let workspace = controlled_root.join("repository");
        tokio::fs::create_dir_all(&workspace)
            .await
            .expect("create workflow workspace");
        sqlx::query(
            "UPDATE devrail_environments SET workspace_root=$1
             WHERE organization_id=$2 AND id=$3",
        )
        .bind(workspace.to_string_lossy().as_ref())
        .bind(target.organization_id)
        .bind(target.environment_id)
        .execute(pool)
        .await
        .expect("bind controlled workspace");
        let valid = include_str!("../../../WORKFLOW.md");
        tokio::fs::write(workspace.join("WORKFLOW.md"), valid)
            .await
            .expect("write valid workflow");
        crate::workers::workflow_reloader::reload_once(pool, &controlled_root)
            .await
            .expect("accept valid workflow");
        let first = last_known_good_for_target(pool, target.organization_id, target.environment_id)
            .await
            .expect("last known good")
            .expect("accepted workflow");
        assert_eq!(first.source, "repository");

        let invalid = valid.replacen("version:", "unknown: true\nversion:", 1);
        tokio::fs::write(workspace.join("WORKFLOW.md"), invalid)
            .await
            .expect("write invalid workflow");
        crate::workers::workflow_reloader::reload_once(pool, &controlled_root)
            .await
            .expect("invalid workflow retains fallback");
        crate::workers::workflow_reloader::reload_once(pool, &controlled_root)
            .await
            .expect("repeated invalid workflow is idempotent");
        let still_valid =
            last_known_good_for_target(pool, target.organization_id, target.environment_id)
                .await
                .expect("fallback query")
                .expect("fallback exists");
        assert_eq!(still_valid.id, first.id);
        let failure = sqlx::query_as::<_, (i64, Option<i64>)>(
            "SELECT count(*), max(occurrence_count)
             FROM devrail_workflow_reload_failures
             WHERE organization_id=$1 AND environment_id=$2",
        )
        .bind(target.organization_id)
        .bind(target.environment_id)
        .fetch_one(pool)
        .await
        .expect("reload failure evidence");
        assert_eq!(failure, (1, Some(2)));
        let rejection_audits = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM audit_logs
             WHERE action='devrail.workflow.reject'
               AND target_type='devrail_environment' AND target_id=$1",
        )
        .bind(target.environment_id)
        .fetch_one(pool)
        .await
        .expect("rejection audit count");
        assert_eq!(rejection_audits, 1);

        tokio::fs::remove_file(workspace.join("WORKFLOW.md"))
            .await
            .expect("remove workflow");
        crate::workers::workflow_reloader::reload_once(pool, &controlled_root)
            .await
            .expect("load safe default after deletion");
        let default_version =
            last_known_good_for_target(pool, target.organization_id, target.environment_id)
                .await
                .expect("default query")
                .expect("default exists");
        assert_eq!(default_version.source, "default");
        tokio::fs::remove_dir_all(&controlled_root)
            .await
            .expect("cleanup workflow workspace");
        fixture.cleanup().await.expect("cleanup workflow schema");
    }
}
