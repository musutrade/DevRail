use crate::access::ActorContext;
use crate::error::ApiError;
use crate::models::*;
use crate::repositories::{self, devrail, devrail_runs};
use crate::workers::harness_supervisor::{HarnessSupervisor, RunLaunch};
use serde_json::json;
use sqlx::PgPool;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;

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
fn string_field(payload: &serde_json::Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}
fn integer_field(payload: &serde_json::Value, key: &str) -> Option<i64> {
    payload.get(key).and_then(|value| value.as_i64())
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

fn quality_gate_commands(
    template: &serde_json::Value,
) -> Result<Vec<(String, Vec<String>)>, ApiError> {
    let gates = template
        .get("gates")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ApiError::validation("质量门禁模板必须包含 gates 数组"))?;
    if gates.is_empty() || gates.len() > 8 {
        return Err(ApiError::validation("质量门禁数量必须为 1-8 项"));
    }
    gates
        .iter()
        .map(|gate| {
            let name = gate
                .get("name")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty() && value.len() <= 128)
                .ok_or_else(|| ApiError::validation("质量门禁缺少有效名称"))?
                .to_string();
            let command = gate
                .get("command")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| ApiError::validation("质量门禁缺少命令"))?;
            let args: Vec<String> = command
                .split_ascii_whitespace()
                .map(str::to_string)
                .collect();
            let allowed = (args.len() == 2
                && args[0] == "cargo"
                && matches!(args[1].as_str(), "check" | "test" | "clippy" | "fmt"))
                || (args.len() == 3
                    && args[0] == "cargo"
                    && args[1] == "flow"
                    && args[2] == "verify")
                || (args.len() == 3
                    && args[0] == "npm"
                    && args[1] == "run"
                    && matches!(args[2].as_str(), "lint" | "test:ci" | "build"));
            if !allowed {
                return Err(ApiError::validation("质量门禁命令不在允许列表中"));
            }
            Ok((name, args))
        })
        .collect()
}

fn gate_summary(output: &[u8]) -> String {
    String::from_utf8_lossy(output)
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .unwrap_or("命令未输出摘要")
        .chars()
        .take(500)
        .collect()
}

fn gate_log_lines(output: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(output)
        .lines()
        .take(200)
        .map(|line| {
            let lowered = line.to_ascii_lowercase();
            if [
                "authorization",
                "cookie",
                "token",
                "password",
                "private key",
                "secret",
            ]
            .iter()
            .any(|needle| lowered.contains(needle))
            {
                "[日志已脱敏]".to_string()
            } else {
                line.chars().take(1000).collect()
            }
        })
        .collect()
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
    let policy = json!({"version":"devrail-policy-v1","networkMode":environment.network_mode,"toolPolicy":environment.tool_policy,"secretRefs":[]});
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

pub async fn get_changeset(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<DevRailChangesetResponse, ApiError> {
    let _ = get_run(pool, actor, id).await?;
    let rows = devrail_runs::list_events(pool, actor, id, 0, 500)
        .await
        .map_err(db_error)?;
    let files = rows
        .into_iter()
        .filter(|row| row.event_type == "file_change")
        .map(|row| DevRailChangeFileResponse {
            path: string_field(&row.payload, "path").unwrap_or_else(|| "未提供路径".to_string()),
            status: string_field(&row.payload, "status").unwrap_or_else(|| "modified".to_string()),
            additions: integer_field(&row.payload, "additions"),
            deletions: integer_field(&row.payload, "deletions"),
            summary: row.summary,
        })
        .collect();
    Ok(DevRailChangesetResponse { run_id: id, files })
}

pub async fn get_quality_gates(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<DevRailQualityGatePage, ApiError> {
    let _ = get_run(pool, actor, id).await?;
    let rows = devrail_runs::list_events(pool, actor, id, 0, 500)
        .await
        .map_err(db_error)?;
    let items = rows
        .into_iter()
        .filter(|row| row.event_type == "quality_gate")
        .map(|row| DevRailQualityGateResponse {
            name: string_field(&row.payload, "name").unwrap_or_else(|| "质量门禁".to_string()),
            status: string_field(&row.payload, "status").unwrap_or_else(|| "unknown".to_string()),
            command_summary: string_field(&row.payload, "command_summary"),
            executor_version: string_field(&row.payload, "executor_version"),
            log_ref: string_field(&row.payload, "log_ref"),
            exit_code: integer_field(&row.payload, "exit_code"),
            duration_ms: integer_field(&row.payload, "duration_ms"),
            summary: row.summary,
        })
        .collect();
    Ok(DevRailQualityGatePage { run_id: id, items })
}

pub async fn get_quality_gate_log(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
    log_ref: &str,
    after_cursor: i64,
    limit: i64,
) -> Result<DevRailQualityGateLogPage, ApiError> {
    let _ = get_run(pool, actor, id).await?;
    let expected_prefix = format!("run-event:{id}:quality-gate:");
    if !log_ref.starts_with(&expected_prefix) {
        return Err(ApiError::validation("日志引用与运行不匹配"));
    }
    let row = devrail_runs::find_quality_gate_log(pool, actor, id, log_ref)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("质量门禁日志不存在或超出数据范围"))?;
    let lines = row
        .payload
        .get("log_lines")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let start = after_cursor.max(0) as usize;
    let page_size = limit.clamp(1, 200) as usize;
    let page = lines
        .iter()
        .skip(start)
        .take(page_size)
        .cloned()
        .collect::<Vec<_>>();
    let next_cursor = (start + page.len() < lines.len()).then_some((start + page.len()) as i64);
    Ok(DevRailQualityGateLogPage {
        run_id: id,
        log_ref: log_ref.to_string(),
        name: string_field(&row.payload, "name").unwrap_or_else(|| "质量门禁".to_string()),
        status: string_field(&row.payload, "status").unwrap_or_else(|| "unknown".to_string()),
        lines: page,
        next_cursor,
    })
}

pub async fn execute_quality_gates(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
) -> Result<DevRailQualityGatePage, ApiError> {
    let run = get_run(pool, actor, id).await?;
    if !matches!(run.status.as_str(), "completed" | "failed") {
        return Err(ApiError::conflict("运行尚未结束，不能执行质量门禁"));
    }
    let task = devrail::find_task_by_id(pool, actor, run.task_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("任务不存在或超出数据范围"))?;
    let project = devrail::find_project(pool, actor, task.project_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("项目不存在或超出数据范围"))?;
    let commands = quality_gate_commands(&project.quality_gate_template)?;
    let mut failed = false;
    for (index, (name, args)) in commands.iter().enumerate() {
        let started = Instant::now();
        let output = tokio::time::timeout(
            Duration::from_secs(900),
            Command::new(&args[0])
                .args(&args[1..])
                .current_dir(&run.cwd)
                .env_clear()
                .env("PATH", "/usr/local/bin:/usr/bin:/bin")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await;
        let (status, exit_code, summary) = match output {
            Ok(Ok(ref output)) if output.status.success() => {
                ("passed", output.status.code(), gate_summary(&output.stdout))
            }
            Ok(Ok(ref output)) => {
                failed = true;
                ("failed", output.status.code(), gate_summary(&output.stderr))
            }
            Ok(Err(_)) => {
                failed = true;
                ("failed", None, "质量门禁命令无法启动".to_string())
            }
            Err(_) => {
                failed = true;
                ("failed", None, "质量门禁执行超时".to_string())
            }
        };
        let payload = json!({
            "name": name,
            "status": status,
            "command_summary": args.join(" "),
            "executor_version": "devrail-gate-v1",
            "log_ref": format!("run-event:{id}:quality-gate:{index}"),
            "exit_code": exit_code,
            "duration_ms": started.elapsed().as_millis() as i64,
            "log_lines": gate_log_lines(match &output {
                Ok(Ok(output)) => if output.status.success() { &output.stdout } else { &output.stderr },
                _ => &[],
            }),
        });
        let mut tx = pool.begin().await.map_err(db_error)?;
        devrail_runs::append_event(
            &mut tx,
            &devrail_runs::NewRunEvent {
                run_id: id,
                organization_id: actor.organization_id,
                department_id: task.department_id,
                owner_user_id: actor.user_id,
                event_type: "quality_gate",
                source_event_id: None,
                idempotency_key: &format!("quality-gate-{id}-{index}"),
                payload: &payload,
                summary: Some(&summary),
            },
        )
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
    }
    let mut tx = pool.begin().await.map_err(db_error)?;
    if failed {
        devrail_runs::mark_quality_gate_failed(&mut tx, id, task.id)
            .await
            .map_err(db_error)?;
    }
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.quality_gate.execute",
        "devrail_run",
        Some(id),
        json!({"taskId":task.id,"failed":failed,"count":commands.len()}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    get_quality_gates(pool, actor, id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_gate_commands_accept_only_allowlisted_tools() {
        let template = serde_json::json!({"gates":[
            {"name":"检查","command":"cargo check"},
            {"name":"测试","command":"npm run test:ci"}
        ]});
        assert_eq!(quality_gate_commands(&template).unwrap().len(), 2);
        let rejected = serde_json::json!({"gates":[{"name":"危险","command":"sh -c rm -rf"}]});
        assert!(quality_gate_commands(&rejected).is_err());
    }
}
