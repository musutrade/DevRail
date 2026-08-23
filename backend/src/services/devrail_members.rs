use crate::access::ActorContext;
use crate::error::ApiError;
use crate::models::{
    AddDevRailProjectMemberRequest, DevRailProjectMemberPage, DevRailProjectMemberResponse,
};
use crate::repositories::{self, devrail_members};
use serde_json::json;
use sqlx::PgPool;

fn response(row: crate::models::DevRailProjectMemberRow) -> DevRailProjectMemberResponse {
    DevRailProjectMemberResponse {
        id: row.id,
        project_id: row.project_id,
        user_id: row.user_id,
        username: row.username,
        display_name: row.display_name,
        role: row.role,
        joined_at: row.joined_at,
    }
}

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
) -> Result<DevRailProjectMemberPage, ApiError> {
    if repositories::devrail::find_project(pool, actor, project_id)
        .await
        .map_err(ApiError::internal)?
        .is_none()
    {
        return Err(ApiError::not_found("项目不存在或超出数据范围"));
    }
    let items = devrail_members::list(pool, actor, project_id)
        .await
        .map_err(ApiError::internal)?
        .into_iter()
        .map(response)
        .collect();
    Ok(DevRailProjectMemberPage { items })
}

pub async fn add(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    req: &AddDevRailProjectMemberRequest,
) -> Result<DevRailProjectMemberResponse, ApiError> {
    let role = req.role.as_deref().unwrap_or("developer");
    if !matches!(role, "admin" | "developer" | "observer") {
        return Err(ApiError::validation("项目成员角色无效"));
    }
    let mut tx = pool.begin().await.map_err(ApiError::internal)?;
    let row = devrail_members::add(&mut tx, actor, project_id, req.user_id, role)
        .await
        .map_err(ApiError::internal)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.project_member.add",
        "devrail_project_member",
        Some(row.id),
        json!({"projectId":project_id,"userId":req.user_id,"role":role}),
    )
    .await
    .map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(response(row))
}

pub async fn remove(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    user_id: i64,
) -> Result<(), ApiError> {
    let mut tx = pool.begin().await.map_err(ApiError::internal)?;
    if !devrail_members::revoke(&mut tx, actor, project_id, user_id)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::not_found("项目成员不存在或不可移除"));
    }
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.project_member.remove",
        "devrail_project_member",
        None,
        json!({"projectId":project_id,"userId":user_id}),
    )
    .await
    .map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(())
}
