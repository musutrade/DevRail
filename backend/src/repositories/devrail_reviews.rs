use crate::access::ActorContext;
use crate::models::DevRailReviewRow;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};
const COLUMNS: &str = "id,organization_id,department_id,task_id,run_id,requested_by,reviewer_user_id,status,summary,decision_reason,decided_at,created_at,updated_at";
pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    page: i64,
    size: i64,
) -> Result<(Vec<DevRailReviewRow>, i64), sqlx::Error> {
    let sql=format!("SELECT {COLUMNS} FROM devrail_reviews WHERE organization_id=$1 AND (reviewer_user_id=$2 OR requested_by=$2) ORDER BY created_at DESC,id DESC LIMIT $3 OFFSET $4");
    let items = sqlx::query_as::<_, DevRailReviewRow>(AssertSqlSafe(sql))
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(size)
        .bind((page - 1) * size)
        .fetch_all(pool)
        .await?;
    let total=sqlx::query_scalar::<_,i64>("SELECT count(*) FROM devrail_reviews WHERE organization_id=$1 AND (reviewer_user_id=$2 OR requested_by=$2)").bind(actor.organization_id).bind(actor.user_id).fetch_one(pool).await?;
    Ok((items, total))
}
pub async fn create(
    c: &mut PgConnection,
    actor: &ActorContext,
    run_id: i64,
    reviewer: i64,
    summary: Option<&str>,
) -> Result<DevRailReviewRow, sqlx::Error> {
    let sql=format!("WITH RECURSIVE visible_departments AS (SELECT id FROM departments WHERE id=$7 AND organization_id=$5 UNION SELECT child.id FROM departments child JOIN visible_departments parent ON child.parent_id=parent.id WHERE child.organization_id=$5) INSERT INTO devrail_reviews (organization_id,department_id,task_id,run_id,requested_by,reviewer_user_id,summary) SELECT r.organization_id,r.department_id,r.task_id,r.id,$1,$2,$3 FROM devrail_runs r JOIN users reviewer ON reviewer.id=$2 AND reviewer.organization_id=$5 AND reviewer.deleted_at IS NULL AND ($6 IN ('all','organization') OR $6='self' AND reviewer.id=$1 OR $6='department' AND reviewer.department_id=$7 OR $6='department_and_children' AND reviewer.department_id IN (SELECT id FROM visible_departments)) WHERE r.id=$4 AND r.organization_id=$5 RETURNING {COLUMNS}");
    sqlx::query_as::<_, DevRailReviewRow>(AssertSqlSafe(sql))
        .bind(actor.user_id)
        .bind(reviewer)
        .bind(summary)
        .bind(run_id)
        .bind(actor.organization_id)
        .bind(actor.data_scope.as_str())
        .bind(actor.department_id)
        .fetch_one(&mut *c)
        .await
}
pub async fn decide(
    c: &mut PgConnection,
    actor: &ActorContext,
    id: i64,
    decision: &str,
    reason: Option<&str>,
) -> Result<Option<DevRailReviewRow>, sqlx::Error> {
    let sql=format!("UPDATE devrail_reviews SET status=$1,decision_reason=$2,decided_at=now(),updated_at=now() WHERE id=$3 AND organization_id=$4 AND reviewer_user_id=$5 AND status='pending' RETURNING {COLUMNS}");
    sqlx::query_as::<_, DevRailReviewRow>(AssertSqlSafe(sql))
        .bind(decision)
        .bind(reason)
        .bind(id)
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .fetch_optional(&mut *c)
        .await
}
