//! 用户服务：参数校验 + 业务规则（无 SQL）

use crate::access::ActorContext;
use crate::error::{db_error, ApiError};
use crate::models::{
    nullable_patch, user_response, user_with_roles_response, AssignRolesRequest,
    BatchAssignRolesRequest, BatchUserIdsRequest, CreateUserRequest, PageQuery, PageUser,
    UpdateUserRequest, UserResponse,
};
use crate::repositories;
use crate::services::{auth, departments};
use serde_json::json;
use sqlx::PgPool;

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
    query: &PageQuery,
) -> Result<PageUser, ApiError> {
    let requested_page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let keyword = normalized_filter(query.keyword.as_deref());
    let status = normalized_filter(query.status.as_deref());
    if status
        .as_deref()
        .is_some_and(|value| !matches!(value, "active" | "inactive" | "suspended"))
    {
        return Err(ApiError::validation(
            "status 必须是 active、inactive 或 suspended",
        ));
    }
    let role = normalized_filter(query.role.as_deref());
    let sort = user_sort(query.sort_by.as_deref(), query.sort_direction.as_deref())?;
    let mut params = repositories::users::UserListParams {
        keyword,
        status,
        role,
        sort,
        page: requested_page,
        page_size,
    };
    let (total, role_options) = tokio::try_join!(
        repositories::users::count(pool, actor, &params),
        repositories::users::list_role_options(pool, actor),
    )
    .map_err(db_error)?;
    let last_page = ((total + page_size - 1) / page_size).max(1);
    let page = requested_page.min(last_page);
    params.page = page;
    let rows = repositories::users::list(pool, actor, &params)
        .await
        .map_err(db_error)?;
    let items = rows.into_iter().map(user_with_roles_response).collect();
    Ok(PageUser {
        items,
        total,
        page,
        page_size,
        role_options,
    })
}

fn normalized_filter(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "all")
        .map(ToOwned::to_owned)
}

fn user_sort(
    sort_by: Option<&str>,
    sort_direction: Option<&str>,
) -> Result<repositories::users::UserSort, ApiError> {
    use repositories::users::UserSort;

    let direction = sort_direction.unwrap_or("desc");
    let sort = match (sort_by.unwrap_or("createdAt"), direction) {
        ("username", "asc") => UserSort::UsernameAsc,
        ("username", "desc") => UserSort::UsernameDesc,
        ("displayName", "asc") => UserSort::DisplayNameAsc,
        ("displayName", "desc") => UserSort::DisplayNameDesc,
        ("email", "asc") => UserSort::EmailAsc,
        ("email", "desc") => UserSort::EmailDesc,
        ("status", "asc") => UserSort::StatusAsc,
        ("status", "desc") => UserSort::StatusDesc,
        ("lastLoginAt", "asc") => UserSort::LastLoginAtAsc,
        ("lastLoginAt", "desc") => UserSort::LastLoginAtDesc,
        ("createdAt", "asc") => UserSort::CreatedAtAsc,
        ("createdAt", "desc") => UserSort::CreatedAtDesc,
        (_, "asc" | "desc") => return Err(ApiError::validation("sortBy 不是允许的用户排序字段")),
        _ => return Err(ApiError::validation("sortDirection 必须是 asc 或 desc")),
    };
    Ok(sort)
}

pub async fn get(pool: &PgPool, actor: &ActorContext, id: i64) -> Result<UserResponse, ApiError> {
    let row = repositories::users::find_by_id_for_actor(pool, actor, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("用户不存在"))?;
    let roles = repositories::users::role_names_by_user(pool, id)
        .await
        .map_err(db_error)?;
    Ok(user_response(row, roles))
}

pub async fn create(
    pool: &PgPool,
    actor: &ActorContext,
    req: &CreateUserRequest,
    can_grant_super_admin: bool,
    can_assign_department: bool,
) -> Result<UserResponse, ApiError> {
    if !actor.can_create_peer() {
        return Err(ApiError::forbidden("当前数据范围不允许创建其他用户"));
    }
    let actor_user_id = Some(actor.user_id);
    let username = req.username.trim();
    if !(3..=64).contains(&username.len()) {
        return Err(ApiError::validation("用户名长度需在 3-64 个字符之间"));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(ApiError::validation(
            "用户名仅允许字母、数字、下划线或连字符",
        ));
    }
    auth::validate_password(&req.password)?;
    let display_name = req.display_name.trim();
    if display_name.is_empty() || display_name.len() > 128 {
        return Err(ApiError::validation("显示名称长度需在 1-128 个字符之间"));
    }
    let status = req.status.clone().unwrap_or_else(|| "active".to_string());
    if !["active", "inactive", "suspended"].contains(&status.as_str()) {
        return Err(ApiError::validation("状态只能为启用、停用或已暂停"));
    }

    let hash = auth::hash_password_async(&req.password).await?;
    if let Some(role_ids) = &req.role_ids {
        validate_role_grant_scope(pool, actor_user_id, role_ids, can_grant_super_admin).await?;
    }
    let department_id = match req.department_id {
        Some(department_id) => {
            if !can_assign_department {
                return Err(ApiError::forbidden("缺少查看部门目录的权限"));
            }
            departments::validate_assignment(pool, actor, department_id).await?;
            Some(department_id)
        }
        None => actor.department_id,
    };
    let user = repositories::users::NewUser {
        username: username.to_string(),
        password_hash: hash,
        display_name: display_name.to_string(),
        email: req.email.clone(),
        status,
        organization_id: actor.organization_id,
        department_id,
    };
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let row = repositories::users::create(&mut transaction, &user)
        .await
        .map_err(db_error)?;
    if let Some(role_ids) = &req.role_ids {
        repositories::users::assign_roles(&mut transaction, row.id, role_ids)
            .await
            .map_err(db_error)?;
    }
    repositories::audit_logs::record(
        &mut transaction,
        actor_user_id,
        "user.create",
        "user",
        Some(row.id),
        json!({
            "username": username,
            "status": &user.status,
            "departmentId": user.department_id,
            "roleIds": req.role_ids.as_deref().unwrap_or_default(),
        }),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    let roles = repositories::users::role_names_by_user(pool, row.id)
        .await
        .map_err(db_error)?;
    Ok(user_response(row, roles))
}

pub async fn update(
    pool: &PgPool,
    id: i64,
    actor: Option<&ActorContext>,
    req: &UpdateUserRequest,
    can_grant_super_admin: bool,
    can_assign_department: bool,
) -> Result<UserResponse, ApiError> {
    ensure_actor_can_access(pool, actor, id).await?;
    let actor_user_id = actor.map(|context| context.user_id);
    let department_id = match (actor, req.department_id) {
        (Some(actor), Some(department_id)) => {
            if !can_assign_department {
                return Err(ApiError::forbidden("缺少查看部门目录的权限"));
            }
            if actor.user_id == id && actor.department_id != Some(department_id) {
                return Err(ApiError::forbidden("不能调整当前登录账号所属部门"));
            }
            departments::validate_assignment(pool, actor, department_id).await?;
            Some(department_id)
        }
        (None, Some(department_id)) => Some(department_id),
        (_, None) => None,
    };
    let display_name = req
        .display_name
        .as_ref()
        .map(|value| value.trim().to_string());
    if display_name
        .as_deref()
        .is_some_and(|value| value.is_empty() || value.len() > 128)
    {
        return Err(ApiError::validation("显示名称长度需在 1-128 个字符之间"));
    }
    if let Some(status) = &req.status {
        if !["active", "inactive", "suspended"].contains(&status.as_str()) {
            return Err(ApiError::validation("状态只能为启用、停用或已暂停"));
        }
        if status != "active" && actor_user_id == Some(id) {
            return Err(ApiError::forbidden("不能停用当前登录账号"));
        }
        if status == "active" {
            let role_ids = repositories::users::role_ids_by_user(pool, id)
                .await
                .map_err(db_error)?;
            validate_role_grant_scope(pool, actor_user_id, &role_ids, can_grant_super_admin)
                .await?;
        }
    }
    let password_hash = if let Some(password) = &req.password {
        auth::validate_password(password)?;
        Some(auth::hash_password_async(password).await?)
    } else {
        None
    };
    let (email_is_set, email) = nullable_patch(&req.email);
    let email = email.and_then(|value| {
        let value = value.trim().to_string();
        (!value.is_empty()).then_some(value)
    });
    let password_reset = password_hash.is_some();
    let status_revokes_sessions = req
        .status
        .as_deref()
        .is_some_and(|status| status != "active");

    let mut transaction = pool.begin().await.map_err(db_error)?;
    if status_revokes_sessions {
        protect_last_super_admin(&mut transaction, id).await?;
    }
    if let Some(hash) = password_hash {
        repositories::users::update_password(&mut transaction, id, &hash)
            .await
            .map_err(db_error)?
            .ok_or_else(|| ApiError::not_found("用户不存在"))?;
    }
    let row = repositories::users::update_profile(
        &mut transaction,
        id,
        display_name,
        email_is_set,
        email,
        req.status.clone(),
        department_id,
    )
    .await
    .map_err(db_error)?
    .ok_or_else(|| ApiError::not_found("用户不存在"))?;
    if password_reset || status_revokes_sessions {
        let revoked_sessions =
            repositories::auth_sessions::revoke_all_for_user(&mut transaction, id)
                .await
                .map_err(db_error)?;
        auth::record_session_revocation(
            &mut transaction,
            actor_user_id,
            id,
            if password_reset {
                "admin_password_reset"
            } else {
                "account_status_changed"
            },
            revoked_sessions,
        )
        .await?;
    }
    repositories::audit_logs::record(
        &mut transaction,
        actor_user_id,
        "user.update",
        "user",
        Some(id),
        json!({
            "displayNameChanged": req.display_name.is_some(),
            "emailChanged": !matches!(req.email, crate::models::NullablePatch::Missing),
            "status": req.status,
            "departmentId": req.department_id,
            "passwordReset": req.password.is_some(),
        }),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    let roles = repositories::users::role_names_by_user(pool, id)
        .await
        .map_err(db_error)?;
    Ok(user_response(row, roles))
}

pub async fn delete(pool: &PgPool, id: i64, actor: Option<&ActorContext>) -> Result<(), ApiError> {
    ensure_actor_can_access(pool, actor, id).await?;
    let actor_user_id = actor.map(|context| context.user_id);
    if actor_user_id == Some(id) {
        return Err(ApiError::forbidden("不能删除当前登录账号"));
    }
    let mut transaction = pool.begin().await.map_err(db_error)?;
    protect_last_super_admin(&mut transaction, id).await?;
    let previous_role_ids = repositories::users::role_ids_by_user(pool, id)
        .await
        .map_err(db_error)?;
    let deleted = repositories::users::soft_delete(&mut transaction, id)
        .await
        .map_err(db_error)?;
    if !deleted {
        return Err(ApiError::not_found("用户不存在"));
    }
    let revoked_sessions = repositories::auth_sessions::revoke_all_for_user(&mut transaction, id)
        .await
        .map_err(db_error)?;
    auth::record_session_revocation(
        &mut transaction,
        actor_user_id,
        id,
        "account_deleted",
        revoked_sessions,
    )
    .await?;
    repositories::audit_logs::record(
        &mut transaction,
        actor_user_id,
        "user.delete",
        "user",
        Some(id),
        json!({ "previousRoleIds": previous_role_ids }),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(())
}

pub async fn delete_many(
    pool: &PgPool,
    actor: Option<&ActorContext>,
    req: &BatchUserIdsRequest,
) -> Result<(), ApiError> {
    let actor = actor.ok_or_else(|| ApiError::forbidden("需要认证用户"))?;
    validate_batch_user_ids(&req.user_ids, actor.user_id)?;
    for &user_id in &req.user_ids {
        ensure_actor_can_access(pool, Some(actor), user_id).await?;
    }

    let mut transaction = pool.begin().await.map_err(db_error)?;
    for &user_id in &req.user_ids {
        protect_last_super_admin(&mut transaction, user_id).await?;
        let previous_role_ids = repositories::users::role_ids_by_user(pool, user_id)
            .await
            .map_err(db_error)?;
        let deleted = repositories::users::soft_delete(&mut transaction, user_id)
            .await
            .map_err(db_error)?;
        if !deleted {
            return Err(ApiError::not_found("用户不存在"));
        }
        let revoked_sessions =
            repositories::auth_sessions::revoke_all_for_user(&mut transaction, user_id)
                .await
                .map_err(db_error)?;
        auth::record_session_revocation(
            &mut transaction,
            Some(actor.user_id),
            user_id,
            "account_deleted",
            revoked_sessions,
        )
        .await?;
        repositories::audit_logs::record(
            &mut transaction,
            Some(actor.user_id),
            "user.delete",
            "user",
            Some(user_id),
            json!({ "previousRoleIds": previous_role_ids }),
        )
        .await
        .map_err(db_error)?;
    }
    transaction.commit().await.map_err(db_error)?;
    Ok(())
}

pub async fn assign_roles(
    pool: &PgPool,
    actor: Option<&ActorContext>,
    user_id: i64,
    req: &AssignRolesRequest,
    can_grant_super_admin: bool,
) -> Result<(), ApiError> {
    ensure_actor_can_access(pool, actor, user_id).await?;
    let actor_user_id = actor.map(|context| context.user_id);
    let super_admin_role_id = repositories::roles::id_by_code(pool, "super_admin")
        .await
        .map_err(db_error)?;
    validate_role_grant_scope(pool, actor_user_id, &req.role_ids, can_grant_super_admin).await?;
    let previous_role_ids = repositories::users::role_ids_by_user(pool, user_id)
        .await
        .map_err(db_error)?;
    let mut transaction = pool.begin().await.map_err(db_error)?;
    if !super_admin_role_id.is_some_and(|id| req.role_ids.contains(&id)) {
        protect_last_super_admin(&mut transaction, user_id).await?;
    }
    repositories::users::assign_roles(&mut transaction, user_id, &req.role_ids)
        .await
        .map_err(db_error)?;
    if let Some(super_admin_role_id) = super_admin_role_id {
        let was_required = previous_role_ids.contains(&super_admin_role_id);
        let is_required = req.role_ids.contains(&super_admin_role_id);
        if was_required != is_required {
            repositories::audit_logs::record(
                &mut transaction,
                actor_user_id,
                "auth.mfa.policy.changed",
                "user",
                Some(user_id),
                json!({"required": is_required}),
            )
            .await
            .map_err(db_error)?;
        }
    }
    repositories::audit_logs::record(
        &mut transaction,
        actor_user_id,
        "user.roles.update",
        "user",
        Some(user_id),
        json!({
            "previousRoleIds": previous_role_ids,
            "roleIds": req.role_ids,
        }),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(())
}

pub async fn assign_roles_many(
    pool: &PgPool,
    actor: Option<&ActorContext>,
    req: &BatchAssignRolesRequest,
    can_grant_super_admin: bool,
) -> Result<(), ApiError> {
    let actor = actor.ok_or_else(|| ApiError::forbidden("需要认证用户"))?;
    validate_batch_user_ids(&req.user_ids, actor.user_id)?;
    validate_role_grant_scope(
        pool,
        Some(actor.user_id),
        &req.role_ids,
        can_grant_super_admin,
    )
    .await?;
    for &user_id in &req.user_ids {
        ensure_actor_can_access(pool, Some(actor), user_id).await?;
    }

    let super_admin_role_id = repositories::roles::id_by_code(pool, "super_admin")
        .await
        .map_err(db_error)?;
    let mut transaction = pool.begin().await.map_err(db_error)?;
    for &user_id in &req.user_ids {
        let previous_role_ids = repositories::users::role_ids_by_user(pool, user_id)
            .await
            .map_err(db_error)?;
        if !super_admin_role_id.is_some_and(|id| req.role_ids.contains(&id)) {
            protect_last_super_admin(&mut transaction, user_id).await?;
        }
        repositories::users::assign_roles(&mut transaction, user_id, &req.role_ids)
            .await
            .map_err(db_error)?;
        if let Some(super_admin_role_id) = super_admin_role_id {
            let was_required = previous_role_ids.contains(&super_admin_role_id);
            let is_required = req.role_ids.contains(&super_admin_role_id);
            if was_required != is_required {
                repositories::audit_logs::record(
                    &mut transaction,
                    Some(actor.user_id),
                    "auth.mfa.policy.changed",
                    "user",
                    Some(user_id),
                    json!({"required": is_required}),
                )
                .await
                .map_err(db_error)?;
            }
        }
        repositories::audit_logs::record(
            &mut transaction,
            Some(actor.user_id),
            "user.roles.update",
            "user",
            Some(user_id),
            json!({
                "previousRoleIds": previous_role_ids,
                "roleIds": req.role_ids,
            }),
        )
        .await
        .map_err(db_error)?;
    }
    transaction.commit().await.map_err(db_error)?;
    Ok(())
}

async fn ensure_actor_can_access(
    pool: &PgPool,
    actor: Option<&ActorContext>,
    user_id: i64,
) -> Result<(), ApiError> {
    let row = match actor {
        Some(actor) => repositories::users::find_by_id_for_actor(pool, actor, user_id).await,
        None => repositories::users::find_by_id(pool, user_id).await,
    }
    .map_err(db_error)?;
    row.ok_or_else(|| ApiError::not_found("用户不存在"))?;
    Ok(())
}

fn validate_batch_user_ids(user_ids: &[i64], actor_user_id: i64) -> Result<(), ApiError> {
    if user_ids.is_empty() {
        return Err(ApiError::validation("至少需要选择一个用户"));
    }
    let unique_ids = user_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if unique_ids.len() != user_ids.len() {
        return Err(ApiError::validation("用户列表不能包含重复项"));
    }
    if unique_ids.contains(&actor_user_id) {
        return Err(ApiError::forbidden("不能批量操作当前登录账号"));
    }
    Ok(())
}

async fn validate_role_grant_scope(
    pool: &PgPool,
    actor_user_id: Option<i64>,
    role_ids: &[i64],
    can_grant_super_admin: bool,
) -> Result<(), ApiError> {
    let unique_role_ids = role_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if unique_role_ids.len() != role_ids.len() {
        return Err(ApiError::validation("角色列表不能包含重复项"));
    }
    let active_role_ids = repositories::roles::active_ids_by_ids(pool, role_ids)
        .await
        .map_err(db_error)?;
    if active_role_ids.len() != unique_role_ids.len() {
        return Err(ApiError::validation("角色列表中包含不存在或已停用的角色"));
    }
    let super_admin_role_id = repositories::roles::id_by_code(pool, "super_admin")
        .await
        .map_err(db_error)?;
    if !can_grant_super_admin && super_admin_role_id.is_some_and(|id| role_ids.contains(&id)) {
        return Err(ApiError::forbidden("缺少授予超级管理员角色的权限"));
    }
    let Some(actor_user_id) = actor_user_id else {
        return Ok(());
    };
    let actor_permissions =
        repositories::permissions::permission_codes_by_user(pool, actor_user_id)
            .await
            .map_err(db_error)?
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
    let requested_permissions = repositories::permissions::codes_by_role_ids(pool, role_ids)
        .await
        .map_err(db_error)?;
    if requested_permissions
        .iter()
        .any(|permission| !actor_permissions.contains(permission))
    {
        return Err(ApiError::forbidden(
            "不能分配包含操作者自身未拥有权限的角色",
        ));
    }
    Ok(())
}

async fn protect_last_super_admin(
    connection: &mut sqlx::PgConnection,
    user_id: i64,
) -> Result<(), ApiError> {
    let (is_active_super_admin, active_count) =
        repositories::users::super_admin_guard_state(connection, user_id)
            .await
            .map_err(db_error)?;
    if !is_active_super_admin {
        return Ok(());
    }
    if active_count <= 1 {
        return Err(ApiError::forbidden(
            "不能停用、删除或移除最后一个处于启用状态的超级管理员",
        ));
    }
    Ok(())
}
