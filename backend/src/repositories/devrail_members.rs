use crate::access::ActorContext;
use crate::models::DevRailProjectMemberRow;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

const COLUMNS: &str = "m.id, m.organization_id, m.department_id, m.owner_user_id, m.project_id, m.user_id, u.username, u.display_name, m.role, m.joined_at, m.revoked_at";

fn visible_departments_cte() -> &'static str {
    "WITH RECURSIVE visible_departments AS (
         SELECT id FROM departments WHERE id=$4 AND organization_id=$2
         UNION
         SELECT child.id FROM departments child
         JOIN visible_departments parent ON child.parent_id=parent.id
         WHERE child.organization_id=$2
     )"
}

fn scope(alias: &str) -> String {
    format!("{alias}.organization_id = $2 AND ($1 = 'all' OR $1 = 'organization' OR ($1 = 'self' AND {alias}.owner_user_id = $3) OR ($1 = 'department' AND {alias}.department_id = $4) OR ($1 = 'department_and_children' AND {alias}.department_id IN (SELECT id FROM visible_departments)))")
}

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
) -> Result<Vec<DevRailProjectMemberRow>, sqlx::Error> {
    let sql = format!("{} SELECT {COLUMNS} FROM devrail_project_members m JOIN users u ON u.id = m.user_id AND u.organization_id = m.organization_id AND u.deleted_at IS NULL WHERE m.project_id=$5 AND m.revoked_at IS NULL AND {} ORDER BY m.role, u.display_name, m.id", visible_departments_cte(), scope("m"));
    sqlx::query_as::<_, DevRailProjectMemberRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(project_id)
        .fetch_all(pool)
        .await
}

pub async fn add(
    connection: &mut PgConnection,
    actor: &ActorContext,
    project_id: i64,
    user_id: i64,
    role: &str,
) -> Result<DevRailProjectMemberRow, sqlx::Error> {
    let sql = format!(
        "{}
         INSERT INTO devrail_project_members
             (organization_id, department_id, owner_user_id, project_id, user_id, role)
         SELECT p.organization_id, p.department_id, $3, p.id, u.id, $6
         FROM devrail_projects p
         JOIN users u ON u.id=$5 AND u.organization_id=p.organization_id AND u.deleted_at IS NULL
         WHERE p.id=$7 AND p.archived_at IS NULL AND {}
           AND u.organization_id=$2 AND ($1='all' OR
               $1='organization'
               OR $1='self' AND u.id=$3
               OR $1='department' AND u.department_id=$4
               OR $1='department_and_children'
                  AND u.department_id IN (SELECT id FROM visible_departments)
           )
         ON CONFLICT (project_id,user_id) DO UPDATE
         SET department_id=EXCLUDED.department_id,
             owner_user_id=EXCLUDED.owner_user_id,
             role=EXCLUDED.role,
             revoked_at=NULL
         RETURNING id, organization_id, department_id, owner_user_id, project_id, user_id,
             (SELECT username FROM users
              WHERE id=devrail_project_members.user_id
                AND organization_id=devrail_project_members.organization_id),
             (SELECT display_name FROM users
              WHERE id=devrail_project_members.user_id
                AND organization_id=devrail_project_members.organization_id),
             role, joined_at, revoked_at",
        visible_departments_cte(),
        scope("p")
    );
    sqlx::query_as::<_, DevRailProjectMemberRow>(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(user_id)
        .bind(role)
        .bind(project_id)
        .fetch_one(connection)
        .await
}

pub async fn revoke(
    connection: &mut PgConnection,
    actor: &ActorContext,
    project_id: i64,
    user_id: i64,
) -> Result<bool, sqlx::Error> {
    let sql = format!(
        "{} UPDATE devrail_project_members m SET revoked_at=now()
         WHERE m.project_id=$5 AND m.user_id=$6 AND m.revoked_at IS NULL
           AND {}
           AND NOT EXISTS (
               SELECT 1 FROM devrail_project_members keep
               WHERE keep.project_id=m.project_id AND keep.user_id=m.user_id
                 AND keep.role='owner' AND keep.revoked_at IS NULL
           )",
        visible_departments_cte(),
        scope("m")
    );
    let result = sqlx::query(AssertSqlSafe(sql))
        .bind(actor.data_scope.as_str())
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(actor.department_id)
        .bind(project_id)
        .bind(user_id)
        .execute(connection)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::DataScope;
    use crate::repositories::{devrail, devrail_reviews};
    use serde_json::json;
    use uuid::Uuid;

    async fn create_user(
        pool: &PgPool,
        organization_id: i64,
        department_id: i64,
        label: &str,
    ) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO users
                 (username,password_hash,display_name,organization_id,department_id)
             VALUES ($1,'test',$2,$3,$4) RETURNING id",
        )
        .bind(format!("{label}-{}", Uuid::new_v4().simple()))
        .bind(label)
        .bind(organization_id)
        .bind(department_id)
        .fetch_one(pool)
        .await
        .expect("create scoped user")
    }

    #[tokio::test]
    async fn member_assignee_and_reviewer_writes_enforce_actor_scope() {
        let Ok(schema_fixture) = crate::db::test_schema_pool().await else {
            return;
        };
        let pool = schema_fixture.pool().clone();
        let fixture =
            crate::repositories::devrail_continuations::integration_tests::fixture(&pool).await;
        let actor_department_id = fixture.actor.department_id.expect("actor department");
        let same_org_user = create_user(
            &pool,
            fixture.actor.organization_id,
            actor_department_id,
            "同组织用户",
        )
        .await;
        let sibling_department_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO departments (organization_id,code,name)
             VALUES ($1,$2,'同组织其他部门') RETURNING id",
        )
        .bind(fixture.actor.organization_id)
        .bind(format!("sibling-{}", Uuid::new_v4().simple()))
        .fetch_one(&pool)
        .await
        .expect("create sibling department");
        let sibling_user = create_user(
            &pool,
            fixture.actor.organization_id,
            sibling_department_id,
            "同组织其他部门用户",
        )
        .await;
        let other_organization_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO organizations (code,name)
             VALUES ($1,'其他组织') RETURNING id",
        )
        .bind(format!("other-{}", Uuid::new_v4().simple()))
        .fetch_one(&pool)
        .await
        .expect("create other organization");
        let other_department_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO departments (organization_id,code,name)
             VALUES ($1,'root','其他组织根部门') RETURNING id",
        )
        .bind(other_organization_id)
        .fetch_one(&pool)
        .await
        .expect("create other department");
        let other_user = create_user(
            &pool,
            other_organization_id,
            other_department_id,
            "其他组织用户",
        )
        .await;

        let mut transaction = pool.begin().await.expect("begin organization-scope writes");
        let member = add(
            &mut transaction,
            &fixture.actor,
            fixture.project_id,
            same_org_user,
            "developer",
        )
        .await
        .expect("add visible member");
        assert_eq!(member.user_id, same_org_user);
        assert!(add(
            &mut transaction,
            &fixture.actor,
            fixture.project_id,
            other_user,
            "developer",
        )
        .await
        .is_err());
        let all_scope_actor = ActorContext {
            data_scope: DataScope::All,
            ..fixture.actor.clone()
        };
        assert!(add(
            &mut transaction,
            &all_scope_actor,
            fixture.project_id,
            other_user,
            "developer",
        )
        .await
        .is_err());

        let task = devrail::create_task(
            &mut transaction,
            &fixture.actor,
            &devrail::NewTask {
                owner_user_id: fixture.actor.user_id,
                project_id: fixture.project_id,
                repository_id: Some(fixture.repository_id),
                environment_id: Some(fixture.environment_id),
                assignee_user_id: Some(same_org_user),
                title: "授权范围测试任务",
                goal: "验证负责人范围",
                background: None,
                acceptance_criteria: None,
                constraints: None,
                priority: "normal",
                labels: &json!([]),
                due_at: None,
                department_id: fixture.actor.department_id,
                creation_source: "manual",
                source_task_id: None,
                source_run_id: None,
                followup_depth: 0,
            },
        )
        .await
        .expect("create task with visible assignee");
        assert_eq!(task.assignee_user_id, Some(same_org_user));
        assert!(devrail::create_task(
            &mut transaction,
            &fixture.actor,
            &devrail::NewTask {
                owner_user_id: fixture.actor.user_id,
                project_id: fixture.project_id,
                repository_id: Some(fixture.repository_id),
                environment_id: Some(fixture.environment_id),
                assignee_user_id: Some(other_user),
                title: "跨组织负责人测试",
                goal: "必须拒绝",
                background: None,
                acceptance_criteria: None,
                constraints: None,
                priority: "normal",
                labels: &json!([]),
                due_at: None,
                department_id: fixture.actor.department_id,
                creation_source: "manual",
                source_task_id: None,
                source_run_id: None,
                followup_depth: 0,
            },
        )
        .await
        .is_err());
        assert!(devrail_reviews::create(
            &mut transaction,
            &fixture.actor,
            fixture.source_run_id,
            other_user,
            None,
        )
        .await
        .is_err());
        let review = devrail_reviews::create(
            &mut transaction,
            &fixture.actor,
            fixture.source_run_id,
            same_org_user,
            None,
        )
        .await
        .expect("create review with visible reviewer");
        assert_eq!(review.reviewer_user_id, same_org_user);
        transaction
            .rollback()
            .await
            .expect("rollback organization-scope writes");

        let department_actor = ActorContext {
            data_scope: DataScope::Department,
            ..fixture.actor.clone()
        };
        let mut department_tx = pool.begin().await.expect("begin department-scope writes");
        assert!(add(
            &mut department_tx,
            &department_actor,
            fixture.project_id,
            sibling_user,
            "developer",
        )
        .await
        .is_err());
        assert!(devrail_reviews::create(
            &mut department_tx,
            &department_actor,
            fixture.source_run_id,
            sibling_user,
            None,
        )
        .await
        .is_err());
        department_tx
            .rollback()
            .await
            .expect("rollback department-scope writes");

        drop(pool);
        schema_fixture
            .cleanup()
            .await
            .expect("cleanup authorization schema");
    }
}
