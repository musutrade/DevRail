use crate::access::ActorContext;
use crate::models::DevRailExternalReviewCommentResponse;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    review_id: i64,
) -> Result<Vec<DevRailExternalReviewCommentResponse>, sqlx::Error> {
    sqlx::query_as::<_, DevRailExternalReviewCommentResponse>(AssertSqlSafe("SELECT c.id,c.review_id,c.provider,c.external_id,c.file_path,c.line_start,c.line_end,c.body,c.author_name,c.external_created_at,c.created_at FROM devrail_external_review_comments c JOIN devrail_reviews r ON r.id=c.review_id WHERE c.review_id=$1 AND r.organization_id=$2 AND (r.requested_by=$3 OR r.reviewer_user_id=$3) ORDER BY c.created_at ASC,c.id ASC"))
        .bind(review_id).bind(actor.organization_id).bind(actor.user_id).fetch_all(pool).await
}
pub struct ExternalReviewCommentInput<'a> {
    pub review_id: i64,
    pub provider: &'a str,
    pub external_id: &'a str,
    pub file_path: &'a str,
    pub line_start: Option<i32>,
    pub line_end: Option<i32>,
    pub body: &'a str,
    pub author_name: &'a str,
    pub external_created_at: Option<chrono::DateTime<chrono::Utc>>,
}
pub async fn upsert(
    c: &mut PgConnection,
    input: &ExternalReviewCommentInput<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(AssertSqlSafe("INSERT INTO devrail_external_review_comments (organization_id,review_id,provider,external_id,file_path,line_start,line_end,body,author_name,external_created_at) SELECT r.organization_id,$1,$2,$3,$4,$5,$6,$7,$8,$9 FROM devrail_reviews r WHERE r.id=$1 ON CONFLICT (provider,external_id) DO UPDATE SET body=EXCLUDED.body,line_start=EXCLUDED.line_start,line_end=EXCLUDED.line_end"))
        .bind(input.review_id).bind(input.provider).bind(input.external_id).bind(input.file_path).bind(input.line_start).bind(input.line_end).bind(input.body).bind(input.author_name).bind(input.external_created_at).execute(&mut *c).await.map(|_| ())
}
