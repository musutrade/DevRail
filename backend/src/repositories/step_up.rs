//! Repository for short-lived step-up authentication grants.

use chrono::{DateTime, Utc};
use sqlx::PgConnection;

pub async fn create(
    connection: &mut PgConnection,
    token_hash: &str,
    session_id: i64,
    user_id: i64,
    scope: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO auth_step_up_tokens
             (token_hash, session_id, user_id, scope, expires_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(token_hash)
    .bind(session_id)
    .bind(user_id)
    .bind(scope)
    .bind(expires_at)
    .execute(connection)
    .await?;
    Ok(())
}

pub async fn consume(
    connection: &mut PgConnection,
    token_hash: &str,
    session_id: i64,
    user_id: i64,
    scope: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE auth_step_up_tokens
         SET consumed_at = now()
         WHERE token_hash = $1
           AND session_id = $2
           AND user_id = $3
           AND scope = $4
           AND consumed_at IS NULL
           AND expires_at > now()",
    )
    .bind(token_hash)
    .bind(session_id)
    .bind(user_id)
    .bind(scope)
    .execute(connection)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn prune(connection: &mut PgConnection) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM auth_step_up_tokens
         WHERE expires_at < now() - interval '1 day'
            OR consumed_at < now() - interval '1 day'",
    )
    .execute(connection)
    .await?;
    Ok(result.rows_affected())
}
