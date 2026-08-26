use crate::access::ActorContext;
use crate::error::ApiError;
use crate::models::*;
use crate::repositories::{self, devrail, devrail_runs};
use crate::workers::harness_supervisor::{HarnessSupervisor, RunLaunch, SupervisorError};
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
struct ResumeContext {
    pub thread_id: String,
    pub turn_id: Option<String>,
}

#[derive(Default)]
struct RunCreationContext {
    resume: Option<ResumeContext>,
    scheduler_claim_token: Option<uuid::Uuid>,
    parent_run_id: Option<i64>,
    parent_turn_id: Option<String>,
}
fn run_response(row: DevRailRunRow) -> DevRailRunResponse {
    DevRailRunResponse {
        id: row.id,
        task_id: row.task_id,
        snapshot_id: row.snapshot_id,
        idempotency_key: row.idempotency_key,
        attempt: row.attempt,
        task_revision: row.task_revision,
        workflow_source: row.workflow_source,
        workflow_version: row.workflow_version,
        workflow_digest: row.workflow_digest,
        actor_type: row.actor_type,
        last_heartbeat_at: row.last_heartbeat_at,
        last_event_at: row.last_event_at,
        retry_reason: row.retry_reason,
        parent_run_id: row.parent_run_id,
        parent_turn_id: row.parent_turn_id,
        cleanup_status: row.cleanup_status,
        branch_name: row.branch_name,
        branch_expires_at: row.branch_expires_at,
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
        recovery_attempts: row.recovery_attempts,
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
const MAX_PATCH_BYTES: usize = 1_000_000;

fn sensitive_path(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    path.ends_with(".env")
        || path.contains("/.env.")
        || path.contains("secret")
        || path.contains("credential")
        || path.contains("private_key")
        || path.ends_with(".pem")
        || path.ends_with(".key")
}

fn redact_patch(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if (line.starts_with('+') || line.starts_with('-'))
                && [
                    "password",
                    "token",
                    "secret",
                    "authorization",
                    "cookie",
                    "database_url",
                    "private_key",
                ]
                .iter()
                .any(|key| lower.contains(key))
                && (line.contains('=') || line.contains(':'))
            {
                format!("{}[已脱敏的敏感字段]", &line[..1])
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn export_patch(
    pool: &PgPool,
    actor: &ActorContext,
    id: i64,
    controlled_workspace_root: &std::path::Path,
) -> Result<DevRailPatchExportResponse, ApiError> {
    let run = get_run(pool, actor, id).await?;
    let controlled = tokio::fs::canonicalize(controlled_workspace_root)
        .await
        .map_err(ApiError::internal)?;
    let workspace = tokio::fs::canonicalize(&run.cwd)
        .await
        .map_err(|_| ApiError::validation("运行工作区不存在或不可访问"))?;
    if !workspace.starts_with(&controlled) {
        return Err(ApiError::validation("运行工作区不在受控根目录内"));
    }
    let root = workspace
        .to_str()
        .ok_or_else(|| ApiError::validation("运行工作区路径无效"))?;
    let names = tokio::time::timeout(
        Duration::from_secs(15),
        Command::new("git")
            .args(["diff", "--no-ext-diff", "--name-only", "HEAD", "--"])
            .current_dir(root)
            .env_clear()
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| ApiError::conflict("补丁导出超时"))?
    .map_err(ApiError::internal)?;
    if !names.status.success() {
        return Err(ApiError::conflict("工作区不是可导出补丁的 Git 仓库"));
    }
    if String::from_utf8_lossy(&names.stdout)
        .lines()
        .any(sensitive_path)
    {
        return Err(ApiError::conflict("变更包含敏感文件，已拒绝导出补丁"));
    }
    let output = tokio::time::timeout(
        Duration::from_secs(20),
        Command::new("git")
            .args([
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--binary",
                "HEAD",
                "--",
            ])
            .current_dir(root)
            .env_clear()
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| ApiError::conflict("补丁导出超时"))?
    .map_err(ApiError::internal)?;
    if !output.status.success() {
        return Err(ApiError::conflict("无法生成补丁"));
    }
    if output.stdout.len() > MAX_PATCH_BYTES {
        return Err(ApiError::conflict("补丁超过 1MB 限制，无法导出"));
    }
    let content = redact_patch(&String::from_utf8_lossy(&output.stdout));
    let mut tx = pool.begin().await.map_err(db_error)?;
    repositories::audit_logs::record(
        &mut tx,
        Some(actor.user_id),
        "devrail.run.patch_export",
        "devrail_run",
        Some(id),
        json!({"bytes":content.len(),"redacted":content.contains("[已脱敏的敏感字段]")}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(DevRailPatchExportResponse {
        run_id: id,
        file_name: format!("devrail-run-{id}.patch"),
        content,
    })
}

#[cfg(test)]
mod patch_export_tests {
    use super::{redact_patch, sensitive_path};

    #[test]
    fn rejects_sensitive_file_paths() {
        assert!(sensitive_path("config/.env.production"));
        assert!(sensitive_path("keys/deploy.pem"));
        assert!(!sensitive_path("src/main.rs"));
    }

    #[test]
    fn redacts_sensitive_diff_assignments() {
        let patch = "+DATABASE_URL=postgres://secret\n+let healthy = true;";
        assert_eq!(
            redact_patch(patch),
            "+[已脱敏的敏感字段]\n+let healthy = true;"
        );
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
    create_run_with_context(
        pool,
        actor,
        supervisor,
        task_id,
        req,
        RunCreationContext::default(),
    )
    .await
}

pub(crate) async fn create_scheduled_run(
    pool: &PgPool,
    actor: &ActorContext,
    supervisor: &HarnessSupervisor,
    task_id: i64,
    req: &CreateDevRailRunRequest,
    claim_token: uuid::Uuid,
) -> Result<DevRailRunResponse, ApiError> {
    create_run_with_context(
        pool,
        actor,
        supervisor,
        task_id,
        req,
        RunCreationContext {
            scheduler_claim_token: Some(claim_token),
            ..RunCreationContext::default()
        },
    )
    .await
}

async fn create_run_with_context(
    pool: &PgPool,
    actor: &ActorContext,
    supervisor: &HarnessSupervisor,
    task_id: i64,
    req: &CreateDevRailRunRequest,
    context: RunCreationContext,
) -> Result<DevRailRunResponse, ApiError> {
    let RunCreationContext {
        resume,
        scheduler_claim_token,
        parent_run_id,
        parent_turn_id,
    } = context;
    let idempotency_key = key(&req.idempotency_key)?;
    let branch_name = req
        .branch_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(branch) = branch_name {
        if branch.len() > 256
            || branch.contains("..")
            || branch.starts_with('/')
            || branch.ends_with('/')
            || branch.bytes().any(|b| b.is_ascii_whitespace())
        {
            return Err(ApiError::validation("运行分支名称无效"));
        }
    }
    let branch_expires_at = branch_name.map(|_| chrono::Utc::now() + chrono::Duration::hours(24));
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
    if scheduler_claim_token.is_some() && task.status != "queued" {
        return Err(ApiError::validation("任务已不在调度队列中"));
    }
    let environment = devrail::find_environment(pool, actor, task.project_id, req.environment_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApiError::not_found("环境不存在或超出数据范围"))?;
    if !environment.enabled {
        return Err(ApiError::conflict("运行环境已禁用"));
    }
    let cwd = PathBuf::from(&environment.workspace_root);
    let workflow_snapshot = validate_workflow_identity(
        task.dispatch_snapshot.get("workflow"),
        &task.workflow_source,
        &task.workflow_version,
        &task.workflow_digest,
    )?
    .clone();
    let workflow_prompt = workflow_snapshot
        .get("renderedPrompt")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&task.goal);
    let input = bounded_input(req.input.as_deref(), workflow_prompt)?;
    let snapshot = task.dispatch_snapshot.clone();
    let scheduler_policy = supervisor.scheduler_policy();
    let policy = json!({"version":"devrail-policy-v1","networkMode":environment.network_mode,"toolPolicy":environment.tool_policy,"secretRefs":[],"workflowDigest":task.workflow_digest,"workflowConfig":workflow_snapshot.get("config"),"scheduler":{"priorityAgingSeconds":scheduler_policy.priority_aging_seconds},"retry":{"maxAttempts":task.scheduler_max_attempts,"baseDelaySeconds":scheduler_policy.retry_base_seconds,"maxDelaySeconds":scheduler_policy.retry_max_seconds,"jitterPercent":scheduler_policy.retry_jitter_percent,"stallSeconds":scheduler_policy.stall_timeout.as_secs()}});
    let startup_args = json!(["app-server"]);
    let reservation = supervisor
        .reserve()
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let mut tx = pool.begin().await.map_err(db_error)?;
    if let Some(claim_token) = scheduler_claim_token {
        if !devrail::scheduler_claim_is_current(
            &mut tx,
            task.id,
            claim_token,
            scheduler_policy.claim_lease_seconds,
        )
        .await
        .map_err(db_error)?
        {
            return Err(ApiError::conflict("调度租约已失效"));
        }
    }
    let snapshot_id =
        devrail_runs::create_snapshot(&mut tx, actor, task.id, &snapshot, task.department_id)
            .await
            .map_err(db_error)?;
    let attempt = if matches!(actor.actor_type, crate::access::ActorType::System) {
        task.scheduler_attempt.max(1)
    } else {
        devrail_runs::next_attempt(&mut tx, task.id)
            .await
            .map_err(db_error)?
    };
    let row = devrail_runs::create_run(
        &mut tx,
        &devrail_runs::NewRun {
            actor,
            task_id: task.id,
            snapshot_id,
            idempotency_key: &idempotency_key,
            attempt,
            task_revision: task.revision,
            workflow_source: &task.workflow_source,
            workflow_version: &task.workflow_version,
            workflow_digest: &task.workflow_digest,
            workflow_snapshot: &workflow_snapshot,
            actor_type: actor.actor_type.as_str(),
            parent_run_id,
            parent_turn_id: parent_turn_id.as_deref(),
            branch_name,
            branch_expires_at,
            cwd: &environment.workspace_root,
            policy: &policy,
            startup_args: &startup_args,
            model_id: req.model_id.as_deref(),
            department_id: task.department_id,
        },
    )
    .await
    .map_err(db_error)?;
    let Some(row) = row else {
        tx.rollback().await.map_err(db_error)?;
        if let Some(existing) =
            devrail_runs::find_run_by_idempotency(pool, actor, task.id, &idempotency_key)
                .await
                .map_err(db_error)?
        {
            return Ok(run_response(existing));
        }
        return Err(ApiError::conflict("该任务已有相同 attempt 或活动运行"));
    };
    devrail_runs::update_task_status(&mut tx, task.id, "running")
        .await
        .map_err(db_error)?;
    repositories::audit_logs::record_actor(
        &mut tx,
        actor,
        "devrail.run.create",
        "devrail_run",
        Some(row.id),
        json!({"taskId":task.id,"environmentId":environment.id,"actorType":actor.actor_type.as_str(),"attempt":row.attempt,"reason":if scheduler_claim_token.is_some() {"scheduler_dispatch"} else {"user_request"},"policyVersion":"devrail-policy-v1"}),
    )
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    if let Err(error) = supervisor
        .launch_reserved(
            RunLaunch {
                run_id: row.id,
                task_id: task.id,
                organization_id: actor.organization_id,
                department_id: task.department_id,
                owner_user_id: actor.user_id,
                cwd,
                input,
                resume_thread_id: resume.as_ref().map(|value| value.thread_id.clone()),
                resume_turn_id: resume.and_then(|value| value.turn_id),
                attempt: row.attempt,
                max_attempts: task.scheduler_max_attempts,
                automatic: scheduler_claim_token.is_some(),
                scheduler_policy,
            },
            reservation,
        )
        .await
    {
        let capacity = matches!(&error, SupervisorError::Capacity);
        let mut cleanup = pool.begin().await.map_err(db_error)?;
        let trace = uuid::Uuid::new_v4().to_string();
        devrail_runs::update_run_terminal(
            &mut cleanup,
            &devrail_runs::TerminalRunUpdate {
                run_id: row.id,
                status: "failed",
                exit_reason: if capacity {
                    "capacity"
                } else {
                    "launch_failed"
                },
                exit_code: None,
                stderr_summary: None,
                trace_id: &trace,
                recovery_suggestion: Some(if capacity {
                    "Harness 并发额度已用尽；任务将留在队列中稍后重试"
                } else {
                    "Harness 进程未能启动；检查命令配置和受控工作区"
                }),
            },
        )
        .await
        .map_err(db_error)?;
        devrail_runs::update_task_status(
            &mut cleanup,
            task.id,
            if capacity { "queued" } else { "failed" },
        )
        .await
        .map_err(db_error)?;
        cleanup.commit().await.map_err(db_error)?;
        return Err(ApiError::conflict(error.to_string()));
    }
    Ok(run_response(row))
}

fn validate_workflow_identity<'a>(
    snapshot: Option<&'a serde_json::Value>,
    source: &str,
    version: &str,
    digest: &str,
) -> Result<&'a serde_json::Value, ApiError> {
    let snapshot = snapshot.ok_or_else(|| ApiError::conflict("任务派发快照缺少 workflow 身份"))?;
    let snapshot_version = snapshot
        .get("declaredVersion")
        .or_else(|| snapshot.get("version"))
        .and_then(serde_json::Value::as_str);
    if snapshot.get("source").and_then(serde_json::Value::as_str) != Some(source)
        || snapshot_version != Some(version)
        || snapshot.get("digest").and_then(serde_json::Value::as_str) != Some(digest)
    {
        return Err(ApiError::conflict("任务 workflow 快照身份不一致"));
    }
    Ok(snapshot)
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
    let parent_turn_id = req
        .resume_from_turn_id
        .clone()
        .or_else(|| previous.turn_id.clone());
    create_run_with_context(
        pool,
        actor,
        supervisor,
        previous.task_id,
        &CreateDevRailRunRequest {
            environment_id,
            idempotency_key: req.idempotency_key.clone(),
            model_id: previous.model_id,
            input: req.input.clone(),
            branch_name: previous.branch_name,
        },
        RunCreationContext {
            resume,
            scheduler_claim_token: None,
            parent_run_id: Some(previous.id),
            parent_turn_id,
        },
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

    #[test]
    fn workflow_identity_requires_source_version_and_digest_to_match() {
        let digest = "a".repeat(64);
        let snapshot = serde_json::json!({
            "source": "repository",
            "declaredVersion": "v1",
            "digest": digest,
        });
        assert!(
            validate_workflow_identity(Some(&snapshot), "repository", "v1", &"a".repeat(64),)
                .is_ok()
        );
        assert!(
            validate_workflow_identity(Some(&snapshot), "default", "v1", &"a".repeat(64),).is_err()
        );
        assert!(
            validate_workflow_identity(Some(&snapshot), "repository", "v2", &"a".repeat(64),)
                .is_err()
        );
        assert!(
            validate_workflow_identity(Some(&snapshot), "repository", "v1", &"b".repeat(64),)
                .is_err()
        );
    }

    #[tokio::test]
    async fn queued_workflow_snapshot_reaches_harness_once_without_drift() {
        let _guard = crate::db::DATABASE_TEST_LOCK.lock().await;
        let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
            return;
        };
        let pool = crate::db::init_pool(&database_url)
            .await
            .expect("connect test database");
        crate::db::run_migrations(&pool)
            .await
            .expect("run migrations");
        let controlled_root =
            std::env::temp_dir().join(format!("devrail-workflow-e2e-{}", uuid::Uuid::new_v4()));
        let workspace = controlled_root.join("repository");
        tokio::fs::create_dir_all(&workspace)
            .await
            .expect("create controlled workspace");
        tokio::fs::write(
            workspace.join("WORKFLOW.md"),
            include_str!("../../../WORKFLOW.md"),
        )
        .await
        .expect("write workflow contract");
        tokio::fs::write(
            workspace.join("app-server"),
            b"IFS= read -r initialize\nprintf '%s\\n' '{\"id\":\"initialize\",\"result\":{}}'\nIFS= read -r thread\nIFS= read -r turn\nprintf '%s\\n' \"$turn\" > captured-turn.json\nsleep 30\n",
        )
        .await
        .expect("write fake app-server");
        let fixture = crate::repositories::devrail_runs::create_workflow_e2e_fixture(
            &pool,
            workspace.to_string_lossy().as_ref(),
        )
        .await
        .expect("create workflow fixture");
        let queued = crate::services::devrail::update_task(
            &pool,
            &fixture.actor,
            fixture.project_id,
            fixture.task_id,
            &crate::models::UpdateDevRailTaskRequest {
                title: None,
                goal: None,
                background: crate::models::NullablePatch::Missing,
                acceptance_criteria: crate::models::NullablePatch::Missing,
                constraints: crate::models::NullablePatch::Missing,
                priority: None,
                status: Some("queued".to_string()),
                assignee_user_id: crate::models::NullablePatch::Missing,
                labels: None,
                due_at: crate::models::NullablePatch::Missing,
                repository_id: crate::models::NullablePatch::Missing,
                environment_id: crate::models::NullablePatch::Missing,
            },
            &controlled_root,
        )
        .await
        .expect("queue task with workflow snapshot");
        assert_eq!(queued.workflow_source, "repository");
        let claim_token = uuid::Uuid::new_v4();
        let claimed =
            crate::repositories::devrail::claim_scheduler_tasks(&pool, claim_token, 100, 60, 3_600)
                .await
                .expect("claim queued task");
        assert!(claimed.iter().any(|task| task.id == fixture.task_id));
        let supervisor = HarnessSupervisor::new(
            pool.clone(),
            "bash".to_string(),
            1,
            30,
            controlled_root.to_string_lossy().into_owned(),
            1,
            crate::workers::task_scheduler::SchedulerPolicy::default(),
        );
        let request = CreateDevRailRunRequest {
            environment_id: fixture.environment_id,
            idempotency_key: format!("scheduler:{}:1", fixture.task_id),
            model_id: None,
            input: None,
            branch_name: None,
        };
        let run = create_scheduled_run(
            &pool,
            &fixture.actor,
            &supervisor,
            fixture.task_id,
            &request,
            claim_token,
        )
        .await
        .expect("create scheduled run");
        assert_eq!(run.workflow_digest, queued.workflow_digest);
        assert_eq!(run.workflow_source, queued.workflow_source);
        assert_eq!(run.workflow_version, queued.workflow_version);
        assert_eq!(run.task_revision, queued.revision);

        let captured_path = workspace.join("captured-turn.json");
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let captured = loop {
            if let Ok(value) = tokio::fs::read_to_string(&captured_path).await {
                break value;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "fake app-server did not capture turn input"
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        };
        let turn: serde_json::Value =
            serde_json::from_str(captured.trim()).expect("parse captured turn");
        let input = turn
            .pointer("/params/input")
            .and_then(serde_json::Value::as_str)
            .expect("turn input");
        assert!(input.contains("工作流端到端任务"));
        assert!(input.contains("执行不可变工作流快照"));
        assert!(input.contains("输入必须来自已渲染工作流"));

        let duplicate = create_scheduled_run(
            &pool,
            &fixture.actor,
            &supervisor,
            fixture.task_id,
            &request,
            claim_token,
        )
        .await
        .expect("same idempotency key returns existing run");
        assert_eq!(duplicate.id, run.id);
        assert_eq!(
            crate::repositories::devrail_runs::count_task_runs_for_test(&pool, fixture.task_id,)
                .await
                .expect("count task runs"),
            1
        );
        supervisor
            .interrupt(run.id)
            .await
            .expect("stop fake app-server");
        tokio::fs::remove_dir_all(&controlled_root)
            .await
            .expect("cleanup controlled workspace");
    }
}
