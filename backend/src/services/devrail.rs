//! DevRail Phase 0 business validation and transaction orchestration.

use crate::access::ActorContext;
use crate::error::{db_error, ApiError};
use crate::models::*;
use crate::repositories::{self, devrail, devrail_members};
use serde_json::{json, Value};
use sqlx::PgPool;
use url::Url;

const DEFAULT_PAGE: i64 = 1;
const DEFAULT_PAGE_SIZE: i64 = 20;

fn paging(q: &DevRailListQuery) -> Result<(i64, i64), ApiError> {
    let page = q.page.unwrap_or(DEFAULT_PAGE);
    let size = q.page_size.unwrap_or(DEFAULT_PAGE_SIZE);
    if !(1..=10_000).contains(&page) || !(1..=100).contains(&size) {
        return Err(ApiError::validation("分页参数超出范围"));
    }
    Ok((page, size))
}

fn text(value: &str, field: &str, max: usize) -> Result<String, ApiError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max {
        return Err(ApiError::validation(format!(
            "{field}不能为空且不能超过 {max} 个字符"
        )));
    }
    Ok(value.to_string())
}

fn slug(value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if !(3..=64).contains(&value.len())
        || !value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        || value.starts_with('-')
        || value.ends_with('-')
    {
        return Err(ApiError::validation(
            "项目标识需为 3-64 位小写字母、数字或连字符",
        ));
    }
    Ok(value.to_string())
}

fn scope_department(actor: &ActorContext, requested: Option<i64>) -> Result<Option<i64>, ApiError> {
    if requested.is_some()
        && matches!(
            actor.data_scope,
            crate::access::DataScope::Department | crate::access::DataScope::SelfOnly
        )
        && requested != actor.department_id
    {
        return Err(ApiError::forbidden("不能将资源写入当前数据范围之外的部门"));
    }
    Ok(requested.or(actor.department_id))
}

fn project_response(row: DevRailProjectRow) -> DevRailProjectResponse {
    DevRailProjectResponse {
        id: row.id,
        organization_id: row.organization_id,
        department_id: row.department_id,
        owner_user_id: row.owner_user_id,
        slug: row.slug,
        name: row.name,
        description: row.description,
        status: row.status,
        default_repository_id: row.default_repository_id,
        default_environment_id: row.default_environment_id,
        notification_policy: row.notification_policy,
        quality_gate_template: row.quality_gate_template,
        created_at: row.created_at,
        updated_at: row.updated_at,
        archived_at: row.archived_at,
    }
}

fn project_policy_response(row: DevRailProjectRow) -> DevRailProjectPolicyResponse {
    DevRailProjectPolicyResponse {
        project_id: row.id,
        notification_policy: row.notification_policy,
        quality_gate_template: row.quality_gate_template,
    }
}

pub async fn get_project_policy(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<DevRailProjectPolicyResponse, ApiError> {
    devrail::find_project(pool, actor, id)
        .await
        .map_err(db_error)?
        .map(project_policy_response)
        .ok_or_else(|| ApiError::not_found("项目不存在或超出数据范围"))
}

pub async fn update_project_policy(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
    req: &UpdateDevRailProjectPolicyRequest,
) -> Result<DevRailProjectPolicyResponse, ApiError> {
    if req.notification_policy.is_none() && req.quality_gate_template.is_none() {
        return Err(ApiError::validation("至少需要提供一个策略字段"));
    }
    let update = UpdateDevRailProjectRequest {
        name: None,
        description: NullablePatch::Missing,
        department_id: NullablePatch::Missing,
        status: None,
        default_repository_id: NullablePatch::Missing,
        default_environment_id: NullablePatch::Missing,
        notification_policy: req.notification_policy.clone(),
        quality_gate_template: req.quality_gate_template.clone(),
    };
    update_project(pool, actor, id, &update).await?;
    get_project_policy(pool, actor, id).await
}
fn repository_response(row: DevRailRepositoryRow) -> DevRailRepositoryResponse {
    DevRailRepositoryResponse {
        id: row.id,
        organization_id: row.organization_id,
        department_id: row.department_id,
        owner_user_id: row.owner_user_id,
        project_id: row.project_id,
        name: row.name,
        remote_url: row.remote_url,
        protocol: row.protocol,
        default_branch: row.default_branch,
        credential_configured: row.credential_ref.is_some(),
        last_sync_status: row.last_sync_status,
        last_head_sha: row.last_head_sha,
        created_at: row.created_at,
        updated_at: row.updated_at,
        archived_at: row.archived_at,
    }
}
fn environment_response(row: DevRailEnvironmentRow) -> DevRailEnvironmentResponse {
    let secret_ref_names = row
        .secret_refs
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    DevRailEnvironmentResponse {
        id: row.id,
        organization_id: row.organization_id,
        department_id: row.department_id,
        owner_user_id: row.owner_user_id,
        project_id: row.project_id,
        name: row.name,
        workspace_root: row.workspace_root,
        network_mode: row.network_mode,
        tool_policy: row.tool_policy,
        secret_ref_names,
        max_duration_secs: row.max_duration_secs,
        enabled: row.enabled,
        created_at: row.created_at,
        updated_at: row.updated_at,
        archived_at: row.archived_at,
    }
}
fn task_response(row: DevRailTaskRow) -> DevRailTaskResponse {
    DevRailTaskResponse {
        id: row.id,
        organization_id: row.organization_id,
        department_id: row.department_id,
        owner_user_id: row.owner_user_id,
        project_id: row.project_id,
        assignee_user_id: row.assignee_user_id,
        title: row.title,
        goal: row.goal,
        background: row.background,
        acceptance_criteria: row.acceptance_criteria,
        constraints: row.constraints,
        priority: row.priority,
        status: row.status,
        labels: row.labels,
        due_at: row.due_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
        archived_at: row.archived_at,
    }
}

pub async fn list_projects(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
) -> Result<DevRailProjectPage, ApiError> {
    let (page, size) = paging(q)?;
    let (rows, total) = tokio::try_join!(
        devrail::list_projects(pool, actor, q, page, size),
        devrail::count_projects(pool, actor, q)
    )
    .map_err(db_error)?;
    Ok(DevRailProjectPage {
        items: rows.into_iter().map(project_response).collect(),
        total,
        page,
        page_size: size,
    })
}
pub async fn get_project(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<DevRailProjectResponse, ApiError> {
    devrail::find_project(pool, actor, id)
        .await
        .map_err(db_error)?
        .map(project_response)
        .ok_or_else(|| ApiError::not_found("项目不存在或超出数据范围"))
}
pub async fn create_project(
    pool: &PgPool,
    actor: &ActorContext,
    req: &CreateDevRailProjectRequest,
) -> Result<DevRailProjectResponse, ApiError> {
    let slug = slug(&req.slug)?;
    let name = text(&req.name, "项目名称", 128)?;
    let department_id = scope_department(actor, req.department_id)?;
    let notification = req.notification_policy.clone().unwrap_or_else(|| json!({}));
    let quality = req
        .quality_gate_template
        .clone()
        .unwrap_or_else(|| json!({}));
    let mut tx = pool.begin().await.map_err(db_error)?;
    let row = devrail::create_project(
        &mut tx,
        actor,
        &devrail::NewProject {
            slug: &slug,
            name: &name,
            description: req.description.as_deref(),
            department_id,
            notification_policy: &notification,
            quality_gate_template: &quality,
        },
    )
    .await
    .map_err(db_error)?;
    devrail_members::add(&mut tx, actor, row.id, actor.user_id, "owner")
        .await
        .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.project.create",
        "devrail_project",
        Some(row.id),
        json!({"slug":slug,"name":name}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(project_response(row))
}
pub async fn update_project(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
    req: &UpdateDevRailProjectRequest,
) -> Result<DevRailProjectResponse, ApiError> {
    if req.name.is_none()
        && matches!(req.description, NullablePatch::Missing)
        && matches!(req.department_id, NullablePatch::Missing)
        && req.status.is_none()
        && matches!(req.default_repository_id, NullablePatch::Missing)
        && matches!(req.default_environment_id, NullablePatch::Missing)
        && req.notification_policy.is_none()
        && req.quality_gate_template.is_none()
    {
        return Err(ApiError::validation("至少需要提供一个待更新字段"));
    }
    let name = req
        .name
        .as_deref()
        .map(|v| text(v, "项目名称", 128))
        .transpose()?;
    let (department_set, department_id) = nullable_patch(&req.department_id);
    let department_id = if department_set {
        scope_department(actor, department_id)?
    } else {
        None
    };
    let (description_set, description) = nullable_patch(&req.description);
    let (repo_set, repo) = nullable_patch(&req.default_repository_id);
    let (env_set, env) = nullable_patch(&req.default_environment_id);
    if let Some(status) = &req.status {
        if !["active", "archived"].contains(&status.as_str()) {
            return Err(ApiError::validation("项目状态无效"));
        }
    }
    let mut tx = pool.begin().await.map_err(db_error)?;
    let changed = devrail::update_project(
        &mut tx,
        actor,
        id,
        &devrail::ProjectUpdate {
            name: name.as_deref(),
            description_set,
            description: description.as_deref(),
            department_set,
            department_id,
            status: req.status.as_deref(),
            default_repository_set: repo_set,
            default_repository_id: repo,
            default_environment_set: env_set,
            default_environment_id: env,
            notification_policy: req.notification_policy.as_ref(),
            quality_gate_template: req.quality_gate_template.as_ref(),
        },
    )
    .await
    .map_err(db_error)?;
    if !changed {
        return Err(ApiError::not_found("项目不存在或超出数据范围"));
    }
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.project.update",
        "devrail_project",
        Some(id),
        json!({"fields":["project"]}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    get_project(pool, actor, id).await
}
pub async fn archive_project(pool: &PgPool, actor: &ActorContext, id: i64) -> Result<(), ApiError> {
    let mut tx = pool.begin().await.map_err(db_error)?;
    if !devrail::archive_project(&mut tx, actor, id)
        .await
        .map_err(db_error)?
    {
        return Err(ApiError::not_found("项目不存在或超出数据范围"));
    }
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.project.archive",
        "devrail_project",
        Some(id),
        json!({}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)
}

fn remote(value: &str) -> Result<(String, String), ApiError> {
    let value = value.trim();
    if value.starts_with("git@") && value.contains(':') {
        return Ok((value.to_string(), "ssh".to_string()));
    }
    let parsed =
        Url::parse(value).map_err(|_| ApiError::validation("仓库地址必须是 HTTPS 或 SSH 地址"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(ApiError::validation("仓库 HTTPS 地址不允许携带凭据"));
    }
    Ok((value.to_string(), "https".to_string()))
}
pub async fn list_repositories(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
) -> Result<DevRailRepositoryPage, ApiError> {
    let project_id = q
        .project_id
        .ok_or_else(|| ApiError::validation("缺少 projectId"))?;
    let (page, size) = paging(q)?;
    let q2 = DevRailListQuery {
        project_id: Some(project_id),
        ..(*q).clone()
    };
    let (rows, total) = tokio::try_join!(
        devrail::list_repositories(pool, actor, &q2, page, size),
        devrail::count_repositories(pool, actor, &q2)
    )
    .map_err(db_error)?;
    Ok(DevRailRepositoryPage {
        items: rows.into_iter().map(repository_response).collect(),
        total,
        page,
        page_size: size,
    })
}
pub async fn get_repository(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
) -> Result<DevRailRepositoryResponse, ApiError> {
    devrail::find_repository(pool, actor, project_id, id)
        .await
        .map_err(db_error)?
        .map(repository_response)
        .ok_or_else(|| ApiError::not_found("仓库不存在或超出数据范围"))
}
pub async fn create_repository(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    req: &CreateDevRailRepositoryRequest,
) -> Result<DevRailRepositoryResponse, ApiError> {
    get_project(pool, actor, project_id).await?;
    let name = text(&req.name, "仓库名称", 128)?;
    let (url, protocol) = remote(&req.remote_url)?;
    let branch = text(
        req.default_branch.as_deref().unwrap_or("main"),
        "默认分支",
        128,
    )?;
    let department_id = scope_department(actor, req.department_id)?;
    let mut tx = pool.begin().await.map_err(db_error)?;
    let row = devrail::create_repository(
        &mut tx,
        actor,
        &devrail::NewRepository {
            project_id,
            name: &name,
            remote_url: &url,
            protocol: &protocol,
            default_branch: &branch,
            credential_ref: req.credential_ref.as_deref(),
            department_id,
        },
    )
    .await
    .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.repository.create",
        "devrail_repository",
        Some(row.id),
        json!({"projectId":project_id,"name":name,"protocol":protocol}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(repository_response(row))
}
pub async fn update_repository(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
    req: &UpdateDevRailRepositoryRequest,
) -> Result<DevRailRepositoryResponse, ApiError> {
    let (credential_set, credential) = nullable_patch(&req.credential_ref);
    let remote_value = req.remote_url.as_deref().map(remote).transpose()?;
    let name = req
        .name
        .as_deref()
        .map(|v| text(v, "仓库名称", 128))
        .transpose()?;
    let branch = req
        .default_branch
        .as_deref()
        .map(|v| text(v, "默认分支", 128))
        .transpose()?;
    if req.name.is_none()
        && req.remote_url.is_none()
        && req.default_branch.is_none()
        && !credential_set
        && req.status.is_none()
    {
        return Err(ApiError::validation("至少需要提供一个待更新字段"));
    }
    let mut tx = pool.begin().await.map_err(db_error)?;
    if !devrail::update_repository(
        &mut tx,
        actor,
        project_id,
        id,
        &devrail::RepositoryUpdate {
            name: name.as_deref(),
            remote_url: remote_value.as_ref().map(|v| v.0.as_str()),
            protocol: remote_value.as_ref().map(|v| v.1.as_str()),
            default_branch: branch.as_deref(),
            credential_set,
            credential_ref: credential.as_deref(),
            status: req.status.as_deref(),
        },
    )
    .await
    .map_err(db_error)?
    {
        return Err(ApiError::not_found("仓库不存在或超出数据范围"));
    }
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.repository.update",
        "devrail_repository",
        Some(id),
        json!({"projectId":project_id}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    get_repository(pool, actor, project_id, id).await
}

fn workspace(value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if !value.starts_with('/') || value.contains("..") || value.len() > 512 {
        return Err(ApiError::validation("工作区必须是受控根目录下的绝对路径"));
    }
    Ok(value.to_string())
}
fn network(value: &str) -> Result<String, ApiError> {
    if ["off", "allowlist"].contains(&value) {
        Ok(value.to_string())
    } else {
        Err(ApiError::validation("网络模式只能是 off 或 allowlist"))
    }
}
fn duration(value: i64) -> Result<i64, ApiError> {
    if (60..=86400).contains(&value) {
        Ok(value)
    } else {
        Err(ApiError::validation("最大运行时长必须在 60-86400 秒之间"))
    }
}
pub async fn list_environments(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
) -> Result<DevRailEnvironmentPage, ApiError> {
    let project_id = q
        .project_id
        .ok_or_else(|| ApiError::validation("缺少 projectId"))?;
    let (page, size) = paging(q)?;
    let q2 = DevRailListQuery {
        project_id: Some(project_id),
        ..(*q).clone()
    };
    let (rows, total) = tokio::try_join!(
        devrail::list_environments(pool, actor, &q2, page, size),
        devrail::count_environments(pool, actor, &q2)
    )
    .map_err(db_error)?;
    Ok(DevRailEnvironmentPage {
        items: rows.into_iter().map(environment_response).collect(),
        total,
        page,
        page_size: size,
    })
}
pub async fn get_environment(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
) -> Result<DevRailEnvironmentResponse, ApiError> {
    devrail::find_environment(pool, actor, project_id, id)
        .await
        .map_err(db_error)?
        .map(environment_response)
        .ok_or_else(|| ApiError::not_found("环境不存在或超出数据范围"))
}
pub async fn create_environment(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    req: &CreateDevRailEnvironmentRequest,
) -> Result<DevRailEnvironmentResponse, ApiError> {
    get_project(pool, actor, project_id).await?;
    let name = text(&req.name, "环境名称", 128)?;
    let root = workspace(&req.workspace_root)?;
    let network_mode = network(req.network_mode.as_deref().unwrap_or("off"))?;
    let max = duration(req.max_duration_secs.unwrap_or(3600))?;
    let refs = json!(req.secret_ref_names.clone().unwrap_or_default());
    let department_id = scope_department(actor, req.department_id)?;
    let policy = req.tool_policy.clone().unwrap_or_else(|| json!({}));
    let mut tx = pool.begin().await.map_err(db_error)?;
    let row = devrail::create_environment(
        &mut tx,
        actor,
        &devrail::NewEnvironment {
            project_id,
            name: &name,
            workspace_root: &root,
            network_mode: &network_mode,
            tool_policy: &policy,
            secret_refs: &refs,
            max_duration_secs: max,
            enabled: req.enabled.unwrap_or(true),
            department_id,
        },
    )
    .await
    .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.environment.create",
        "devrail_environment",
        Some(row.id),
        json!({"projectId":project_id,"name":name}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(environment_response(row))
}
pub async fn update_environment(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
    req: &UpdateDevRailEnvironmentRequest,
) -> Result<DevRailEnvironmentResponse, ApiError> {
    let name = req
        .name
        .as_deref()
        .map(|v| text(v, "环境名称", 128))
        .transpose()?;
    let root = req.workspace_root.as_deref().map(workspace).transpose()?;
    let network_mode = req.network_mode.as_deref().map(network).transpose()?;
    let max = req.max_duration_secs.map(duration).transpose()?;
    let refs = req.secret_ref_names.as_ref().map(|v| json!(v));
    if name.is_none()
        && root.is_none()
        && network_mode.is_none()
        && req.tool_policy.is_none()
        && refs.is_none()
        && max.is_none()
        && req.enabled.is_none()
    {
        return Err(ApiError::validation("至少需要提供一个待更新字段"));
    }
    let mut tx = pool.begin().await.map_err(db_error)?;
    if !devrail::update_environment(
        &mut tx,
        actor,
        project_id,
        id,
        &devrail::EnvironmentUpdate {
            name: name.as_deref(),
            workspace_root: root.as_deref(),
            network_mode: network_mode.as_deref(),
            tool_policy: req.tool_policy.as_ref(),
            secret_refs: refs.as_ref(),
            max_duration_secs: max,
            enabled: req.enabled,
        },
    )
    .await
    .map_err(db_error)?
    {
        return Err(ApiError::not_found("环境不存在或超出数据范围"));
    }
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.environment.update",
        "devrail_environment",
        Some(id),
        json!({"projectId":project_id}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    get_environment(pool, actor, project_id, id).await
}

fn priority(value: &str) -> Result<String, ApiError> {
    if ["low", "normal", "high", "urgent"].contains(&value) {
        Ok(value.to_string())
    } else {
        Err(ApiError::validation("任务优先级无效"))
    }
}
fn task_status(value: &str) -> Result<String, ApiError> {
    if [
        "draft",
        "queued",
        "running",
        "awaiting_approval",
        "succeeded",
        "failed",
        "cancelled",
        "archived",
    ]
    .contains(&value)
    {
        Ok(value.to_string())
    } else {
        Err(ApiError::validation("任务状态无效"))
    }
}
pub async fn list_tasks(
    pool: &PgPool,
    actor: &ActorContext,
    q: &DevRailListQuery,
) -> Result<DevRailTaskPage, ApiError> {
    let project_id = q
        .project_id
        .ok_or_else(|| ApiError::validation("缺少 projectId"))?;
    let (page, size) = paging(q)?;
    let q2 = DevRailListQuery {
        project_id: Some(project_id),
        ..(*q).clone()
    };
    let (rows, total) = tokio::try_join!(
        devrail::list_tasks(pool, actor, &q2, page, size),
        devrail::count_tasks(pool, actor, &q2)
    )
    .map_err(db_error)?;
    Ok(DevRailTaskPage {
        items: rows.into_iter().map(task_response).collect(),
        total,
        page,
        page_size: size,
    })
}
pub async fn get_task(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
) -> Result<DevRailTaskResponse, ApiError> {
    devrail::find_task(pool, actor, project_id, id)
        .await
        .map_err(db_error)?
        .map(task_response)
        .ok_or_else(|| ApiError::not_found("任务不存在或超出数据范围"))
}
pub async fn create_task(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    req: &CreateDevRailTaskRequest,
) -> Result<DevRailTaskResponse, ApiError> {
    get_project(pool, actor, project_id).await?;
    let title = text(&req.title, "任务标题", 200)?;
    let goal = text(&req.goal, "任务目标", 4000)?;
    let priority = priority(req.priority.as_deref().unwrap_or("normal"))?;
    let labels = json!(req.labels.clone().unwrap_or_default());
    let department_id = scope_department(actor, req.department_id)?;
    let mut tx = pool.begin().await.map_err(db_error)?;
    let row = devrail::create_task(
        &mut tx,
        actor,
        &devrail::NewTask {
            project_id,
            assignee_user_id: req.assignee_user_id,
            title: &title,
            goal: &goal,
            background: req.background.as_deref(),
            acceptance_criteria: req.acceptance_criteria.as_deref(),
            constraints: req.constraints.as_deref(),
            priority: &priority,
            labels: &labels,
            due_at: req.due_at,
            department_id,
        },
    )
    .await
    .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.task.create",
        "devrail_task",
        Some(row.id),
        json!({"projectId":project_id,"title":title}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(task_response(row))
}
pub async fn update_task(
    pool: &PgPool,
    actor: &ActorContext,
    project_id: i64,
    id: i64,
    req: &UpdateDevRailTaskRequest,
) -> Result<DevRailTaskResponse, ApiError> {
    let title = req
        .title
        .as_deref()
        .map(|v| text(v, "任务标题", 200))
        .transpose()?;
    let goal = req
        .goal
        .as_deref()
        .map(|v| text(v, "任务目标", 4000))
        .transpose()?;
    let priority = req.priority.as_deref().map(priority).transpose()?;
    let status = req.status.as_deref().map(task_status).transpose()?;
    let (background_set, background) = nullable_patch(&req.background);
    let (acceptance_set, acceptance_criteria) = nullable_patch(&req.acceptance_criteria);
    let (constraints_set, constraints) = nullable_patch(&req.constraints);
    let (assignee_set, assignee_user_id) = nullable_patch(&req.assignee_user_id);
    let (due_at_set, due_at) = nullable_patch(&req.due_at);
    if title.is_none()
        && goal.is_none()
        && !background_set
        && !acceptance_set
        && !constraints_set
        && priority.is_none()
        && status.is_none()
        && !assignee_set
        && req.labels.is_none()
        && !due_at_set
    {
        return Err(ApiError::validation("至少需要提供一个待更新字段"));
    }
    let labels = req.labels.as_ref().map(|v| json!(v));
    let mut tx = pool.begin().await.map_err(db_error)?;
    if !devrail::update_task(
        &mut tx,
        actor,
        project_id,
        id,
        &devrail::TaskUpdate {
            title: title.as_deref(),
            goal: goal.as_deref(),
            background_set,
            background: background.as_deref(),
            acceptance_set,
            acceptance_criteria: acceptance_criteria.as_deref(),
            constraints_set,
            constraints: constraints.as_deref(),
            priority: priority.as_deref(),
            status: status.as_deref(),
            assignee_set,
            assignee_user_id,
            labels: labels.as_ref(),
            due_at_set,
            due_at,
        },
    )
    .await
    .map_err(db_error)?
    {
        return Err(ApiError::not_found("任务不存在或超出数据范围"));
    }
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.task.update",
        "devrail_task",
        Some(id),
        json!({"projectId":project_id}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    get_task(pool, actor, project_id, id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_project_slugs_and_remote_credentials() {
        assert!(slug("devrail-core").is_ok());
        assert!(slug("DevRail").is_err());
        assert_eq!(remote("git@example.com:team/repo.git").unwrap().1, "ssh");
        let credential_url = [
            "https",
            "://",
            "user",
            ":",
            "credential",
            "@example.com/repo.git",
        ]
        .concat();
        assert!(remote(&credential_url).is_err());
    }

    #[test]
    fn rejects_uncontrolled_workspace_and_invalid_limits() {
        assert!(workspace("/srv/devrail/workspaces/project").is_ok());
        assert!(workspace("relative/path").is_err());
        assert!(workspace("/srv/../etc").is_err());
        assert!(duration(3600).is_ok());
        assert!(duration(30).is_err());
    }
}
