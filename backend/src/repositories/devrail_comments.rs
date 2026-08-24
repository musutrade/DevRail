use crate::access::ActorContext;
use crate::models::{CreateDevRailTaskCommentRequest, DevRailTaskCommentRow};
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

const COLUMNS: &str = "c.id, c.organization_id, c.department_id, c.task_id, c.author_user_id, u.username AS author_username, u.display_name AS author_display_name, c.body, c.mentions, c.created_at";

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: i64,
    page: i64,
    size: i64,
) -> Result<(Vec<DevRailTaskCommentRow>, i64), sqlx::Error> {
    let scope = "t.organization_id=$1 AND t.id=$2 AND (t.owner_user_id=$3 OR t.assignee_user_id=$3 OR $4 IN ('all','organization') OR ($4='department_and_children' AND t.department_id=$5))";
    let items = sqlx::query_as::<_, DevRailTaskCommentRow>(AssertSqlSafe(format!("SELECT {COLUMNS} FROM devrail_task_comments c JOIN devrail_tasks t ON t.id=c.task_id AND t.organization_id=c.organization_id JOIN users u ON u.id=c.author_user_id WHERE {scope} ORDER BY c.created_at ASC,c.id ASC LIMIT $6 OFFSET $7")))
        .bind(actor.organization_id).bind(task_id).bind(actor.user_id).bind(actor.data_scope.as_str()).bind(actor.department_id).bind(size).bind((page-1)*size).fetch_all(pool).await?;
    let (total,) = sqlx::query_as::<_, (i64,)>(AssertSqlSafe(format!("SELECT count(*) FROM devrail_task_comments c JOIN devrail_tasks t ON t.id=c.task_id AND t.organization_id=c.organization_id WHERE {scope}")))
        .bind(actor.organization_id).bind(task_id).bind(actor.user_id).bind(actor.data_scope.as_str()).bind(actor.department_id).fetch_one(pool).await?;
    Ok((items, total))
}

pub async fn create<'a>(
    c: &mut PgConnection,
    actor: &ActorContext,
    task_id: i64,
    request: &'a CreateDevRailTaskCommentRequest,
    mentions: &'a serde_json::Value,
) -> Result<DevRailTaskCommentRow, sqlx::Error> {
    sqlx::query_as::<_, DevRailTaskCommentRow>(AssertSqlSafe("INSERT INTO devrail_task_comments (organization_id,department_id,task_id,author_user_id,body,mentions) SELECT t.organization_id,t.department_id,t.id,$1,$2,$3 FROM devrail_tasks t WHERE t.id=$4 AND t.organization_id=$5 RETURNING id,organization_id,department_id,task_id,author_user_id,'' AS author_username,'' AS author_display_name,body,mentions,created_at"))
        .bind(actor.user_id).bind(request.body.trim()).bind(mentions).bind(task_id).bind(actor.organization_id).fetch_one(c).await
}

pub async fn mentioned_users(
    c: &mut PgConnection,
    organization_id: i64,
    usernames: &[String],
) -> Result<Vec<(i64, String)>, sqlx::Error> {
    sqlx::query_as::<_, (i64, String)>(
        "SELECT id,username FROM users WHERE organization_id=$1 AND username = ANY($2)",
    )
    .bind(organization_id)
    .bind(usernames)
    .fetch_all(c)
    .await
}

pub async fn hydrate(c: &mut PgConnection, id: i64) -> Result<DevRailTaskCommentRow, sqlx::Error> {
    sqlx::query_as::<_, DevRailTaskCommentRow>(AssertSqlSafe(format!("SELECT {COLUMNS} FROM devrail_task_comments c JOIN users u ON u.id=c.author_user_id WHERE c.id=$1"))).bind(id).fetch_one(c).await
}
