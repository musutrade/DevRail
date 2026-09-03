//! 角色服务：参数校验 + 业务规则（无 SQL）

use crate::access::DataScope;
use crate::error::{db_error, ApiError};
use crate::models::{
    CreateRoleRequest, NullablePatch, RolePermissions, RoleResponse, RoleRow,
    RoleWithPermissionsRow, UpdateRolePermissionsRequest, UpdateRoleRequest,
};
use crate::repositories;
use serde_json::json;
use sqlx::PgPool;
use std::collections::BTreeSet;

const ROLE_COLORS: [&str; 5] = ["primary", "warning", "success", "danger", "neutral"];
const DATA_SCOPES: [&str; 5] = [
    "all",
    "organization",
    "department_and_children",
    "department",
    "self",
];

fn nullable_text_update(patch: &NullablePatch<String>) -> repositories::roles::NullableTextUpdate {
    match patch {
        NullablePatch::Missing => repositories::roles::NullableTextUpdate::Unchanged,
        NullablePatch::Null => repositories::roles::NullableTextUpdate::Set(None),
        NullablePatch::Value(value) => {
            repositories::roles::NullableTextUpdate::Set(Some(value.clone()))
        }
    }
}

async fn to_response(pool: &PgPool, row: RoleRow) -> Result<RoleResponse, ApiError> {
    let permission_group_ids = repositories::roles::permission_group_ids_by_role(pool, row.id)
        .await
        .map_err(db_error)?;
    Ok(RoleResponse {
        id: row.id,
        code: row.code,
        name: row.name,
        category: row.category,
        icon: row.icon,
        color: row.color,
        description: row.description,
        data_scope: row.data_scope,
        is_active: row.is_active,
        members: row.members,
        permission_group_ids,
    })
}

pub async fn list(pool: &PgPool) -> Result<Vec<RoleResponse>, ApiError> {
    let rows = repositories::roles::list_all(pool)
        .await
        .map_err(db_error)?;
    Ok(rows.into_iter().map(list_response).collect())
}

fn list_response(row: RoleWithPermissionsRow) -> RoleResponse {
    RoleResponse {
        id: row.id,
        code: row.code,
        name: row.name,
        category: row.category,
        icon: row.icon,
        color: row.color,
        description: row.description,
        data_scope: row.data_scope,
        is_active: row.is_active,
        members: row.members,
        permission_group_ids: row.permission_group_ids,
    }
}

pub async fn get(pool: &PgPool, id: i64) -> Result<RoleResponse, ApiError> {
    let row = repositories::roles::find_by_id(pool, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("角色不存在"))?;
    to_response(pool, row).await
}

pub async fn create(
    pool: &PgPool,
    actor_user_id: Option<i64>,
    actor_data_scope: DataScope,
    req: &CreateRoleRequest,
    can_assign_permissions: bool,
) -> Result<RoleResponse, ApiError> {
    let code = req.code.trim();
    let valid_code = (3..=64).contains(&code.len())
        && code.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !valid_code {
        return Err(ApiError::validation(
            "角色编码需为 3-64 位小写字母、数字或下划线，且以字母开头",
        ));
    }
    if req.name.trim().is_empty() {
        return Err(ApiError::validation("角色名称不能为空"));
    }
    let color = req.color.clone().unwrap_or_else(|| "neutral".to_string());
    if !ROLE_COLORS.contains(&color.as_str()) {
        return Err(ApiError::validation(
            "颜色只能是 primary、warning、success、danger 或 neutral",
        ));
    }
    let category = req
        .category
        .clone()
        .unwrap_or_else(|| "general".to_string());
    let data_scope = req.data_scope.as_deref().unwrap_or("self");
    let data_scope = validate_data_scope(data_scope)?;
    validate_data_scope_grant(actor_data_scope, data_scope)?;

    let role = repositories::roles::NewRole {
        code: code.to_string(),
        name: req.name.trim().to_string(),
        category,
        icon: req.icon.clone(),
        color: Some(color),
        description: req.description.clone(),
        data_scope: data_scope.as_str().to_string(),
    };
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let id = repositories::roles::create(&mut transaction, &role)
        .await
        .map_err(db_error)?;
    if let Some(permission_ids) = &req.permission_ids {
        if !permission_ids.is_empty() && !can_assign_permissions {
            return Err(ApiError::forbidden("缺少分配角色权限的权限"));
        }
        let permission_codes =
            validate_permission_dependencies(&mut transaction, permission_ids).await?;
        validate_permission_grant_scope_in_connection(
            &mut transaction,
            actor_user_id,
            &permission_codes,
        )
        .await?;
        repositories::roles::assign_permissions(&mut transaction, id, permission_ids)
            .await
            .map_err(db_error)?;
    }
    repositories::audit_logs::record(
        &mut transaction,
        actor_user_id,
        "role.create",
        "role",
        Some(id),
        json!({
            "code": code,
            "name": req.name.trim(),
            "dataScope": data_scope.as_str(),
            "permissionIds": req.permission_ids.as_deref().unwrap_or_default(),
        }),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    get(pool, id).await
}

pub async fn update(
    pool: &PgPool,
    actor_user_id: Option<i64>,
    actor_data_scope: DataScope,
    id: i64,
    req: &UpdateRoleRequest,
) -> Result<RoleResponse, ApiError> {
    let name = req.name.as_ref().map(|value| value.trim().to_string());
    if name.as_deref().is_some_and(str::is_empty) {
        return Err(ApiError::validation("角色名称不能为空"));
    }
    let category = req.category.as_ref().map(|value| value.trim().to_string());
    if category.as_deref().is_some_and(str::is_empty) {
        return Err(ApiError::validation("角色分类不能为空"));
    }
    if req.is_active == Some(false) {
        let role = repositories::roles::find_by_id(pool, id)
            .await
            .map_err(db_error)?
            .ok_or_else(|| ApiError::not_found("角色不存在"))?;
        if role.code == "super_admin" {
            return Err(ApiError::forbidden("内置超级管理员角色不可停用"));
        }
    }
    if let Some(data_scope) = &req.data_scope {
        let requested_scope = validate_data_scope(data_scope)?;
        validate_data_scope_grant(actor_data_scope, requested_scope)?;
        let role = repositories::roles::find_by_id(pool, id)
            .await
            .map_err(db_error)?
            .ok_or_else(|| ApiError::not_found("角色不存在"))?;
        if role.code == "super_admin" && data_scope != "all" {
            return Err(ApiError::forbidden(
                "内置超级管理员角色的数据范围必须为全部数据",
            ));
        }
    }
    if req.is_active == Some(true) {
        let permission_codes = repositories::permissions::codes_by_role_ids(pool, &[id])
            .await
            .map_err(db_error)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        validate_permission_grant_scope(pool, actor_user_id, &permission_codes).await?;
    }
    if let Some(color) = &req.color {
        if !ROLE_COLORS.contains(&color.as_str()) {
            return Err(ApiError::validation(
                "颜色只能是 primary、warning、success、danger 或 neutral",
            ));
        }
    }
    let role = repositories::roles::RoleUpdate {
        name,
        category,
        icon: nullable_text_update(&req.icon),
        color: req.color.clone(),
        description: nullable_text_update(&req.description),
        data_scope: req.data_scope.clone(),
        is_active: req.is_active,
    };
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let updated = repositories::roles::update(&mut transaction, id, &role)
        .await
        .map_err(db_error)?;
    if !updated {
        return Err(ApiError::not_found("角色不存在"));
    }
    repositories::audit_logs::record(
        &mut transaction,
        actor_user_id,
        "role.update",
        "role",
        Some(id),
        json!({
            "name": req.name,
            "category": req.category,
            "color": req.color,
            "dataScope": req.data_scope,
            "isActive": req.is_active,
        }),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    get(pool, id).await
}

fn validate_data_scope(data_scope: &str) -> Result<DataScope, ApiError> {
    if !DATA_SCOPES.contains(&data_scope) {
        return Err(ApiError::validation(
            "数据范围只能是全部、组织、部门及下级、部门或仅本人",
        ));
    }
    DataScope::from_database(data_scope).ok_or_else(|| ApiError::internal("无效的数据范围配置"))
}

fn validate_data_scope_grant(
    actor_data_scope: DataScope,
    requested_scope: DataScope,
) -> Result<(), ApiError> {
    if actor_data_scope.can_grant(requested_scope) {
        Ok(())
    } else {
        Err(ApiError::forbidden("不能授予超出自身范围的数据权限"))
    }
}

pub async fn delete(pool: &PgPool, actor_user_id: Option<i64>, id: i64) -> Result<(), ApiError> {
    if let Some(row) = repositories::roles::find_by_id(pool, id)
        .await
        .map_err(db_error)?
    {
        if row.code == "super_admin" {
            return Err(ApiError::forbidden("内置超级管理员角色不可删除"));
        }
        if row.members > 0 {
            return Err(ApiError::conflict("角色仍有成员，请先迁移用户后再删除"));
        }
    } else {
        return Err(ApiError::not_found("角色不存在"));
    }
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let deleted = repositories::roles::delete_if_unassigned(&mut transaction, id)
        .await
        .map_err(db_error)?;
    if !deleted {
        return Err(ApiError::conflict("角色仍有成员，请先迁移用户后再删除"));
    }
    repositories::audit_logs::record(
        &mut transaction,
        actor_user_id,
        "role.delete",
        "role",
        Some(id),
        json!({}),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(())
}

pub async fn get_permissions(pool: &PgPool, id: i64) -> Result<RolePermissions, ApiError> {
    repositories::roles::find_by_id(pool, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("角色不存在"))?;
    let permission_ids = repositories::roles::permission_ids_by_role(pool, id)
        .await
        .map_err(db_error)?;
    Ok(RolePermissions { permission_ids })
}

pub async fn assign_permissions(
    pool: &PgPool,
    actor_user_id: Option<i64>,
    id: i64,
    req: &UpdateRolePermissionsRequest,
) -> Result<(), ApiError> {
    let role = repositories::roles::find_by_id(pool, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("角色不存在"))?;
    if role.code == "super_admin" {
        return Err(ApiError::forbidden("内置超级管理员角色的权限不可修改"));
    }
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let permission_codes =
        validate_permission_dependencies(&mut transaction, &req.permission_ids).await?;
    validate_permission_grant_scope_in_connection(
        &mut transaction,
        actor_user_id,
        &permission_codes,
    )
    .await?;
    let previous_permission_ids =
        repositories::roles::permission_ids_by_role_in_connection(&mut transaction, id)
            .await
            .map_err(db_error)?;
    repositories::roles::assign_permissions(&mut transaction, id, &req.permission_ids)
        .await
        .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut transaction,
        actor_user_id,
        "role.permissions.update",
        "role",
        Some(id),
        json!({
            "previousPermissionIds": previous_permission_ids,
            "permissionIds": req.permission_ids,
        }),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(())
}

async fn validate_permission_dependencies(
    connection: &mut sqlx::PgConnection,
    permission_ids: &[i64],
) -> Result<BTreeSet<String>, ApiError> {
    let unique_ids = permission_ids.iter().copied().collect::<BTreeSet<_>>();
    if unique_ids.len() != permission_ids.len() {
        return Err(ApiError::validation("权限列表不能包含重复项"));
    }
    let codes = repositories::permissions::codes_by_ids(connection, permission_ids)
        .await
        .map_err(db_error)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    if codes.len() != unique_ids.len() {
        return Err(ApiError::validation("权限列表中包含不存在的权限"));
    }
    if codes.contains("role:permissions:write")
        && (!codes.contains("role:directory:read") || !codes.contains("permission:directory:read"))
    {
        return Err(ApiError::validation(
            "分配角色权限管理能力时，必须同时授予角色读取和权限目录读取权限",
        ));
    }
    if codes.contains("organization:department:write")
        && !codes.contains("organization:department:read")
    {
        return Err(ApiError::validation(
            "分配部门管理能力时，必须同时授予部门目录读取权限",
        ));
    }
    Ok(codes)
}

async fn validate_permission_grant_scope(
    pool: &PgPool,
    actor_user_id: Option<i64>,
    requested_permissions: &BTreeSet<String>,
) -> Result<(), ApiError> {
    let Some(actor_user_id) = actor_user_id else {
        return Ok(());
    };
    let actor_permissions =
        repositories::permissions::permission_codes_by_user(pool, actor_user_id)
            .await
            .map_err(db_error)?
            .into_iter()
            .collect::<BTreeSet<_>>();
    if requested_permissions
        .iter()
        .any(|permission| !actor_permissions.contains(permission))
    {
        return Err(ApiError::forbidden("不能授予操作者自身未拥有的权限"));
    }
    Ok(())
}

async fn validate_permission_grant_scope_in_connection(
    connection: &mut sqlx::PgConnection,
    actor_user_id: Option<i64>,
    requested_permissions: &BTreeSet<String>,
) -> Result<(), ApiError> {
    let Some(actor_user_id) = actor_user_id else {
        return Ok(());
    };
    let actor_permissions = repositories::permissions::permission_codes_by_user_in_connection(
        connection,
        actor_user_id,
    )
    .await
    .map_err(db_error)?
    .into_iter()
    .collect::<BTreeSet<_>>();
    if requested_permissions
        .iter()
        .any(|permission| !actor_permissions.contains(permission))
    {
        return Err(ApiError::forbidden("不能授予操作者自身未拥有的权限"));
    }
    Ok(())
}
