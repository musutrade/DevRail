use crate::access::ActorContext;
use crate::models::{CreateDevRailTaskCommentRequest, DevRailTaskCommentRow};
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

const COLUMNS: &str = "c.id, c.organization_id, c.department_id, c.task_id, c.author_user_id, u.username AS author_username, u.display_name AS author_display_name, c.body, c.mentions, c.created_at, c.edited_at, c.deleted_at";

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
    sqlx::query_as::<_, DevRailTaskCommentRow>(AssertSqlSafe("INSERT INTO devrail_task_comments (organization_id,department_id,task_id,author_user_id,body,mentions) SELECT t.organization_id,t.department_id,t.id,$1,$2,$3 FROM devrail_tasks t WHERE t.id=$4 AND t.organization_id=$5 RETURNING id,organization_id,department_id,task_id,author_user_id,'' AS author_username,'' AS author_display_name,body,mentions,created_at,edited_at,deleted_at"))
        .bind(actor.user_id).bind(request.body.trim()).bind(mentions).bind(task_id).bind(actor.organization_id).fetch_one(c).await
}

pub async fn update(
    c: &mut PgConnection,
    actor: &ActorContext,
    id: i64,
    body: &str,
    mentions: &serde_json::Value,
) -> Result<Option<DevRailTaskCommentRow>, sqlx::Error> {
    sqlx::query_as::<_, DevRailTaskCommentRow>(AssertSqlSafe(format!("UPDATE devrail_task_comments c SET body=$1, mentions=$2, edited_at=now() FROM devrail_tasks t WHERE c.id=$3 AND c.task_id=t.id AND c.organization_id=$4 AND c.deleted_at IS NULL AND (c.author_user_id=$5 OR $6 IN ('all','organization')) RETURNING {COLUMNS}")))
        .bind(body).bind(mentions).bind(id).bind(actor.organization_id).bind(actor.user_id).bind(actor.data_scope.as_str()).fetch_optional(c).await
}

pub async fn soft_delete(
    c: &mut PgConnection,
    actor: &ActorContext,
    id: i64,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query("UPDATE devrail_task_comments SET deleted_at=now(), body='[评论已删除]', mentions='[]'::jsonb WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL AND (author_user_id=$3 OR $4 IN ('all','organization'))").bind(id).bind(actor.organization_id).bind(actor.user_id).bind(actor.data_scope.as_str()).execute(c).await?.rows_affected() == 1)
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

pub async fn task_project_id(
    connection: &mut PgConnection,
    organization_id: i64,
    task_id: i64,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT project_id FROM devrail_tasks
         WHERE id=$1 AND organization_id=$2 AND archived_at IS NULL",
    )
    .bind(task_id)
    .bind(organization_id)
    .fetch_optional(connection)
    .await
}

pub async fn hydrate(c: &mut PgConnection, id: i64) -> Result<DevRailTaskCommentRow, sqlx::Error> {
    sqlx::query_as::<_, DevRailTaskCommentRow>(AssertSqlSafe(format!("SELECT {COLUMNS} FROM devrail_task_comments c JOIN users u ON u.id=c.author_user_id WHERE c.id=$1"))).bind(id).fetch_one(c).await
}
