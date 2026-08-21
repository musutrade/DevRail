//! Organization lookup Repository used by system bootstrap operations.

use sqlx::PgPool;

pub async fn default_assignment(pool: &PgPool) -> Result<(i64, Option<i64>), sqlx::Error> {
    sqlx::query_as(
        "SELECT o.id, d.id
         FROM organizations o
         LEFT JOIN departments d
           ON d.organization_id = o.id AND d.code = 'root' AND d.status = 'active'
         WHERE o.code = 'default' AND o.status = 'active'",
    )
    .fetch_one(pool)
    .await
}
