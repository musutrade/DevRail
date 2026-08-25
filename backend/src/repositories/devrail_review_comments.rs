use crate::{
    access::ActorContext,
    models::{CreateDevRailReviewCommentRequest, DevRailReviewCommentRow},
};
use sqlx::{AssertSqlSafe, PgConnection, PgPool};
const COLUMNS:&str="c.id,c.organization_id,c.review_id,c.author_user_id,c.file_path,c.line_start,c.line_end,c.body,c.created_at,c.updated_at";
const SCOPE:&str="EXISTS (SELECT 1 FROM devrail_reviews r WHERE r.id=c.review_id AND r.organization_id=$1 AND (r.requested_by=$2 OR r.reviewer_user_id=$2))";
pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    review_id: i64,
) -> Result<Vec<DevRailReviewCommentRow>, sqlx::Error> {
    let sql=format!("SELECT {COLUMNS} FROM devrail_review_comments c WHERE c.review_id=$3 AND {SCOPE} ORDER BY c.created_at ASC,c.id ASC");
    sqlx::query_as::<_, DevRailReviewCommentRow>(AssertSqlSafe(sql))
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .bind(review_id)
        .fetch_all(pool)
        .await
}
pub async fn create(
    c: &mut PgConnection,
    actor: &ActorContext,
    review_id: i64,
    req: &CreateDevRailReviewCommentRequest,
) -> Result<DevRailReviewCommentRow, sqlx::Error> {
    let sql=format!("INSERT INTO devrail_review_comments (organization_id,review_id,author_user_id,file_path,line_start,line_end,body) SELECT r.organization_id,r.id,$2,$3,$4,$5,$6 FROM devrail_reviews r WHERE r.id=$1 AND r.organization_id=$7 AND (r.requested_by=$2 OR r.reviewer_user_id=$2) RETURNING {COLUMNS}");
    sqlx::query_as::<_, DevRailReviewCommentRow>(AssertSqlSafe(sql))
        .bind(review_id)
        .bind(actor.user_id)
        .bind(&req.file_path)
        .bind(req.line_start)
        .bind(req.line_end)
        .bind(&req.body)
        .bind(actor.organization_id)
        .fetch_one(&mut *c)
        .await
}
pub async fn update(
    c: &mut PgConnection,
    actor: &ActorContext,
    id: i64,
    body: &str,
) -> Result<Option<DevRailReviewCommentRow>, sqlx::Error> {
    let sql=format!("UPDATE devrail_review_comments c SET body=$1,updated_at=now() WHERE c.id=$2 AND c.organization_id=$3 AND c.author_user_id=$4 AND EXISTS (SELECT 1 FROM devrail_reviews r WHERE r.id=c.review_id AND r.organization_id=$3 AND (r.requested_by=$4 OR r.reviewer_user_id=$4)) RETURNING {COLUMNS}");
    sqlx::query_as::<_, DevRailReviewCommentRow>(AssertSqlSafe(sql))
        .bind(body)
        .bind(id)
        .bind(actor.organization_id)
        .bind(actor.user_id)
        .fetch_optional(&mut *c)
        .await
}
