//! 审计日志服务：分页参数与筛选校验。

use crate::access::ActorContext;
use crate::error::{db_error, ApiError};
use crate::models::{audit_log_response, AuditLogQuery, PageAuditLog};
use crate::repositories;
use sqlx::PgPool;

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    query: &AuditLogQuery,
) -> Result<PageAuditLog, ApiError> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let keyword = query
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let action = query
        .action
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let cursor = query.cursor.as_deref().map(parse_cursor).transpose()?;
    let (mut rows, total) = tokio::try_join!(
        repositories::audit_logs::list(
            pool,
            actor,
            keyword.clone(),
            action.clone(),
            page,
            page_size,
            cursor,
        ),
        repositories::audit_logs::count(pool, actor, keyword, action),
    )
    .map_err(db_error)?;
    let has_more = rows.len() > page_size as usize;
    if has_more {
        rows.pop();
    }
    let next_cursor = has_more.then(|| rows.last().map(encode_cursor)).flatten();
    Ok(PageAuditLog {
        items: rows.into_iter().map(audit_log_response).collect(),
        total,
        page,
        page_size,
        next_cursor,
    })
}

fn parse_cursor(cursor: &str) -> Result<(chrono::DateTime<chrono::Utc>, i64), ApiError> {
    let (timestamp, id) = cursor
        .split_once('.')
        .ok_or_else(|| ApiError::validation("cursor 格式无效"))?;
    let timestamp = timestamp
        .parse::<i64>()
        .map_err(|_| ApiError::validation("cursor 格式无效"))?;
    let id = id
        .parse::<i64>()
        .map_err(|_| ApiError::validation("cursor 格式无效"))?;
    let created_at = chrono::DateTime::from_timestamp_micros(timestamp)
        .ok_or_else(|| ApiError::validation("cursor 格式无效"))?;
    if id <= 0 {
        return Err(ApiError::validation("cursor 格式无效"));
    }
    Ok((created_at, id))
}

fn encode_cursor(row: &crate::models::AuditLogRow) -> String {
    format!("{}.{}", row.created_at.timestamp_micros(), row.id)
}
