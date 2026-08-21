//! 部门服务：层级校验、业务规则和审计编排。

use crate::access::{ActorContext, DataScope};
use crate::error::{db_error, ApiError};
use crate::models::{
    CreateDepartmentRequest, DepartmentResponse, DepartmentRow, UpdateDepartmentRequest,
};
use crate::repositories;
use serde_json::json;
use sqlx::PgPool;

fn response(row: DepartmentRow) -> DepartmentResponse {
    row.into()
}

pub async fn list(
    pool: &PgPool,
    actor: &ActorContext,
) -> Result<Vec<DepartmentResponse>, ApiError> {
    repositories::departments::list(pool, actor)
        .await
        .map(|rows| rows.into_iter().map(response).collect())
        .map_err(db_error)
}

pub async fn get(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<DepartmentResponse, ApiError> {
    repositories::departments::find_by_id(pool, actor, id)
        .await
        .map_err(db_error)?
        .map(response)
        .ok_or_else(|| ApiError::not_found("部门不存在或超出数据范围"))
}

pub async fn create(
    pool: &PgPool,
    actor: &ActorContext,
    req: &CreateDepartmentRequest,
) -> Result<DepartmentResponse, ApiError> {
    if matches!(
        actor.data_scope,
        DataScope::Department | DataScope::SelfOnly
    ) {
        return Err(ApiError::forbidden("当前数据范围不允许创建下级部门"));
    }
    let code = validate_code(&req.code)?;
    let name = validate_name(&req.name)?;
    let status = validate_status(req.status.as_deref().unwrap_or("active"))?;
    validate_parent(pool, actor, req.parent_id, None).await?;
    ensure_code_available(pool, actor.organization_id, &code, None).await?;

    let department = repositories::departments::NewDepartment {
        parent_id: req.parent_id,
        code: code.clone(),
        name: name.clone(),
        status: status.clone(),
    };
    let mut transaction = pool.begin().await.map_err(db_error)?;
    let id =
        repositories::departments::create(&mut transaction, actor.organization_id, &department)
            .await
            .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut transaction,
        Some(actor.user_id),
        "department.create",
        "department",
        Some(id),
        json!({
            "organizationId": actor.organization_id,
            "parentId": req.parent_id,
            "code": code,
            "name": name,
            "status": status,
        }),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    get(pool, actor, id).await
}

pub async fn update(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
    req: &UpdateDepartmentRequest,
) -> Result<DepartmentResponse, ApiError> {
    if req.parent_id.is_none() && req.code.is_none() && req.name.is_none() && req.status.is_none() {
        return Err(ApiError::validation("至少需要提供一个待更新字段"));
    }
    let current = repositories::departments::find_by_id(pool, actor, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("部门不存在或超出数据范围"))?;
    if current.parent_id.is_none() {
        return Err(ApiError::forbidden("根部门不可修改"));
    }

    let parent_id = req.parent_id;
    let parent_changed = parent_id.is_some_and(|parent_id| current.parent_id != Some(parent_id));
    if parent_changed {
        let parent_id = parent_id.expect("parent_changed implies parent_id is present");
        validate_parent(pool, actor, parent_id, Some(id)).await?;
        if repositories::departments::parent_would_create_cycle(
            pool,
            actor.organization_id,
            id,
            parent_id,
        )
        .await
        .map_err(db_error)?
        {
            return Err(ApiError::validation("上级部门不能是当前部门或其下级部门"));
        }
    }
    let code = req.code.as_deref().map(validate_code).transpose()?;
    if let Some(code) = &code {
        ensure_code_available(pool, actor.organization_id, code, Some(id)).await?;
    }
    let name = req.name.as_deref().map(validate_name).transpose()?;
    let status = req.status.as_deref().map(validate_status).transpose()?;
    let update = repositories::departments::DepartmentUpdate {
        parent_id,
        code: code.clone(),
        name: name.clone(),
        status: status.clone(),
    };

    let mut transaction = pool.begin().await.map_err(db_error)?;
    let updated =
        repositories::departments::update(&mut transaction, actor.organization_id, id, &update)
            .await
            .map_err(db_error)?;
    if !updated {
        return Err(ApiError::not_found("部门不存在"));
    }
    repositories::audit_logs::record(
        &mut transaction,
        Some(actor.user_id),
        "department.update",
        "department",
        Some(id),
        json!({
            "previousParentId": current.parent_id,
            "parentId": parent_id,
            "code": code,
            "name": name,
            "status": status,
        }),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    get(pool, actor, id).await
}

pub async fn delete(pool: &PgPool, actor: &ActorContext, id: i64) -> Result<(), ApiError> {
    let current = repositories::departments::find_by_id(pool, actor, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("部门不存在或超出数据范围"))?;
    if current.parent_id.is_none() {
        return Err(ApiError::forbidden("根部门不可删除"));
    }
    if current.child_count > 0 {
        return Err(ApiError::conflict(
            "部门仍有下级部门，请先移动或删除下级部门",
        ));
    }
    if current.member_count > 0 {
        return Err(ApiError::conflict("部门仍有成员，请先将成员调至其他部门"));
    }

    let mut transaction = pool.begin().await.map_err(db_error)?;
    let deleted =
        repositories::departments::delete_if_empty(&mut transaction, actor.organization_id, id)
            .await
            .map_err(db_error)?;
    if !deleted {
        return Err(ApiError::conflict("部门已被占用，请刷新后重试"));
    }
    repositories::audit_logs::record(
        &mut transaction,
        Some(actor.user_id),
        "department.delete",
        "department",
        Some(id),
        json!({"code": current.code, "name": current.name}),
    )
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    Ok(())
}

pub async fn validate_assignment(
    pool: &PgPool,
    actor: &ActorContext,
    department_id: i64,
) -> Result<(), ApiError> {
    let department = repositories::departments::find_by_id(pool, actor, department_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::validation("目标部门不存在或超出数据范围"))?;
    if department.status != "active" {
        return Err(ApiError::validation("不能将用户分配到已停用部门"));
    }
    Ok(())
}

async fn validate_parent(
    pool: &PgPool,
    actor: &ActorContext,
    parent_id: i64,
    target_id: Option<i64>,
) -> Result<(), ApiError> {
    if target_id == Some(parent_id) {
        return Err(ApiError::validation("部门不能作为自己的上级部门"));
    }
    let parent = repositories::departments::find_by_id(pool, actor, parent_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::validation("上级部门不存在或超出数据范围"))?;
    if parent.status != "active" {
        return Err(ApiError::validation("不能选择已停用部门作为上级部门"));
    }
    Ok(())
}

async fn ensure_code_available(
    pool: &PgPool,
    organization_id: i64,
    code: &str,
    exclude_id: Option<i64>,
) -> Result<(), ApiError> {
    if repositories::departments::code_exists(pool, organization_id, code, exclude_id)
        .await
        .map_err(db_error)?
    {
        return Err(ApiError::conflict("当前组织中已存在相同部门编码"));
    }
    Ok(())
}

fn validate_code(value: &str) -> Result<String, ApiError> {
    let code = value.trim();
    let valid = (2..=64).contains(&code.len())
        && code.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if !valid {
        return Err(ApiError::validation(
            "部门编码需为 2-64 位小写字母、数字、下划线或连字符，且以字母开头",
        ));
    }
    Ok(code.to_string())
}

fn validate_name(value: &str) -> Result<String, ApiError> {
    let name = value.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(ApiError::validation("部门名称长度需在 1-128 个字符之间"));
    }
    Ok(name.to_string())
}

fn validate_status(value: &str) -> Result<String, ApiError> {
    if !matches!(value, "active" | "inactive") {
        return Err(ApiError::validation("部门状态只能为启用或停用"));
    }
    Ok(value.to_string())
}
