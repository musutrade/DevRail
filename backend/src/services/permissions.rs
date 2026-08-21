//! 权限服务：组装权限组树 + 仪表盘统计（无 SQL）

use crate::access::ActorContext;
use crate::error::{db_error, ApiError};
use crate::models::{
    DashboardStats, DashboardStatsRow, PermissionGroupResponse, PermissionResponse,
};
use crate::repositories;
use sqlx::PgPool;

pub async fn groups(pool: &PgPool) -> Result<Vec<PermissionGroupResponse>, ApiError> {
    let (group_rows, permission_rows) = tokio::try_join!(
        repositories::permissions::list_groups(pool),
        repositories::permissions::list_permissions(pool),
    )
    .map_err(db_error)?;

    let mut groups: Vec<PermissionGroupResponse> = group_rows
        .into_iter()
        .map(|g| PermissionGroupResponse {
            id: g.id,
            code: g.code,
            name: g.name,
            icon: g.icon,
            permissions: Vec::new(),
        })
        .collect();

    for p in permission_rows {
        if let Some(group) = groups.iter_mut().find(|g| g.id == p.group_id) {
            group.permissions.push(PermissionResponse {
                id: p.id,
                code: p.code,
                name: p.name,
                r#type: p.r#type,
                description: p.description,
            });
        }
    }
    Ok(groups)
}

pub async fn stats(pool: &PgPool, actor: &ActorContext) -> Result<DashboardStats, ApiError> {
    let row: DashboardStatsRow = repositories::permissions::stats(pool, actor)
        .await
        .map_err(db_error)?;
    Ok(row.into())
}
