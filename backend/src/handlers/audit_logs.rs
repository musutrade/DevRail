//! 审计日志 Handler：受审计读取权限保护的分页列表。

use crate::auth::RequirePermission;
use crate::error::ApiError;
use crate::models::{AuditLogQuery, PageAuditLog};
use crate::permissions::AuditLogRead;
use crate::services;
use crate::AppState;
use axum::extract::{Query, State};
use axum::Json;

pub async fn list(
    State(state): State<AppState>,
    auth: RequirePermission<AuditLogRead>,
    Query(query): Query<AuditLogQuery>,
) -> Result<Json<PageAuditLog>, ApiError> {
    services::audit_logs::list(&state.pool, &auth, &query)
        .await
        .map(Json)
}
