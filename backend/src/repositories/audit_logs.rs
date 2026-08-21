//! 审计日志 Repository：安全变更记录与只读分页查询。

use crate::access::ActorContext;
use crate::models::{AuditArchiveRow, AuditLogRow};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgConnection, PgPool};

pub async fn archive_batch(
    pool: &PgPool,
    cutoff: DateTime<Utc>,
    batch_size: i64,
) -> Result<Vec<AuditArchiveRow>, sqlx::Error> {
    sqlx::query_as::<_, AuditArchiveRow>(
        "SELECT id, actor_user_id, action, target_type, target_id, details, trace_id,
                organization_id, department_id, created_at
         FROM audit_logs
         WHERE created_at < $1
         ORDER BY id
         LIMIT $2",
    )
    .bind(cutoff)
    .bind(batch_size)
    .fetch_all(pool)
    .await
}

pub async fn delete_archived(
    pool: &PgPool,
    ids: &[i64],
    cutoff: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    if ids.is_empty() {
        return Ok(0);
    }
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT set_config('arc_admin.audit_maintenance', 'on', true)")
        .execute(&mut *transaction)
        .await?;
    let result = sqlx::query("DELETE FROM audit_logs WHERE id = ANY($1) AND created_at < $2")
        .bind(ids)
        .bind(cutoff)
        .execute(&mut *transaction)
        .await?;
    let expected = ids.len() as u64;
    if result.rows_affected() != expected {
        return Err(sqlx::Error::Protocol(format!(
            "audit archive delete mismatch: expected {expected}, deleted {}",
            result.rows_affected()
        )));
    }
    transaction.commit().await?;
    Ok(result.rows_affected())
}

pub async fn record(
    connection: &mut PgConnection,
    actor_user_id: Option<i64>,
    action: &str,
    target_type: &str,
    target_id: Option<i64>,
    details: Value,
) -> Result<(), sqlx::Error> {
    let trace_id = crate::telemetry::current_trace_id();
    sqlx::query(
        "INSERT INTO audit_logs
             (actor_user_id, action, target_type, target_id, details, trace_id,
              organization_id, department_id)
         VALUES (
             $1, $2, $3, $4, $5, $6,
             COALESCE(
                 (SELECT organization_id FROM users WHERE id = $1),
                 (SELECT organization_id FROM users WHERE id = $4 AND $3 = 'user')
             ),
             COALESCE(
                 (SELECT department_id FROM users WHERE id = $1),
                 (SELECT department_id FROM users WHERE id = $4 AND $3 = 'user')
             )
         )",
    )
    .bind(actor_user_id)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(details)
    .bind(trace_id)
    .execute(connection)
    .await?;
    Ok(())
}

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    keyword: Option<String>,
    action: Option<String>,
    page: i64,
    page_size: i64,
    cursor: Option<(DateTime<Utc>, i64)>,
) -> Result<Vec<AuditLogRow>, sqlx::Error> {
    let (cursor_created_at, cursor_id) = cursor.unzip();
    sqlx::query_as::<_, AuditLogRow>(
        "WITH RECURSIVE visible_departments AS (
             SELECT d.id
             FROM departments d
             WHERE d.id = $4 AND d.organization_id = $2
             UNION
             SELECT child.id
             FROM departments child
             JOIN visible_departments parent ON child.parent_id = parent.id
             WHERE child.organization_id = $2
         )
         SELECT a.id, a.actor_user_id, u.username AS actor_username, a.action,
                a.target_type, a.target_id, a.details, a.trace_id, a.created_at
         FROM audit_logs a
         LEFT JOIN users u ON u.id = a.actor_user_id
         WHERE (
               $1 = 'all'
               OR a.organization_id = $2 AND (
                   $1 = 'organization'
                   OR $1 = 'self' AND a.actor_user_id = $3
                   OR $1 = 'department' AND a.department_id = $4
                   OR $1 = 'department_and_children'
                      AND a.department_id IN (SELECT id FROM visible_departments)
               )
           )
           AND ($5::text IS NULL OR a.action ILIKE '%' || $5 || '%'
                OR a.target_type ILIKE '%' || $5 || '%'
                OR u.username ILIKE '%' || $5 || '%'
                OR a.trace_id = $5)
           AND ($6::text IS NULL OR a.action = $6)
           AND ($7::timestamptz IS NULL OR (a.created_at, a.id) < ($7, $8::bigint))
         ORDER BY a.created_at DESC, a.id DESC
         LIMIT $9 OFFSET $10",
    )
    .bind(actor.data_scope.as_str())
    .bind(actor.organization_id)
    .bind(actor.user_id)
    .bind(actor.department_id)
    .bind(keyword)
    .bind(action)
    .bind(cursor_created_at)
    .bind(cursor_id)
    .bind(page_size + 1)
    .bind(if cursor_created_at.is_some() {
        0
    } else {
        (page - 1) * page_size
    })
    .fetch_all(pool)
    .await
}

pub async fn count(
    pool: &PgPool,
    actor: &ActorContext,
    keyword: Option<String>,
    action: Option<String>,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "WITH RECURSIVE visible_departments AS (
             SELECT d.id
             FROM departments d
             WHERE d.id = $4 AND d.organization_id = $2
             UNION
             SELECT child.id
             FROM departments child
             JOIN visible_departments parent ON child.parent_id = parent.id
             WHERE child.organization_id = $2
         )
         SELECT count(*)
         FROM audit_logs a
         LEFT JOIN users u ON u.id = a.actor_user_id
         WHERE (
               $1 = 'all'
               OR a.organization_id = $2 AND (
                   $1 = 'organization'
                   OR $1 = 'self' AND a.actor_user_id = $3
                   OR $1 = 'department' AND a.department_id = $4
                   OR $1 = 'department_and_children'
                      AND a.department_id IN (SELECT id FROM visible_departments)
               )
           )
           AND ($5::text IS NULL OR a.action ILIKE '%' || $5 || '%'
                OR a.target_type ILIKE '%' || $5 || '%'
                OR u.username ILIKE '%' || $5 || '%'
                OR a.trace_id = $5)
           AND ($6::text IS NULL OR a.action = $6)",
    )
    .bind(actor.data_scope.as_str())
    .bind(actor.organization_id)
    .bind(actor.user_id)
    .bind(actor.department_id)
    .bind(keyword)
    .bind(action)
    .fetch_one(pool)
    .await
}
