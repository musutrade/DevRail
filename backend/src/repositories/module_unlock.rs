//! Repository for short-lived module unlocks bound to one authenticated session.

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};

pub async fn active_expires_at(
    pool: &PgPool,
    session_id: i64,
    user_id: i64,
    module: &str,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT expires_at FROM auth_module_unlocks
         WHERE session_id = $1
           AND user_id = $2
           AND module_scope = $3
           AND expires_at > now()",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(module)
    .fetch_optional(pool)
    .await
}

pub async fn upsert(
    connection: &mut PgConnection,
    session_id: i64,
    user_id: i64,
    module: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO auth_module_unlocks
             (session_id, user_id, module_scope, expires_at)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (session_id, module_scope) DO UPDATE
         SET user_id = EXCLUDED.user_id,
             issued_at = now(),
             expires_at = EXCLUDED.expires_at",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(module)
    .bind(expires_at)
    .execute(connection)
    .await?;
    Ok(())
}

pub async fn prune(connection: &mut PgConnection) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM auth_module_unlocks WHERE expires_at < now()")
        .execute(connection)
        .await?;
    Ok(result.rows_affected())
}
