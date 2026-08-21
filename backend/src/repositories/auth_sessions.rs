//! Authentication session Repository: server-side sessions and distributed login throttling.

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgConnection, PgPool};

#[derive(Debug, FromRow)]
pub struct SessionAuthContext {
    pub session_id: i64,
    pub user_id: i64,
    pub csrf_token_hash: String,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub data_scope: String,
    pub permission_codes: Vec<String>,
}

pub(crate) struct NewSession {
    pub(crate) user_id: i64,
    pub(crate) session_token_hash: String,
    pub(crate) csrf_token_hash: String,
    pub(crate) token_version: i64,
    pub(crate) idle_timeout_secs: i64,
    pub(crate) persistent: bool,
    pub(crate) expires_at: DateTime<Utc>,
}

pub async fn auth_context(
    pool: &PgPool,
    session_token_hash: &str,
) -> Result<Option<SessionAuthContext>, sqlx::Error> {
    let context = sqlx::query_as::<_, SessionAuthContext>(
        "SELECT s.id AS session_id,
                s.user_id,
                s.csrf_token_hash,
                u.organization_id,
                u.department_id,
                CASE
                    WHEN COALESCE(bool_or(r.code = 'super_admin' OR r.data_scope = 'all'), FALSE)
                        THEN 'all'
                    WHEN COALESCE(bool_or(r.data_scope = 'organization'), FALSE)
                        THEN 'organization'
                    WHEN COALESCE(bool_or(r.data_scope = 'department_and_children'), FALSE)
                        THEN 'department_and_children'
                    WHEN COALESCE(bool_or(r.data_scope = 'department'), FALSE)
                        THEN 'department'
                    ELSE 'self'
                END AS data_scope,
                COALESCE(
                    array_agg(DISTINCT p.code ORDER BY p.code)
                        FILTER (WHERE p.code IS NOT NULL),
                    ARRAY[]::text[]
                ) AS permission_codes
         FROM auth_sessions s
         JOIN users u ON u.id = s.user_id
         LEFT JOIN user_roles ur ON ur.user_id = u.id
         LEFT JOIN roles r ON r.id = ur.role_id AND r.is_active = TRUE
         LEFT JOIN role_permissions rp ON rp.role_id = r.id
         LEFT JOIN permissions p ON p.id = rp.permission_id
         WHERE s.session_token_hash = $1
           AND s.revoked_at IS NULL
           AND s.expires_at > now()
           AND s.last_seen_at + s.idle_timeout_secs * interval '1 second' > now()
           AND s.token_version = u.token_version
           AND u.deleted_at IS NULL
           AND u.status = 'active'
           AND (
               NOT EXISTS (
                   SELECT 1 FROM user_roles required_ur
                   JOIN roles required_role ON required_role.id = required_ur.role_id
                   WHERE required_ur.user_id = u.id
                     AND required_role.code = 'super_admin'
                     AND required_role.is_active = TRUE
               )
               OR EXISTS (
                   SELECT 1 FROM user_mfa_settings mfa
                   WHERE mfa.user_id = u.id AND mfa.totp_enabled_at IS NOT NULL
               )
           )
         GROUP BY s.id, s.user_id, s.csrf_token_hash, u.organization_id, u.department_id",
    )
    .bind(session_token_hash)
    .fetch_optional(pool)
    .await?;

    if let Some(context) = context.as_ref() {
        sqlx::query(
            "UPDATE auth_sessions
             SET last_seen_at = now()
             WHERE id = $1 AND last_seen_at < now() - interval '1 minute'",
        )
        .bind(context.session_id)
        .execute(pool)
        .await?;
    }
    Ok(context)
}

pub(crate) async fn create(
    connection: &mut PgConnection,
    session: &NewSession,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO auth_sessions
             (user_id, session_token_hash, csrf_token_hash, token_version,
              idle_timeout_secs, persistent, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id",
    )
    .bind(session.user_id)
    .bind(&session.session_token_hash)
    .bind(&session.csrf_token_hash)
    .bind(session.token_version)
    .bind(session.idle_timeout_secs)
    .bind(session.persistent)
    .bind(session.expires_at)
    .fetch_one(connection)
    .await
}

pub async fn revoke(
    connection: &mut PgConnection,
    session_id: i64,
    user_id: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE auth_sessions
         SET revoked_at = now()
         WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(session_id)
    .bind(user_id)
    .execute(&mut *connection)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn revoke_all_for_user(
    connection: &mut PgConnection,
    user_id: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE auth_sessions
         SET revoked_at = now()
         WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .execute(&mut *connection)
    .await?;
    Ok(result.rows_affected())
}

pub async fn enforce_user_limit(
    connection: &mut PgConnection,
    user_id: i64,
    max_sessions: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE auth_sessions
         SET revoked_at = now()
         WHERE user_id = $1
           AND revoked_at IS NULL
           AND id NOT IN (
               SELECT id
               FROM auth_sessions
               WHERE user_id = $1 AND revoked_at IS NULL
               ORDER BY created_at DESC, id DESC
               LIMIT $2
           )",
    )
    .bind(user_id)
    .bind(max_sessions)
    .execute(connection)
    .await?;
    Ok(result.rows_affected())
}

pub async fn prune(connection: &mut PgConnection) -> Result<u64, sqlx::Error> {
    let sessions = sqlx::query(
        "DELETE FROM auth_sessions
         WHERE expires_at < now() - interval '7 days'
            OR revoked_at < now() - interval '7 days'",
    )
    .execute(&mut *connection)
    .await?;
    let attempts = sqlx::query(
        "DELETE FROM auth_login_attempts
         WHERE updated_at < now() - interval '30 days'",
    )
    .execute(&mut *connection)
    .await?;
    Ok(sessions.rows_affected() + attempts.rows_affected())
}

pub async fn locked_until(
    pool: &PgPool,
    identifier_hash: &str,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT locked_until
         FROM auth_login_attempts
         WHERE identifier_hash = $1 AND locked_until > now()",
    )
    .bind(identifier_hash)
    .fetch_optional(pool)
    .await
    .map(Option::flatten)
}

pub async fn record_login_failure(
    connection: &mut PgConnection,
    identifier_hash: &str,
    max_failures: i32,
    window_secs: i64,
    lockout_secs: i64,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO auth_login_attempts AS attempts
             (identifier_hash, failure_count, window_started_at, locked_until, updated_at)
         VALUES ($1, 1, now(), NULL, now())
         ON CONFLICT (identifier_hash) DO UPDATE
         SET failure_count = CASE
                 WHEN attempts.window_started_at <= now() - $2::double precision * interval '1 second'
                     THEN 1
                 ELSE attempts.failure_count + 1
             END,
             window_started_at = CASE
                 WHEN attempts.window_started_at <= now() - $2::double precision * interval '1 second'
                     THEN now()
                 ELSE attempts.window_started_at
             END,
             locked_until = CASE
                 WHEN (CASE
                     WHEN attempts.window_started_at <= now() - $2::double precision * interval '1 second'
                         THEN 1
                     ELSE attempts.failure_count + 1
                 END) >= $3
                     THEN now() + $4::double precision * interval '1 second'
                 ELSE NULL
             END,
             updated_at = now()
         RETURNING locked_until",
    )
    .bind(identifier_hash)
    .bind(window_secs)
    .bind(max_failures)
    .bind(lockout_secs)
    .fetch_one(connection)
    .await
}

pub async fn clear_login_failures(
    connection: &mut PgConnection,
    identifier_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM auth_login_attempts WHERE identifier_hash = $1")
        .bind(identifier_hash)
        .execute(connection)
        .await?;
    Ok(())
}
