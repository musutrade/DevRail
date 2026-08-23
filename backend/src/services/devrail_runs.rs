use crate::access::ActorContext;
use crate::error::ApiError;
use crate::models::*;
use crate::repositories::{self, devrail, devrail_runs};
use crate::workers::harness_supervisor::{HarnessSupervisor, RunLaunch};
use serde_json::json;
use sqlx::PgPool;
use std::path::PathBuf;

fn db_error(error: sqlx::Error) -> ApiError {
    ApiError::internal(error)
}
#[derive(Debug, Clone)]
pub struct ResumeContext {
    pub thread_id: String,
    pub turn_id: Option<String>,
}
fn run_response(row: DevRailRunRow) -> DevRailRunResponse {
    DevRailRunResponse {
        id: row.id,
        task_id: row.task_id,
        snapshot_id: row.snapshot_id,
        idempotency_key: row.idempotency_key,
        status: row.status,
        thread_id: row.thread_id,
        turn_id: row.turn_id,
        harness_version: row.harness_version,
        model_id: row.model_id,
        cwd: row.cwd,
        policy: row.policy,
        startup_args_summary: row.startup_args_summary,
        exit_reason: row.exit_reason,
        exit_code: row.exit_code,
        stderr_summary: row.stderr_summary,
        trace_id: row.trace_id,
        recovery_suggestion: row.recovery_suggestion,
        started_at: row.started_at,
        completed_at: row.completed_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}
fn event_response(row: DevRailRunEventRow) -> DevRailRunEventResponse {
    DevRailRunEventResponse {
        cursor: row.cursor,
        event_type: row.event_type,
        source_event_id: row.source_event_id,
        payload: row.payload,
        summary: row.summary,
        occurred_at: row.occurred_at,
    }
}
fn key(value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
    {
        return Err(ApiError::validation("idempotencyKey 格式无效"));
    }
    Ok(value.to_string())
}
fn bounded_input(value: Option<&str>, fallback: &str) -> Result<String, ApiError> {
    let input = value.unwrap_or(fallback).trim();
    if input.is_empty() || input.len() > 16_000 {
        return Err(ApiError::validation(
            "运行输入不能为空且不得超过 16000 个字符",
        ));
    }
    Ok(input.to_string())
}

pub async fn create_run(
    pool: &PgPool,
    actor: &ActorContext,
    supervisor: &HarnessSupervisor,
    task_id: i64,
    req: &CreateDevRailRunRequest,
) -> Result<DevRailRunResponse, ApiError> {
    create_run_with_resume(pool, actor, supervisor, task_id, req, None).await
}

async fn create_run_with_resume(
    pool: &PgPool,
    actor: &ActorContext,
    supervisor: &HarnessSupervisor,
    task_id: i64,
    req: &CreateDevRailRunRequest,
    resume: Option<ResumeContext>,
) -> Result<DevRailRunResponse, ApiError> {
    let idempotency_key = key(&req.idempotency_key)?;
    if let Some(existing) =
        devrail_runs::find_run_by_idempotency(pool, actor, task_id, &idempotency_key)
            .await
            .map_err(db_error)?
    {
        return Ok(run_response(existing));
    }
    let task = devrail::find_task_by_id(pool, actor, task_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("任务不存在或超出数据范围"))?;
    let environment = devrail::find_environment(pool, actor, task.project_id, req.environment_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("环境不存在或超出数据范围"))?;
    if !environment.enabled {
        return Err(ApiError::conflict("运行环境已禁用"));
    }
    let cwd = PathBuf::from(&environment.workspace_root);
    let input = bounded_input(req.input.as_deref(), &task.goal)?;
    let snapshot = json!({"taskId":task.id,"projectId":task.project_id,"title":task.title,"goal":task.goal,"background":task.background,"acceptanceCriteria":task.acceptance_criteria,"constraints":task.constraints,"labels":task.labels,"environmentId":environment.id,"workspaceRoot":environment.workspace_root,"networkMode":environment.network_mode,"toolPolicy":environment.tool_policy});
    let policy = json!({"networkMode":environment.network_mode,"toolPolicy":environment.tool_policy,"secretRefs":[]});
    let startup_args = json!(["app-server"]);
    let mut tx = pool.begin().await.map_err(db_error)?;
    let snapshot_id =
        devrail_runs::create_snapshot(&mut tx, actor, task.id, &snapshot, task.department_id)
            .await
            .map_err(db_error)?;
    let row = devrail_runs::create_run(
        &mut tx,
        &devrail_runs::NewRun {
            actor,
            task_id: task.id,
            snapshot_id,
            idempotency_key: &idempotency_key,
            cwd: &environment.workspace_root,
            policy: &policy,
            startup_args: &startup_args,
            model_id: req.model_id.as_deref(),
            department_id: task.department_id,
        },
    )
    .await
    .map_err(|error| {
        if let sqlx::Error::Database(db) = &error {
            if db
                .constraint()
                .is_some_and(|c| c == "uq_devrail_active_run_per_task")
            {
                return ApiError::conflict("该任务已有活动运行");
            }
        }
        db_error(error)
    })?;
    devrail_runs::update_task_status(&mut tx, task.id, "running")
        .await
        .map_err(db_error)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.run.create",
        "devrail_run",
        Some(row.id),
        json!({"taskId":task.id,"environmentId":environment.id}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    if let Err(error) = supervisor
        .launch(RunLaunch {
            run_id: row.id,
            task_id: task.id,
            organization_id: actor.organization_id,
            department_id: task.department_id,
            owner_user_id: actor.user_id,
            cwd,
            input,
            resume_thread_id: resume.as_ref().map(|value| value.thread_id.clone()),
            resume_turn_id: resume.and_then(|value| value.turn_id),
        })
        .await
    {
        let mut cleanup = pool.begin().await.map_err(db_error)?;
        let trace = uuid::Uuid::new_v4().to_string();
        devrail_runs::update_run_terminal(
            &mut cleanup,
            &devrail_runs::TerminalRunUpdate {
                run_id: row.id,
                status: "failed",
                exit_reason: "launch_failed",
                exit_code: None,
                stderr_summary: None,
                trace_id: &trace,
                recovery_suggestion: Some("Harness 进程未能启动；检查命令配置和受控工作区"),
            },
        )
        .await
        .map_err(db_error)?;
        devrail_runs::update_task_status(&mut cleanup, task.id, "failed")
            .await
            .map_err(db_error)?;
        cleanup.commit().await.map_err(db_error)?;
        return Err(ApiError::conflict(error.to_string()));
    }
    Ok(run_response(row))
}
pub async fn get_run(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<DevRailRunResponse, ApiError> {
    devrail_runs::find_run(pool, actor, id)
        .await
        .map_err(db_error)?
        .map(run_response)
        .ok_or_else(|| ApiError::not_found("运行不存在或超出数据范围"))
}
pub async fn list_runs(
    pool: &PgPool,
    actor: &ActorContext,
    task_id: i64,
    page: i64,
    size: i64,
) -> Result<DevRailRunPage, ApiError> {
    let (items, total) = tokio::try_join!(
        devrail_runs::list_runs(pool, actor, task_id, page, size),
        devrail_runs::count_runs(pool, actor, task_id)
    )
    .map_err(db_error)?;
    Ok(DevRailRunPage {
        items: items.into_iter().map(run_response).collect(),
        total,
        page,
        page_size: size,
    })
}
pub async fn interrupt_run(
    pool: &PgPool,
    actor: &ActorContext,
    supervisor: &HarnessSupervisor,
    id: i64,
) -> Result<DevRailRunResponse, ApiError> {
    let run = devrail_runs::find_run(pool, actor, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("运行不存在或超出数据范围"))?;
    if matches!(run.status.as_str(), "completed" | "failed" | "cancelled") {
        return Ok(run_response(run));
    }
    supervisor
        .interrupt(id)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    get_run(pool, actor, id).await
}

pub async fn retry_run(
    pool: &PgPool,
    actor: &ActorContext,
    supervisor: &HarnessSupervisor,
    id: i64,
    req: &RetryDevRailRunRequest,
) -> Result<DevRailRunResponse, ApiError> {
    let previous = devrail_runs::find_run(pool, actor, id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("运行不存在或超出数据范围"))?;
    if !matches!(
        previous.status.as_str(),
        "completed" | "failed" | "cancelled"
    ) {
        return Err(ApiError::conflict("只有已结束的运行可以重试"));
    }
    let snapshot = devrail_runs::find_snapshot(pool, actor, previous.snapshot_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("运行快照不存在"))?;
    let environment_id = snapshot
        .get("environmentId")
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| ApiError::conflict("运行快照缺少环境信息"))?;
    let resume = if let Some(turn_id) = req.resume_from_turn_id.as_ref() {
        Some(ResumeContext {
            thread_id: previous
                .thread_id
                .clone()
                .ok_or_else(|| ApiError::conflict("原运行尚未建立 Codex thread，无法恢复"))?,
            turn_id: Some(turn_id.clone()),
        })
    } else {
        None
    };
    create_run_with_resume(
        pool,
        actor,
        supervisor,
        previous.task_id,
        &CreateDevRailRunRequest {
            environment_id,
            idempotency_key: req.idempotency_key.clone(),
            model_id: previous.model_id,
            input: req.input.clone(),
        },
        resume,
    )
    .await
}
pub async fn list_events(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
    after_cursor: i64,
    limit: i64,
) -> Result<DevRailRunEventPage, ApiError> {
    let _ = get_run(pool, actor, id).await?;
    let rows = devrail_runs::list_events(pool, actor, id, after_cursor, limit.clamp(1, 500))
        .await
        .map_err(db_error)?;
    Ok(DevRailRunEventPage {
        next_cursor: rows.last().map(|r| r.cursor),
        items: rows.into_iter().map(event_response).collect(),
    })
}
