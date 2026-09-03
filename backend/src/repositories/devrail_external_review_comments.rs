use crate::access::ActorContext;
use crate::models::DevRailExternalReviewCommentResponse;
use sqlx::{AssertSqlSafe, PgConnection, PgPool};

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    review_id: i64,
) -> Result<Vec<DevRailExternalReviewCommentResponse>, sqlx::Error> {
    sqlx::query_as::<_, DevRailExternalReviewCommentResponse>(AssertSqlSafe("SELECT c.id,c.review_id,c.provider,c.external_id,c.file_path,c.line_start,c.line_end,c.body,c.author_name,c.external_created_at,c.created_at,c.resolved,c.deleted_at,EXISTS (SELECT 1 FROM devrail_reviews cr JOIN devrail_run_events ce ON ce.run_id=cr.run_id WHERE cr.id=c.review_id AND ce.event_type='file_change' AND ce.payload->>'path'=c.file_path) AS changeset_matched FROM devrail_external_review_comments c JOIN devrail_reviews r ON r.id=c.review_id WHERE c.review_id=$1 AND r.organization_id=$2 AND (r.requested_by=$3 OR r.reviewer_user_id=$3) ORDER BY c.created_at ASC,c.id ASC"))
        .bind(review_id).bind(actor.organization_id).bind(actor.user_id).fetch_all(pool).await
}
pub struct ExternalReviewCommentInput<'a> {
    pub organization_id: i64,
    pub review_id: i64,
    pub provider: &'a str,
    pub external_id: &'a str,
    pub file_path: &'a str,
    pub line_start: Option<i32>,
    pub line_end: Option<i32>,
    pub body: &'a str,
    pub author_name: &'a str,
    pub external_created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub resolved: bool,
    pub deleted: bool,
}

/// Resolve the review and its requested repository in one scoped query before
/// contacting the external provider.  The participant check mirrors `list`
/// so a caller cannot use an otherwise visible repository to write into a
/// different review.
pub async fn sync_target(
    pool: &PgPool,
    actor: &ActorContext,
    review_id: i64,
    project_id: i64,
    repository_id: i64,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT r.organization_id
         FROM devrail_reviews r
         JOIN devrail_tasks t
           ON t.id = r.task_id AND t.organization_id = r.organization_id
         JOIN devrail_repositories repo
           ON repo.id = t.repository_id AND repo.organization_id = t.organization_id
         WHERE r.id = $1
           AND r.organization_id = $2
           AND (r.requested_by = $3 OR r.reviewer_user_id = $3)
           AND t.project_id = $4
           AND t.repository_id = $5
           AND repo.archived_at IS NULL",
    )
    .bind(review_id)
    .bind(actor.organization_id)
    .bind(actor.user_id)
    .bind(project_id)
    .bind(repository_id)
    .fetch_optional(pool)
    .await
}

pub async fn upsert(
    c: &mut PgConnection,
    input: &ExternalReviewCommentInput<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(AssertSqlSafe("INSERT INTO devrail_external_review_comments (organization_id,review_id,provider,external_id,file_path,line_start,line_end,body,author_name,external_created_at,resolved,deleted_at) SELECT r.organization_id,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,CASE WHEN $12 THEN now() ELSE NULL END FROM devrail_reviews r WHERE r.id=$2 AND r.organization_id=$1 ON CONFLICT (organization_id,provider,external_id) DO UPDATE SET review_id=EXCLUDED.review_id,body=EXCLUDED.body,line_start=EXCLUDED.line_start,line_end=EXCLUDED.line_end,resolved=EXCLUDED.resolved,deleted_at=EXCLUDED.deleted_at"))
        .bind(input.organization_id).bind(input.review_id).bind(input.provider).bind(input.external_id).bind(input.file_path).bind(input.line_start).bind(input.line_end).bind(input.body).bind(input.author_name).bind(input.external_created_at).bind(input.resolved).bind(input.deleted).execute(&mut *c).await.map(|_| ())
}

pub async fn mark_missing_deleted(
    c: &mut PgConnection,
    organization_id: i64,
    review_id: i64,
    provider: &str,
    ids: &[String],
) -> Result<(), sqlx::Error> {
    if ids.is_empty() {
        return sqlx::query(AssertSqlSafe("UPDATE devrail_external_review_comments c SET deleted_at=now() FROM devrail_reviews r WHERE c.review_id=$1 AND c.organization_id=$2 AND c.provider=$3 AND c.deleted_at IS NULL AND r.id=c.review_id AND r.organization_id=c.organization_id"))
            .bind(review_id).bind(organization_id).bind(provider).execute(&mut *c).await.map(|_| ());
    }
    sqlx::query(AssertSqlSafe("UPDATE devrail_external_review_comments c SET deleted_at=now() FROM devrail_reviews r WHERE c.review_id=$1 AND c.organization_id=$2 AND c.provider=$3 AND c.external_id <> ALL($4) AND c.deleted_at IS NULL AND r.id=c.review_id AND r.organization_id=c.organization_id"))
        .bind(review_id).bind(organization_id).bind(provider).bind(ids).execute(&mut *c).await.map(|_| ())
}
