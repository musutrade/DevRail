//! MFA Repository: factors, server-side challenge state, and recovery codes.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;
use webauthn_rs::prelude::Passkey;

#[derive(Debug, FromRow)]
pub struct MfaSummaryRow {
    pub required: bool,
    pub webauthn_user_id: Option<Uuid>,
    pub totp_enabled: bool,
    pub recovery_codes_remaining: i64,
    pub passkey_count: i64,
}

#[derive(Debug, FromRow)]
pub struct ChallengeRow {
    pub id: i64,
    pub user_id: i64,
    pub kind: String,
    pub persistent: bool,
    pub state: Json<Value>,
    pub attempt_count: i32,
    pub max_attempts: i32,
}

#[derive(Debug, FromRow)]
pub struct PasskeyRow {
    pub id: i64,
    pub name: String,
    pub credential: Json<Passkey>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub struct RecoveryCodeRow {
    pub id: i64,
    pub code_hash: String,
}

pub async fn summary(pool: &PgPool, user_id: i64) -> Result<MfaSummaryRow, sqlx::Error> {
    sqlx::query_as(
        "SELECT EXISTS (
                    SELECT 1 FROM user_roles ur
                    JOIN roles r ON r.id = ur.role_id
                    WHERE ur.user_id = $1 AND r.code = 'super_admin' AND r.is_active = TRUE
                ) AS required,
                settings.webauthn_user_id,
                COALESCE(settings.totp_enabled_at IS NOT NULL, FALSE) AS totp_enabled,
                (SELECT count(*) FROM user_mfa_recovery_codes recovery
                 WHERE recovery.user_id = $1 AND recovery.used_at IS NULL) AS recovery_codes_remaining,
                (SELECT count(*) FROM user_passkeys passkey
                 WHERE passkey.user_id = $1) AS passkey_count
         FROM (SELECT $1::bigint AS user_id) requested_user
         LEFT JOIN user_mfa_settings settings ON settings.user_id = requested_user.user_id",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
}

pub async fn ensure_settings(
    connection: &mut PgConnection,
    user_id: i64,
    webauthn_user_id: Uuid,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO user_mfa_settings (user_id, webauthn_user_id)
         VALUES ($1, $2)
         ON CONFLICT (user_id) DO UPDATE SET updated_at = user_mfa_settings.updated_at
         RETURNING webauthn_user_id",
    )
    .bind(user_id)
    .bind(webauthn_user_id)
    .fetch_one(connection)
    .await
}

pub async fn enable_totp(
    connection: &mut PgConnection,
    user_id: i64,
    ciphertext: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE user_mfa_settings
         SET totp_secret_ciphertext = $2,
             totp_enabled_at = now(),
             updated_at = now()
         WHERE user_id = $1",
    )
    .bind(user_id)
    .bind(ciphertext)
    .execute(connection)
    .await?;
    Ok(())
}

pub async fn consume_reauth_totp_counter(
    connection: &mut PgConnection,
    user_id: i64,
    counter: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE user_mfa_settings
         SET last_reauth_totp_counter = $2,
             last_reauth_totp_used_at = now(),
             updated_at = now()
         WHERE user_id = $1
           AND (last_reauth_totp_counter IS NULL OR last_reauth_totp_counter < $2)
         RETURNING TRUE",
    )
    .bind(user_id)
    .bind(counter)
    .fetch_optional(connection)
    .await
    .map(|updated| updated.unwrap_or(false))
}

pub async fn create_challenge(
    connection: &mut PgConnection,
    token_hash: &str,
    user_id: i64,
    kind: &str,
    persistent: bool,
    state: &Value,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO auth_mfa_challenges
             (token_hash, user_id, kind, persistent, state, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(token_hash)
    .bind(user_id)
    .bind(kind)
    .bind(persistent)
    .bind(Json(state))
    .bind(expires_at)
    .execute(connection)
    .await?;
    Ok(())
}

pub async fn consume_login_challenges_for_user(
    connection: &mut PgConnection,
    user_id: i64,
) -> Result<u64, sqlx::Error> {
    sqlx::query(
        "UPDATE auth_mfa_challenges
         SET consumed_at = now()
         WHERE user_id = $1 AND consumed_at IS NULL
           AND kind IN ('login', 'totp_enrollment', 'passkey_authentication')",
    )
    .bind(user_id)
    .execute(connection)
    .await
    .map(|result| result.rows_affected())
}

pub async fn challenge_for_update(
    connection: &mut PgConnection,
    token_hash: &str,
    expected_kind: &str,
) -> Result<Option<ChallengeRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, user_id, kind, persistent, state, attempt_count, max_attempts
         FROM auth_mfa_challenges
         WHERE token_hash = $1 AND kind = $2 AND consumed_at IS NULL AND expires_at > now()
         FOR UPDATE",
    )
    .bind(token_hash)
    .bind(expected_kind)
    .fetch_optional(connection)
    .await
}

pub async fn challenge(
    pool: &PgPool,
    token_hash: &str,
    expected_kind: &str,
) -> Result<Option<ChallengeRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, user_id, kind, persistent, state, attempt_count, max_attempts
         FROM auth_mfa_challenges
         WHERE token_hash = $1 AND kind = $2 AND consumed_at IS NULL AND expires_at > now()",
    )
    .bind(token_hash)
    .bind(expected_kind)
    .fetch_optional(pool)
    .await
}

pub async fn challenge_kind(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kind FROM auth_mfa_challenges
         WHERE token_hash = $1 AND consumed_at IS NULL AND expires_at > now()",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
}

pub async fn totp_secret_for_update(
    connection: &mut PgConnection,
    user_id: i64,
) -> Result<Option<Vec<u8>>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT totp_secret_ciphertext FROM user_mfa_settings
         WHERE user_id = $1 AND totp_enabled_at IS NOT NULL",
    )
    .bind(user_id)
    .fetch_optional(connection)
    .await
    .map(Option::flatten)
}

pub async fn record_challenge_failure(
    connection: &mut PgConnection,
    challenge_id: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE auth_mfa_challenges
         SET attempt_count = attempt_count + 1,
             consumed_at = CASE WHEN attempt_count + 1 >= max_attempts THEN now() ELSE consumed_at END
         WHERE id = $1
         RETURNING attempt_count >= max_attempts",
    )
    .bind(challenge_id)
    .fetch_one(connection)
    .await
}

pub async fn consume_challenge(
    connection: &mut PgConnection,
    challenge_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE auth_mfa_challenges SET consumed_at = now() WHERE id = $1")
        .bind(challenge_id)
        .execute(connection)
        .await?;
    Ok(())
}

pub async fn prune_challenges(connection: &mut PgConnection) -> Result<u64, sqlx::Error> {
    sqlx::query(
        "DELETE FROM auth_mfa_challenges
         WHERE expires_at < now() - interval '1 day' OR consumed_at < now() - interval '1 day'",
    )
    .execute(connection)
    .await
    .map(|result| result.rows_affected())
}

pub async fn replace_recovery_codes(
    connection: &mut PgConnection,
    user_id: i64,
    hashes: &[String],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM user_mfa_recovery_codes WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *connection)
        .await?;
    for hash in hashes {
        sqlx::query("INSERT INTO user_mfa_recovery_codes (user_id, code_hash) VALUES ($1, $2)")
            .bind(user_id)
            .bind(hash)
            .execute(&mut *connection)
            .await?;
    }
    sqlx::query(
        "UPDATE user_mfa_settings
         SET recovery_codes_issued_at = now(), updated_at = now() WHERE user_id = $1",
    )
    .bind(user_id)
    .execute(connection)
    .await?;
    Ok(())
}

pub async fn recovery_codes(
    pool: &PgPool,
    user_id: i64,
) -> Result<Vec<RecoveryCodeRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, code_hash FROM user_mfa_recovery_codes
         WHERE user_id = $1 AND used_at IS NULL ORDER BY id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn consume_recovery_code(
    connection: &mut PgConnection,
    user_id: i64,
    id: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE user_mfa_recovery_codes
         SET used_at = now()
         WHERE id = $1 AND user_id = $2 AND used_at IS NULL
         RETURNING TRUE",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(connection)
    .await
    .map(|consumed| consumed.unwrap_or(false))
}

pub async fn list_passkeys(pool: &PgPool, user_id: i64) -> Result<Vec<PasskeyRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, name, credential, last_used_at, created_at
         FROM user_passkeys WHERE user_id = $1 ORDER BY created_at, id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn list_passkeys_in_connection(
    connection: &mut PgConnection,
    user_id: i64,
) -> Result<Vec<PasskeyRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, name, credential, last_used_at, created_at
         FROM user_passkeys WHERE user_id = $1 ORDER BY created_at, id",
    )
    .bind(user_id)
    .fetch_all(connection)
    .await
}

pub async fn create_passkey(
    connection: &mut PgConnection,
    user_id: i64,
    name: &str,
    credential_id: &str,
    credential: &Passkey,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO user_passkeys (user_id, name, credential_id, credential)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(user_id)
    .bind(name)
    .bind(credential_id)
    .bind(Json(credential))
    .fetch_one(connection)
    .await
}

pub async fn update_passkey_after_use(
    connection: &mut PgConnection,
    id: i64,
    credential: &Passkey,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE user_passkeys
         SET credential = $2, last_used_at = now(), updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(Json(credential))
    .execute(connection)
    .await?;
    Ok(())
}

pub async fn delete_passkey(
    connection: &mut PgConnection,
    user_id: i64,
    id: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query("DELETE FROM user_passkeys WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(connection)
        .await
        .map(|result| result.rows_affected() > 0)
}
