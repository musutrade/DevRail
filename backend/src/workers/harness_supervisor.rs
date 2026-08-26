//! The only component allowed to start Codex.  The browser talks to the API;
//! this worker owns the controlled app-server process and its JSONL streams.

use crate::access::{ActorContext, ActorType, DataScope};
use crate::models::CreateDevRailFollowupTaskRequest;
use crate::repositories::devrail_runs;
use crate::services;
use crate::workers::task_scheduler::SchedulerPolicy;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{
    collections::{BTreeSet, HashMap},
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{mpsc, Mutex, Semaphore},
};

#[derive(Debug, Clone)]
pub struct RunLaunch {
    pub run_id: i64,
    pub task_id: i64,
    pub organization_id: i64,
    pub department_id: Option<i64>,
    pub owner_user_id: i64,
    pub cwd: PathBuf,
    pub input: String,
    pub resume_thread_id: Option<String>,
    pub resume_turn_id: Option<String>,
    pub attempt: i32,
    pub max_attempts: i32,
    pub automatic: bool,
    pub scheduler_policy: SchedulerPolicy,
}

struct ProcessContext {
    supervisor: HarnessSupervisor,
    launch: RunLaunch,
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    stderr: tokio::process::ChildStderr,
    controls: mpsc::Receiver<ControlMessage>,
    _slot: tokio::sync::OwnedSemaphorePermit,
}

pub(crate) struct RunReservation(tokio::sync::OwnedSemaphorePermit);

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("Harness 并发额度已用尽")]
    Capacity,
    #[error("无法启动 Harness 进程: {0}")]
    Spawn(String),
    #[error("运行工作区不在受控根目录下")]
    Workspace,
    #[error("运行控制通道不可用")]
    ControlUnavailable,
}

#[derive(Debug)]
enum ControlMessage {
    Interrupt(InterruptCause),
    Approval { approval_id: i64, approved: bool },
}

#[derive(Debug, Clone, Copy)]
enum InterruptCause {
    User,
    TaskCancelled,
    EnvironmentInvalid,
}

impl InterruptCause {
    const fn terminal(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::User => ("cancelled", "interrupted", "运行已由用户中断"),
            Self::TaskCancelled => (
                "cancelled",
                "task_cancelled",
                "任务已取消；调度器已清理 Harness 进程",
            ),
            Self::EnvironmentInvalid => (
                "failed",
                "environment_invalid",
                "运行环境在启动阶段失效；请修复环境后重试",
            ),
        }
    }
}

#[derive(Clone)]
pub struct HarnessSupervisor {
    pool: PgPool,
    command: Arc<String>,
    max_duration: Duration,
    graceful_interrupt: Duration,
    workspace_root: Arc<PathBuf>,
    slots: Arc<Semaphore>,
    controls: Arc<Mutex<HashMap<i64, mpsc::Sender<ControlMessage>>>>,
    scheduler_policy: SchedulerPolicy,
}

impl HarnessSupervisor {
    pub fn new(
        pool: PgPool,
        command: String,
        max_concurrency: usize,
        max_duration_secs: i64,
        workspace_root: String,
        graceful_interrupt_secs: i64,
        scheduler_policy: SchedulerPolicy,
    ) -> Self {
        Self {
            pool,
            command: Arc::new(command),
            max_duration: Duration::from_secs(max_duration_secs as u64),
            graceful_interrupt: Duration::from_secs(graceful_interrupt_secs as u64),
            workspace_root: Arc::new(PathBuf::from(workspace_root)),
            slots: Arc::new(Semaphore::new(max_concurrency)),
            controls: Arc::new(Mutex::new(HashMap::new())),
            scheduler_policy,
        }
    }

    pub fn launch(
        &self,
        launch: RunLaunch,
    ) -> Pin<Box<dyn Future<Output = Result<(), SupervisorError>> + Send + '_>> {
        Box::pin(async move {
            let reservation = self.reserve()?;
            self.launch_reserved(launch, reservation).await
        })
    }

    pub(crate) fn reserve(&self) -> Result<RunReservation, SupervisorError> {
        self.slots
            .clone()
            .try_acquire_owned()
            .map(RunReservation)
            .map_err(|_| SupervisorError::Capacity)
    }

    pub(crate) fn launch_reserved(
        &self,
        launch: RunLaunch,
        reservation: RunReservation,
    ) -> Pin<Box<dyn Future<Output = Result<(), SupervisorError>> + Send + '_>> {
        Box::pin(async move {
            if !launch.cwd.starts_with(self.workspace_root.as_ref()) {
                return Err(SupervisorError::Workspace);
            }
            let (tx, rx) = mpsc::channel(2);
            self.controls.lock().await.insert(launch.run_id, tx);

            let mut command = Command::new(self.command.as_str());
            command
                .arg("app-server")
                .current_dir(&launch.cwd)
                .env_clear()
                .env("DEVRAIL_RUN_ID", launch.run_id.to_string())
                .env("DEVRAIL_TASK_ID", launch.task_id.to_string())
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true);
            // PATH and HOME are the only inherited values; credentials and the
            // server's connection environment are deliberately not propagated.
            if let Ok(path) = std::env::var("PATH") {
                command.env("PATH", path);
            }
            if let Ok(home) = std::env::var("HOME") {
                command.env("HOME", home);
            }
            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(error) => {
                    self.controls.lock().await.remove(&launch.run_id);
                    return Err(SupervisorError::Spawn(error.to_string()));
                }
            };
            let stdin = child
                .stdin
                .take()
                .ok_or_else(|| SupervisorError::Spawn("stdin 不可用".into()))?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| SupervisorError::Spawn("stdout 不可用".into()))?;
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| SupervisorError::Spawn("stderr 不可用".into()))?;
            let supervisor = self.clone();
            tokio::spawn(async move {
                run_process(ProcessContext {
                    supervisor,
                    launch,
                    child,
                    stdin,
                    stdout,
                    stderr,
                    controls: rx,
                    _slot: reservation.0,
                })
                .await;
            });
            Ok(())
        })
    }

    pub async fn interrupt(&self, run_id: i64) -> Result<(), SupervisorError> {
        self.send_interrupt(run_id, InterruptCause::User).await
    }

    pub(crate) async fn interrupt_for_reconciliation(
        &self,
        run_id: i64,
        reason: &str,
    ) -> Result<(), SupervisorError> {
        let cause = match reason {
            "task_cancelled" => InterruptCause::TaskCancelled,
            _ => InterruptCause::EnvironmentInvalid,
        };
        self.send_interrupt(run_id, cause).await
    }

    async fn send_interrupt(
        &self,
        run_id: i64,
        cause: InterruptCause,
    ) -> Result<(), SupervisorError> {
        let sender = self
            .controls
            .lock()
            .await
            .get(&run_id)
            .cloned()
            .ok_or(SupervisorError::ControlUnavailable)?;
        sender
            .send(ControlMessage::Interrupt(cause))
            .await
            .map_err(|_| SupervisorError::ControlUnavailable)
    }

    pub async fn running_run_ids(&self) -> Vec<i64> {
        let mut ids = self
            .controls
            .lock()
            .await
            .keys()
            .copied()
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    pub const fn scheduler_policy(&self) -> SchedulerPolicy {
        self.scheduler_policy
    }

    pub async fn resolve_approval(
        &self,
        run_id: i64,
        approval_id: i64,
        approved: bool,
    ) -> Result<(), SupervisorError> {
        let sender = self
            .controls
            .lock()
            .await
            .get(&run_id)
            .cloned()
            .ok_or(SupervisorError::ControlUnavailable)?;
        sender
            .send(ControlMessage::Approval {
                approval_id,
                approved,
            })
            .await
            .map_err(|_| SupervisorError::ControlUnavailable)
    }

    pub async fn recover_stale_runs(&self) -> Result<u64, sqlx::Error> {
        let runs = devrail_runs::list_recoverable_runs(&self.pool).await?;
        let mut recovered = 0;
        for run in runs {
            let snapshot = sqlx::query_scalar::<_, Value>(
                "SELECT snapshot FROM devrail_task_snapshots WHERE id=$1",
            )
            .bind(run.snapshot_id)
            .fetch_optional(&self.pool)
            .await?;
            let input = snapshot
                .as_ref()
                .and_then(|value| value.get("goal"))
                .and_then(Value::as_str)
                .unwrap_or("继续执行原任务")
                .to_string();
            let launch = RunLaunch {
                run_id: run.id,
                task_id: run.task_id,
                organization_id: run.organization_id,
                department_id: run.department_id,
                owner_user_id: run.owner_user_id,
                cwd: PathBuf::from(run.cwd),
                input,
                resume_thread_id: run.thread_id,
                resume_turn_id: run.turn_id,
                attempt: run.attempt,
                max_attempts: devrail_runs::scheduler_retry_policy(&self.pool, run.task_id)
                    .await?
                    .1,
                automatic: run.actor_type == "system",
                scheduler_policy: self.scheduler_policy,
            };
            if self.launch(launch).await.is_ok() {
                recovered += 1;
            }
        }
        let _ = devrail_runs::mark_unrecoverable_runs(&self.pool).await?;
        Ok(recovered)
    }

    pub async fn recover_run(&self, run_id: i64) -> Result<(), SupervisorError> {
        let run = devrail_runs::find_for_recovery(&self.pool, run_id)
            .await
            .map_err(|error| SupervisorError::Spawn(error.to_string()))?
            .ok_or(SupervisorError::ControlUnavailable)?;
        if run.status != "awaiting_approval" || run.thread_id.is_none() {
            return Err(SupervisorError::ControlUnavailable);
        }
        let snapshot = sqlx::query_scalar::<_, Value>(
            "SELECT snapshot FROM devrail_task_snapshots WHERE id=$1",
        )
        .bind(run.snapshot_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| SupervisorError::Spawn(error.to_string()))?;
        let input = snapshot
            .as_ref()
            .and_then(|value| value.get("goal"))
            .and_then(Value::as_str)
            .unwrap_or("继续执行原任务")
            .to_string();
        self.launch(RunLaunch {
            run_id: run.id,
            task_id: run.task_id,
            organization_id: run.organization_id,
            department_id: run.department_id,
            owner_user_id: run.owner_user_id,
            cwd: PathBuf::from(run.cwd),
            input,
            resume_thread_id: run.thread_id,
            resume_turn_id: run.turn_id,
            attempt: run.attempt,
            max_attempts: devrail_runs::scheduler_retry_policy(&self.pool, run.task_id)
                .await
                .map_err(|error| SupervisorError::Spawn(error.to_string()))?
                .1,
            automatic: run.actor_type == "system",
            scheduler_policy: self.scheduler_policy,
        })
        .await
    }

    async fn prepare_transport_recovery(
        &self,
        launch: &RunLaunch,
        reason: &str,
    ) -> Option<RunLaunch> {
        let Ok(true) =
            devrail_runs::prepare_transport_recovery(&self.pool, launch.run_id, reason).await
        else {
            let _ = finish_run(
                &self.pool,
                launch,
                "failed",
                reason,
                None,
                None,
                Some("Harness 多次连接中断；自动恢复已达到上限，请人工重试"),
            )
            .await;
            return None;
        };
        let _ = persist_event(
            &self.pool,
            launch,
            "run_recovery",
            None,
            json!({"reason": reason, "automatic": true}),
            Some("Harness 连接中断，正在自动恢复"),
        )
        .await;
        let run = match devrail_runs::find_for_recovery(&self.pool, launch.run_id).await {
            Ok(Some(run)) => run,
            _ => {
                let _ = finish_run(
                    &self.pool,
                    launch,
                    "failed",
                    "recovery_state_missing",
                    None,
                    None,
                    Some("Harness 断流后无法读取恢复状态；任务将按策略重试"),
                )
                .await;
                return None;
            }
        };
        let Some(thread_id) = run.thread_id else {
            let _ = finish_run(
                &self.pool,
                launch,
                "failed",
                "transport_resume_unavailable",
                None,
                None,
                Some("Harness 断流前尚未持久化 thread；为避免重复 Agent，将进入下一 attempt"),
            )
            .await;
            return None;
        };
        let mut recovery = launch.clone();
        recovery.resume_thread_id = Some(thread_id);
        recovery.resume_turn_id = run.turn_id;
        Some(recovery)
    }
}

async fn run_process(context: ProcessContext) {
    let ProcessContext {
        supervisor,
        launch,
        mut child,
        mut stdin,
        stdout,
        stderr,
        mut controls,
        _slot: slot,
    } = context;
    let pool = supervisor.pool.clone();
    let mut out_reader = BufReader::new(stdout);
    let mut err_reader = BufReader::new(stderr);
    let _ = write_json(&mut stdin, json!({"id":"initialize","method":"initialize","params":{"clientName":"devrail","clientVersion":"1"}})).await;
    let mut handshake_line = String::new();
    let handshake_ok = tokio::time::timeout(
        Duration::from_secs(10),
        out_reader.read_line(&mut handshake_line),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .and_then(|count| {
        (count > 0)
            .then(|| serde_json::from_str::<Value>(handshake_line.trim()).ok())
            .flatten()
    })
    .is_some_and(|value| {
        value.get("id").and_then(Value::as_str) == Some("initialize")
            || value.get("method").and_then(Value::as_str) == Some("initialized")
            || value.get("result").is_some()
    });
    if !handshake_ok {
        let _ = finish_run(
            &pool,
            &launch,
            "failed",
            "initialization_failed",
            None,
            None,
            Some("Harness 初始化握手失败；检查 app-server 版本和启动参数"),
        )
        .await;
        let _ = child.start_kill();
        supervisor.controls.lock().await.remove(&launch.run_id);
        return;
    }
    let thread_method = if launch.resume_thread_id.is_some() {
        "thread/resume"
    } else {
        "thread/start"
    };
    let thread_params = if let Some(thread_id) = launch.resume_thread_id.as_deref() {
        json!({"threadId": thread_id, "cwd": launch.cwd})
    } else {
        json!({"cwd": launch.cwd})
    };
    let _ = write_json(
        &mut stdin,
        json!({"id":"thread-start","method":thread_method,"params":thread_params}),
    )
    .await;
    let _ = write_json(
        &mut stdin,
        json!({"id":"turn-start","method":"turn/start","params":{"input":launch.input,"threadId":launch.resume_thread_id,"resumeFromTurnId":launch.resume_turn_id}}),
    )
    .await;
    let _ = persist_started(&pool, &launch).await;
    let _ = persist_event(
        &pool,
        &launch,
        "run_started",
        None,
        json!({"cwd": launch.cwd}),
        Some(if launch.resume_thread_id.is_some() {
            "Harness 已恢复原线程"
        } else {
            "Harness 已启动"
        }),
    )
    .await;
    let mut out_line = String::new();
    let mut err_line = String::new();
    let mut stderr_summary = String::new();
    let mut protocol_failed = false;
    let mut timeout_sleep = Box::pin(tokio::time::sleep(supervisor.max_duration));
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    let mut stall_sleep = Box::pin(tokio::time::sleep(
        supervisor.scheduler_policy.stall_timeout,
    ));
    let mut transport_recovery = None;

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                let _ = devrail_runs::update_run_heartbeat(&pool, launch.run_id).await;
            }
            _ = &mut stall_sleep => {
                let _ = child.start_kill();
                let code = child.wait().await.ok().and_then(|status| status.code());
                let _ = finish_run(
                    &pool,
                    &launch,
                    "failed",
                    "stall",
                    code,
                    Some(&stderr_summary),
                    Some("运行在 stall 阈值内没有心跳或事件；已清理 Harness 进程，请检查日志后重试"),
                )
                .await;
                crate::app_metrics::record_scheduler_stall();
                break;
            }
            command = controls.recv() => {
                if let Some(ControlMessage::Interrupt(cause)) = command {
                    if let Err(error) = write_json(&mut stdin, json!({"method":"turn/interrupt","params":{}})).await {
                        append_summary(&mut stderr_summary, &format!("transport write error: {error}"));
                    }
                    let status = tokio::time::timeout(supervisor.graceful_interrupt, child.wait()).await;
                    let exit_code = match status { Ok(Ok(s)) => s.code(), _ => { let _ = child.start_kill(); child.wait().await.ok().and_then(|s| s.code()) } };
                    let (terminal_status, reason, recovery) = cause.terminal();
                    let _ = finish_run(&pool, &launch, terminal_status, reason, exit_code, Some(&stderr_summary), Some(recovery)).await;
                    break;
                }
                if let Some(ControlMessage::Approval { approval_id, approved }) = command {
                    let _ = write_json(&mut stdin, json!({"method":"approval/resolve","params":{"approvalId":approval_id,"approved":approved}})).await;
                }
            }
            result = out_reader.read_line(&mut out_line) => {
                match result {
                    Ok(0) => {
                        protocol_failed = true;
                        append_summary(&mut stderr_summary, "transport read error: EOF");
                        let _ = child.start_kill();
                    },
                    Ok(_) => {
                        stall_sleep
                            .as_mut()
                            .reset(tokio::time::Instant::now() + supervisor.scheduler_policy.stall_timeout);
                        let line = out_line.trim();
                        if !line.is_empty() && !handle_stdout(&pool, &launch, line).await { protocol_failed = true; let _ = child.start_kill(); }
                        out_line.clear();
                    }
                    Err(error) => {
                        protocol_failed = true;
                        append_summary(&mut stderr_summary, &format!("transport read error: {error}"));
                        let _ = child.start_kill();
                    }
                }
            }
            result = err_reader.read_line(&mut err_line) => {
                match result {
                    Ok(0) => {},
                    Ok(_) => {
                        stall_sleep
                            .as_mut()
                            .reset(tokio::time::Instant::now() + supervisor.scheduler_policy.stall_timeout);
                        append_summary(&mut stderr_summary, err_line.trim());
                        err_line.clear();
                    }
                    Err(error) => {
                        append_summary(&mut stderr_summary, &format!("transport stderr read error: {error}"));
                    }
                }
            }
            _ = &mut timeout_sleep => {
                let _ = child.start_kill();
                let code = child.wait().await.ok().and_then(|s| s.code());
                let _ = finish_run(&pool, &launch, "failed", "timeout", code, Some(&stderr_summary), Some("运行超时；请检查任务范围或增加环境时限")).await;
                break;
            }
            result = child.wait() => {
                let code = result.ok().and_then(|s| s.code());
                let reason = classify_failure(protocol_failed, &stderr_summary);
                if protocol_failed && matches!(reason, "transport_disconnect" | "transport_read_error" | "transport_write_error") {
                    transport_recovery = supervisor.prepare_transport_recovery(&launch, reason).await;
                    break;
                }
                let (status, reason, recovery) = if protocol_failed || code != Some(0) {
                    ("failed", reason, Some(recovery_for_failure(protocol_failed, &stderr_summary)))
                } else { ("completed", "completed", None) };
                let _ = finish_run(&pool, &launch, status, reason, code, Some(&stderr_summary), recovery).await;
                break;
            }
        }
    }
    supervisor.controls.lock().await.remove(&launch.run_id);
    drop(slot);
    if let Some(recovery) = transport_recovery {
        if supervisor.launch(recovery).await.is_err() {
            let _ = finish_run(
                &pool,
                &launch,
                "failed",
                "recovery_spawn_failed",
                None,
                None,
                Some("Harness 自动恢复启动失败；任务将按策略重试"),
            )
            .await;
        }
    }
}

fn classify_failure(protocol_failed: bool, stderr: &str) -> &'static str {
    let value = stderr.to_ascii_lowercase();
    if value.contains("broken pipe")
        || value.contains("connection reset")
        || value.contains("connection closed")
    {
        "transport_disconnect"
    } else if value.contains("read") || value.contains("eof") {
        "transport_read_error"
    } else if value.contains("write") || value.contains("flush") {
        "transport_write_error"
    } else if protocol_failed {
        "protocol_error"
    } else {
        "process_exit"
    }
}

fn recovery_for_failure(protocol_failed: bool, stderr: &str) -> &'static str {
    match classify_failure(protocol_failed, stderr) {
        "transport_disconnect" | "transport_read_error" | "transport_write_error" => {
            "Harness 连接中断；保留已持久化事件后从最近回合恢复或重试"
        }
        "protocol_error" => "Harness 协议异常；请检查事件摘要后重试",
        _ => "检查 Harness stderr 摘要并重试",
    }
}

fn should_retry_automatically(launch: &RunLaunch, status: &str, reason: &str) -> bool {
    launch.automatic
        && status == "failed"
        && launch.attempt < launch.max_attempts
        && matches!(
            reason,
            "stall"
                | "timeout"
                | "process_exit"
                | "transport_disconnect"
                | "transport_read_error"
                | "transport_write_error"
                | "transport_resume_unavailable"
                | "recovery_state_missing"
                | "recovery_spawn_failed"
        )
}

async fn write_json(stdin: &mut tokio::process::ChildStdin, value: Value) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec());
    line.push(b'\n');
    stdin.write_all(&line).await
}

async fn handle_stdout(pool: &PgPool, launch: &RunLaunch, line: &str) -> bool {
    let value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(_) => {
            let _ = persist_event(
                pool,
                launch,
                "error",
                None,
                json!({"message":"Harness 返回了无效 JSONL"}),
                Some("Harness 协议解析失败"),
            )
            .await;
            return false;
        }
    };
    let (event_type, source_id, summary, payload) = classify_event(&value);
    let thread_id = value
        .get("thread_id")
        .or_else(|| value.pointer("/result/thread/id"))
        .and_then(Value::as_str);
    let turn_id = value
        .get("turn_id")
        .or_else(|| value.pointer("/result/turn/id"))
        .and_then(Value::as_str);
    let harness_version = value
        .get("harness_version")
        .or_else(|| value.get("serverVersion"))
        .and_then(Value::as_str);
    if thread_id.is_some() || turn_id.is_some() || harness_version.is_some() {
        if let Ok(mut tx) = pool.begin().await {
            let _ = devrail_runs::update_run_started(
                &mut tx,
                launch.run_id,
                thread_id,
                turn_id,
                harness_version,
            )
            .await;
            let _ = tx.commit().await;
        }
    }
    if followup_proposal(&value).is_some() {
        handle_followup_proposal(pool, launch, &value).await;
        return true;
    }
    let _ = persist_event(
        pool,
        launch,
        &event_type,
        source_id.as_deref(),
        payload,
        summary.as_deref(),
    )
    .await;
    if event_type == "approval_request" {
        let tool_name = value
            .get("tool")
            .or_else(|| value.get("tool_name"))
            .and_then(Value::as_str)
            .unwrap_or("unknown-tool");
        let risk_level = value
            .get("risk_level")
            .and_then(Value::as_str)
            .filter(|risk| matches!(*risk, "low" | "medium" | "high" | "critical"))
            .unwrap_or("high");
        let approval_key = source_id.as_deref().unwrap_or("approval:unknown");
        let cwd = launch.cwd.to_string_lossy().to_string();
        let _ = crate::services::devrail_approvals::request_from_harness(
            pool,
            crate::services::devrail_approvals::HarnessApprovalRequest {
                run_id: launch.run_id,
                organization_id: launch.organization_id,
                department_id: launch.department_id,
                owner_user_id: launch.owner_user_id,
                tool_name: tool_name.to_string(),
                args_summary: sanitize(value.get("args").unwrap_or(&json!({}))),
                cwd,
                risk_level: risk_level.to_string(),
                idempotency_key: approval_key.to_string(),
            },
        )
        .await;
    }
    true
}

fn followup_proposal(value: &Value) -> Option<Result<CreateDevRailFollowupTaskRequest, ()>> {
    let method = value
        .get("method")
        .or_else(|| value.get("type"))
        .or_else(|| value.get("name"))
        .and_then(Value::as_str);
    let is_followup = method.is_some_and(|name| {
        matches!(
            name,
            "devrail/followup.create" | "devrail.followup.create" | "devrail_followup_create"
        )
    }) || value
        .get("tool")
        .and_then(Value::as_str)
        .is_some_and(|name| name == "devrail/followup.create")
        || value
            .pointer("/item/name")
            .and_then(Value::as_str)
            .is_some_and(|name| name == "devrail_followup_create");
    if !is_followup {
        return None;
    }
    let params = value
        .get("params")
        .or_else(|| value.get("arguments"))
        .or_else(|| value.pointer("/item/arguments"))
        .or_else(|| value.pointer("/item/params"));
    Some(params.cloned().ok_or(()).and_then(|params| {
        if let Value::String(encoded) = params {
            serde_json::from_str(&encoded).map_err(|_| ())
        } else {
            serde_json::from_value(params).map_err(|_| ())
        }
    }))
}

fn followup_actor(launch: &RunLaunch) -> ActorContext {
    ActorContext {
        actor_type: ActorType::System,
        user_id: launch.owner_user_id,
        session_id: 0,
        organization_id: launch.organization_id,
        department_id: launch.department_id,
        data_scope: DataScope::Organization,
        permission_codes: BTreeSet::from([
            "devrail:task:read".to_string(),
            "devrail:followup:create".to_string(),
        ]),
    }
}

fn followup_event_source(value: &Value) -> Option<String> {
    value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 256
                && !id
                    .bytes()
                    .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        })
        .map(str::to_string)
}

async fn handle_followup_proposal(pool: &PgPool, launch: &RunLaunch, value: &Value) {
    let actor = followup_actor(launch);
    let source_event_id = followup_event_source(value);
    match followup_proposal(value) {
        Some(Ok(request)) => {
            match services::devrail::create_followup_task(pool, &actor, launch.run_id, &request)
                .await
            {
                Ok(response) => {
                    crate::app_metrics::record_agent_followup(if response.replayed {
                        "replayed"
                    } else {
                        "accepted"
                    });
                    let _ = persist_event(
                        pool,
                        launch,
                        "followup_accepted",
                        source_event_id.as_deref(),
                        json!({
                            "requestId": response.request_id,
                            "resultTaskId": response.task.id,
                            "replayed": response.replayed,
                        }),
                        Some("受控 Agent 后续任务提议已处理"),
                    )
                    .await;
                }
                Err(error) => {
                    let reason = followup_rejection_reason(&error);
                    crate::app_metrics::record_agent_followup(reason.metric_outcome());
                    let _ = services::devrail::record_followup_rejection(
                        pool,
                        &actor,
                        launch.run_id,
                        reason.audit_code(),
                    )
                    .await;
                    let _ = persist_event(
                        pool,
                        launch,
                        "followup_rejected",
                        source_event_id.as_deref(),
                        json!({"reason": reason.audit_code()}),
                        Some("受控 Agent 后续任务提议被拒绝"),
                    )
                    .await;
                }
            }
        }
        Some(Err(())) => {
            crate::app_metrics::record_agent_followup("rejected_schema");
            let _ =
                services::devrail::record_followup_rejection(pool, &actor, launch.run_id, "schema")
                    .await;
            let _ = persist_event(
                pool,
                launch,
                "followup_rejected",
                source_event_id.as_deref(),
                json!({"reason":"schema"}),
                Some("受控 Agent 后续任务提议字段无效"),
            )
            .await;
        }
        None => {}
    }
}

#[derive(Clone, Copy)]
enum FollowupRejection {
    Policy,
    Unavailable,
}

impl FollowupRejection {
    const fn audit_code(self) -> &'static str {
        match self {
            Self::Policy => "policy",
            Self::Unavailable => "unavailable",
        }
    }

    const fn metric_outcome(self) -> &'static str {
        match self {
            Self::Policy => "rejected_policy",
            Self::Unavailable => "unavailable",
        }
    }
}

fn followup_rejection_reason(error: &crate::error::ApiError) -> FollowupRejection {
    match error {
        crate::error::ApiError::Internal(_) => FollowupRejection::Unavailable,
        _ => FollowupRejection::Policy,
    }
}

fn classify_event(value: &Value) -> (String, Option<String>, Option<String>, Value) {
    let kind = value
        .get("type")
        .or_else(|| value.get("method"))
        .and_then(Value::as_str)
        .unwrap_or("lifecycle");
    let event_type = match kind {
        k if k.contains("approval") => "approval_request",
        k if k.contains("command") && (k.contains("start") || k.contains("begin")) => {
            "command_start"
        }
        k if k.contains("command") => "command_end",
        k if k.contains("file") || k.contains("change") || k.contains("patch") => "file_change",
        k if k.contains("quality") || k.contains("gate") => "quality_gate",
        k if k.contains("tool") => "tool_call",
        k if k.contains("reasoning") => "reasoning_summary",
        k if k.contains("error") || value.get("error").is_some() => "error",
        k if k.contains("turn") && (k.contains("complete") || k.contains("done")) => {
            "turn_complete"
        }
        k if k.contains("message") || k.contains("agent") => "agent_message",
        _ => "lifecycle",
    }
    .to_string();
    let source_id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            value
                .get("event_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let summary = value
        .get("summary")
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
        .map(|s| clip_text(s, 2048));
    let payload = if event_type == "reasoning_summary" {
        json!({"summary": summary.clone().unwrap_or_default()})
    } else {
        sanitize(value)
    };
    (event_type, source_id, summary, payload)
}

fn sanitize(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter_map(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    if [
                        "password",
                        "token",
                        "secret",
                        "cookie",
                        "authorization",
                        "credential",
                        "private_key",
                        "database_url",
                    ]
                    .iter()
                    .any(|needle| lower.contains(needle))
                    {
                        return None;
                    }
                    Some((key.clone(), sanitize(value)))
                })
                .collect::<Map<_, _>>(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(sanitize).collect()),
        Value::String(text) => Value::String(clip_text(text, 4096)),
        other => other.clone(),
    }
}

fn clip_text(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}
fn append_summary(summary: &mut String, line: &str) {
    if !summary.is_empty() {
        summary.push('\n');
    }
    summary.push_str(&clip_text(line, 1024));
    if summary.len() > 4096 {
        *summary = summary.chars().take(4096).collect();
    }
}

async fn persist_started(pool: &PgPool, launch: &RunLaunch) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    devrail_runs::update_run_started(&mut tx, launch.run_id, None, None, None).await?;
    tx.commit().await
}

async fn persist_event(
    pool: &PgPool,
    launch: &RunLaunch,
    event_type: &str,
    source_id: Option<&str>,
    payload: Value,
    summary: Option<&str>,
) -> Result<(), sqlx::Error> {
    let idempotency = source_id.map(str::to_string).unwrap_or_else(|| {
        let mut hasher = Sha256::new();
        hasher.update(serde_json::to_vec(&payload).unwrap_or_default());
        hasher.update(event_type.as_bytes());
        let digest = hasher.finalize();
        format!(
            "sha256:{}",
            digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
    });
    let mut tx = pool.begin().await?;
    devrail_runs::append_event(
        &mut tx,
        &devrail_runs::NewRunEvent {
            run_id: launch.run_id,
            organization_id: launch.organization_id,
            department_id: launch.department_id,
            owner_user_id: launch.owner_user_id,
            event_type,
            source_event_id: source_id,
            idempotency_key: &idempotency,
            payload: &payload,
            summary,
        },
    )
    .await?;
    tx.commit().await
}

async fn finish_run(
    pool: &PgPool,
    launch: &RunLaunch,
    status: &str,
    reason: &str,
    code: Option<i32>,
    stderr: Option<&str>,
    recovery: Option<&str>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let quality_gate_failed = status == "completed"
        && devrail_runs::has_failed_quality_gate(&mut tx, launch.run_id).await?;
    let (status, reason, recovery) = if quality_gate_failed {
        (
            "failed",
            "quality_gate_failed",
            Some("质量门禁未通过；请查看门禁结果后重试"),
        )
    } else {
        (status, reason, recovery)
    };
    let trace = uuid::Uuid::new_v4().to_string();
    let retryable = should_retry_automatically(launch, status, reason);
    let transitioned = devrail_runs::update_run_terminal(
        &mut tx,
        &devrail_runs::TerminalRunUpdate {
            run_id: launch.run_id,
            status,
            exit_reason: reason,
            exit_code: code,
            stderr_summary: stderr.filter(|s| !s.is_empty()),
            trace_id: &trace,
            recovery_suggestion: recovery,
        },
    )
    .await?;
    if !transitioned {
        tx.commit().await?;
        return Ok(());
    }
    let task_status = match status {
        "completed" => "succeeded",
        "cancelled" => "cancelled",
        _ if retryable => "queued",
        _ => "failed",
    };
    if retryable {
        let retry_at = chrono::Utc::now()
            + crate::workers::task_scheduler::retry_delay(launch.attempt, launch.scheduler_policy);
        devrail_runs::requeue_task_after_run(&mut tx, launch.task_id, retry_at, reason).await?;
        crate::app_metrics::record_scheduler_retry();
    } else {
        devrail_runs::update_task_status(&mut tx, launch.task_id, task_status).await?;
    }
    let event_idempotency = format!("terminal:{status}:{reason}");
    let event_payload = json!({"status":status,"exitReason":reason,"exitCode":code});
    devrail_runs::append_event(
        &mut tx,
        &devrail_runs::NewRunEvent {
            run_id: launch.run_id,
            organization_id: launch.organization_id,
            department_id: launch.department_id,
            owner_user_id: launch.owner_user_id,
            event_type: "turn_complete",
            source_event_id: None,
            idempotency_key: &event_idempotency,
            payload: &event_payload,
            summary: Some(reason),
        },
    )
    .await?;
    let (level, title, notification_event) = if status == "completed" {
        ("success", "运行已完成", "run.completed")
    } else if status == "cancelled" {
        ("warning", "运行已取消", "run.cancelled")
    } else if retryable {
        ("warning", "运行失败，任务将自动重试", "run.failed")
    } else {
        ("error", "运行失败", "run.failed")
    };
    let source_key = format!("run:{}:{}", launch.run_id, reason);
    let deep_link = format!("/devrail/runs/{}", launch.run_id);
    crate::repositories::devrail_notifications::create(
        &mut tx,
        &crate::repositories::devrail_notifications::NewNotification {
            organization_id: launch.organization_id,
            department_id: launch.department_id,
            recipient_user_id: launch.owner_user_id,
            event_type: notification_event,
            level,
            title,
            summary: recovery.unwrap_or("运行状态已更新"),
            resource_type: Some("devrail_run"),
            resource_id: Some(launch.run_id),
            deep_link: Some(&deep_link),
            source_key: &source_key,
        },
    )
    .await?;
    crate::repositories::devrail_notifications::outbox(
        &mut tx,
        launch.organization_id,
        "notification.created",
        "devrail_run",
        Some(launch.run_id),
        &json!({"notificationSource":source_key}),
    )
    .await?;
    tx.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::{ActorContext, ActorType, DataScope};
    use crate::db::DATABASE_TEST_LOCK;
    use crate::repositories::devrail_runs;
    use std::collections::BTreeSet;

    async fn test_pool() -> Option<PgPool> {
        let database_url = std::env::var("TEST_DATABASE_URL").ok()?;
        let pool = crate::db::init_pool(&database_url).await.ok()?;
        crate::db::run_migrations(&pool).await.ok()?;
        Some(pool)
    }
    #[test]
    fn sanitizer_removes_credentials_and_bounds_strings() {
        let value = json!({"token":"hidden","message":"ok","nested":{"password":"hidden"}});
        let safe = sanitize(&value);
        assert!(safe.get("token").is_none());
        assert!(safe.get("nested").and_then(|v| v.get("password")).is_none());
        assert_eq!(safe.get("message").and_then(Value::as_str), Some("ok"));
    }
    #[test]
    fn event_types_are_product_safe() {
        let (kind, _, _, _) = classify_event(&json!({"type":"agent_message","message":"hello"}));
        assert_eq!(kind, "agent_message");
        let (kind, _, _, payload) =
            classify_event(&json!({"type":"reasoning_summary","summary":"private"}));
        assert_eq!(kind, "reasoning_summary");
        assert_eq!(payload, json!({"summary":"private"}));
    }

    #[test]
    fn followup_tool_parser_accepts_only_the_fixed_tool_and_closed_payload() {
        let valid = json!({
            "method": "devrail/followup.create",
            "params": {"idempotencyKey":"evt-1","title":"后续","goal":"验证"}
        });
        assert!(matches!(followup_proposal(&valid), Some(Ok(_))));
        let nested = json!({
            "type": "item/completed",
            "item": {"name":"devrail_followup_create","arguments":"{\"idempotencyKey\":\"evt-2\",\"title\":\"后续\",\"goal\":\"验证\"}"}
        });
        assert!(matches!(followup_proposal(&nested), Some(Ok(_))));
        assert!(followup_proposal(&json!({"method":"database.query","params":{}})).is_none());
        assert!(matches!(
            followup_proposal(
                &json!({"method":"devrail/followup.create","params":{"idempotencyKey":"x","title":"后续","goal":"验证","organizationId":1}})
            ),
            Some(Err(()))
        ));
    }

    #[test]
    fn transport_failures_are_classified_for_recovery() {
        assert_eq!(
            classify_failure(true, "broken pipe"),
            "transport_disconnect"
        );
        assert_eq!(classify_failure(true, "read EOF"), "transport_read_error");
        assert_eq!(
            classify_failure(true, "write flush failed"),
            "transport_write_error"
        );
        assert!(recovery_for_failure(true, "connection reset").contains("恢复"));
    }

    #[test]
    fn transport_recovery_is_bounded() {
        assert!(crate::repositories::devrail_runs::can_transport_recover(0));
        assert!(crate::repositories::devrail_runs::can_transport_recover(1));
        assert!(!crate::repositories::devrail_runs::can_transport_recover(2));
        assert!(!crate::repositories::devrail_runs::can_transport_recover(
            99
        ));
    }

    #[test]
    fn automatic_retry_is_bounded_and_does_not_retry_policy_failures() {
        let launch = RunLaunch {
            run_id: 1,
            task_id: 2,
            organization_id: 3,
            department_id: None,
            owner_user_id: 4,
            cwd: PathBuf::from("/tmp"),
            input: "任务".to_string(),
            resume_thread_id: None,
            resume_turn_id: None,
            attempt: 1,
            max_attempts: 3,
            automatic: true,
            scheduler_policy: SchedulerPolicy::default(),
        };
        assert!(should_retry_automatically(&launch, "failed", "stall"));
        assert!(!should_retry_automatically(
            &launch,
            "failed",
            "quality_gate_failed"
        ));
        assert!(!should_retry_automatically(
            &RunLaunch {
                attempt: 3,
                ..launch
            },
            "failed",
            "timeout"
        ));
    }

    #[tokio::test]
    async fn stalled_and_disconnected_processes_recover_without_duplicate_runs() {
        let _guard = DATABASE_TEST_LOCK.lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let (owner_user_id, organization_id, department_id, task_id) =
            devrail_runs::create_harness_test_task(&pool, &suffix)
                .await
                .expect("create Harness test task");
        let workspace = std::env::temp_dir().join(format!("devrail-harness-stall-{suffix}"));
        tokio::fs::create_dir_all(&workspace)
            .await
            .expect("create controlled workspace");
        tokio::fs::write(
            workspace.join("app-server"),
            b"IFS= read -r line\nprintf '%s\\n' '{\"id\":\"initialize\",\"result\":{}}'\nsleep 30\n",
        )
        .await
        .expect("write fake app-server");
        let actor = ActorContext {
            actor_type: ActorType::System,
            user_id: owner_user_id,
            session_id: 0,
            organization_id,
            department_id,
            data_scope: DataScope::Organization,
            permission_codes: BTreeSet::new(),
        };
        let mut tx = pool.begin().await.expect("begin run transaction");
        let snapshot_id = devrail_runs::create_snapshot(
            &mut tx,
            &actor,
            task_id,
            &json!({"goal":"验证 stall 恢复"}),
            department_id,
        )
        .await
        .expect("create snapshot");
        let idempotency_key = format!("scheduler:{task_id}:1");
        let policy_value = json!({"version":"stall-test"});
        let startup_args = json!(["app-server"]);
        let workflow_snapshot = json!({"source":"legacy","version":"legacy-v1","digest":"0000000000000000000000000000000000000000000000000000000000000000"});
        let run = devrail_runs::create_run(
            &mut tx,
            &devrail_runs::NewRun {
                actor: &actor,
                task_id,
                snapshot_id,
                idempotency_key: &idempotency_key,
                attempt: 1,
                task_revision: 1,
                workflow_source: "legacy",
                workflow_version: "legacy-v1",
                workflow_digest: "0000000000000000000000000000000000000000000000000000000000000000",
                workflow_snapshot: &workflow_snapshot,
                actor_type: "system",
                parent_run_id: None,
                parent_turn_id: None,
                branch_name: None,
                branch_expires_at: None,
                cwd: workspace.to_string_lossy().as_ref(),
                policy: &policy_value,
                startup_args: &startup_args,
                model_id: None,
                department_id,
            },
        )
        .await
        .expect("create run")
        .expect("run inserted");
        tx.commit().await.expect("commit run");

        let scheduler_policy = SchedulerPolicy {
            stall_timeout: Duration::from_secs(1),
            ..SchedulerPolicy::default()
        };
        let supervisor = HarnessSupervisor::new(
            pool.clone(),
            "bash".to_string(),
            1,
            30,
            workspace.to_string_lossy().into_owned(),
            1,
            scheduler_policy,
        );
        supervisor
            .launch(RunLaunch {
                run_id: run.id,
                task_id,
                organization_id,
                department_id,
                owner_user_id,
                cwd: workspace.clone(),
                input: "执行测试".to_string(),
                resume_thread_id: None,
                resume_turn_id: None,
                attempt: 1,
                max_attempts: 3,
                automatic: true,
                scheduler_policy,
            })
            .await
            .expect("launch fake app-server");

        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
        let terminal = loop {
            let state = sqlx::query_as::<_, (String, Option<String>, String)>(
                "SELECT status, exit_reason, cleanup_status FROM devrail_runs WHERE id=$1",
            )
            .bind(run.id)
            .fetch_one(&pool)
            .await
            .expect("read run state");
            if state.0 == "failed" {
                break state;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "stalled run did not reach a terminal state"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        assert_eq!(terminal.1.as_deref(), Some("stall"));
        assert_eq!(terminal.2, "completed");
        let task_state = sqlx::query_as::<_, (String, i32, Option<String>)>(
            "SELECT status, scheduler_retry_count, scheduler_last_error
             FROM devrail_tasks WHERE id=$1",
        )
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .expect("read retried task");
        assert_eq!(task_state.0, "queued");
        assert_eq!(task_state.1, 1);
        assert_eq!(task_state.2.as_deref(), Some("stall"));
        let notifications = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM devrail_notifications
             WHERE recipient_user_id=$1 AND source_key=$2",
        )
        .bind(owner_user_id)
        .bind(format!("run:{}:stall", run.id))
        .fetch_one(&pool)
        .await
        .expect("count stall notifications");
        assert_eq!(notifications, 1);

        tokio::fs::write(
            workspace.join("app-server"),
            br#"IFS= read -r initialize
printf '%s\n' '{"id":"initialize","result":{}}'
IFS= read -r thread_command
IFS= read -r turn_command
if [ ! -f transport-recovered ]; then
  : > transport-recovered
  printf '%s\n' '{"event_id":"thread-known","type":"agent_message","thread_id":"thread-transport","turn_id":"turn-transport"}'
  exec 1>&-
  sleep 30
fi
printf '%s\n' "$thread_command" > recovery-command.log
sleep 30
"#,
        )
        .await
        .expect("write disconnecting app-server");
        devrail_runs::prepare_harness_test_attempt(&pool, task_id, 2)
            .await
            .expect("prepare transport recovery attempt");
        let second_key = format!("scheduler:{task_id}:2");
        let mut tx = pool.begin().await.expect("begin recovery run");
        let recovery_run = devrail_runs::create_run(
            &mut tx,
            &devrail_runs::NewRun {
                actor: &actor,
                task_id,
                snapshot_id,
                idempotency_key: &second_key,
                attempt: 2,
                task_revision: 1,
                workflow_source: "legacy",
                workflow_version: "legacy-v1",
                workflow_digest: "0000000000000000000000000000000000000000000000000000000000000000",
                workflow_snapshot: &workflow_snapshot,
                actor_type: "system",
                parent_run_id: Some(run.id),
                parent_turn_id: None,
                branch_name: None,
                branch_expires_at: None,
                cwd: workspace.to_string_lossy().as_ref(),
                policy: &policy_value,
                startup_args: &startup_args,
                model_id: None,
                department_id,
            },
        )
        .await
        .expect("create transport recovery run")
        .expect("transport recovery run inserted");
        tx.commit().await.expect("commit transport recovery run");
        let recovery_policy = SchedulerPolicy {
            stall_timeout: Duration::from_secs(10),
            ..SchedulerPolicy::default()
        };
        let recovery_supervisor = HarnessSupervisor::new(
            pool.clone(),
            "bash".to_string(),
            1,
            30,
            workspace.to_string_lossy().into_owned(),
            1,
            recovery_policy,
        );
        recovery_supervisor
            .launch(RunLaunch {
                run_id: recovery_run.id,
                task_id,
                organization_id,
                department_id,
                owner_user_id,
                cwd: workspace.clone(),
                input: "验证断流恢复".to_string(),
                resume_thread_id: None,
                resume_turn_id: None,
                attempt: 2,
                max_attempts: 3,
                automatic: true,
                scheduler_policy: recovery_policy,
            })
            .await
            .expect("launch disconnecting app-server");
        let recovery_log = workspace.join("recovery-command.log");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
        while !recovery_log.exists() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "transport recovery did not start"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let recovery_command = tokio::fs::read_to_string(&recovery_log)
            .await
            .expect("read recovery command");
        assert!(recovery_command.contains("thread/resume"));
        assert!(recovery_command.contains("thread-transport"));
        assert_eq!(
            recovery_supervisor.running_run_ids().await,
            vec![recovery_run.id]
        );
        recovery_supervisor
            .interrupt(recovery_run.id)
            .await
            .expect("recovered run remains controllable");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let state = sqlx::query_as::<_, (String, i32)>(
                "SELECT status, recovery_attempts FROM devrail_runs WHERE id=$1",
            )
            .bind(recovery_run.id)
            .fetch_one(&pool)
            .await
            .expect("read recovered run state");
            if state.0 == "cancelled" {
                assert_eq!(state.1, 1);
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "recovered run did not stop after interrupt"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        tokio::fs::write(
            workspace.join("app-server"),
            b"IFS= read -r line\nprintf '%s\\n' '{\"id\":\"initialize\",\"result\":{}}'\nsleep 30\n",
        )
        .await
        .expect("write timing-out app-server");
        devrail_runs::prepare_harness_test_attempt(&pool, task_id, 3)
            .await
            .expect("prepare timeout attempt");
        let timeout_key = format!("scheduler:{task_id}:3");
        let mut tx = pool.begin().await.expect("begin timeout run");
        let timeout_run = devrail_runs::create_run(
            &mut tx,
            &devrail_runs::NewRun {
                actor: &actor,
                task_id,
                snapshot_id,
                idempotency_key: &timeout_key,
                attempt: 3,
                task_revision: 1,
                workflow_source: "legacy",
                workflow_version: "legacy-v1",
                workflow_digest: "0000000000000000000000000000000000000000000000000000000000000000",
                workflow_snapshot: &workflow_snapshot,
                actor_type: "system",
                parent_run_id: Some(recovery_run.id),
                parent_turn_id: Some("turn-transport"),
                branch_name: None,
                branch_expires_at: None,
                cwd: workspace.to_string_lossy().as_ref(),
                policy: &policy_value,
                startup_args: &startup_args,
                model_id: None,
                department_id,
            },
        )
        .await
        .expect("create timeout run")
        .expect("timeout run inserted");
        tx.commit().await.expect("commit timeout run");
        let timeout_policy = SchedulerPolicy {
            stall_timeout: Duration::from_secs(10),
            ..SchedulerPolicy::default()
        };
        let timeout_supervisor = HarnessSupervisor::new(
            pool.clone(),
            "bash".to_string(),
            1,
            1,
            workspace.to_string_lossy().into_owned(),
            1,
            timeout_policy,
        );
        timeout_supervisor
            .launch(RunLaunch {
                run_id: timeout_run.id,
                task_id,
                organization_id,
                department_id,
                owner_user_id,
                cwd: workspace.clone(),
                input: "验证超时清理".to_string(),
                resume_thread_id: None,
                resume_turn_id: None,
                attempt: 3,
                max_attempts: 3,
                automatic: true,
                scheduler_policy: timeout_policy,
            })
            .await
            .expect("launch timing-out app-server");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let state = sqlx::query_as::<_, (String, Option<String>, String)>(
                "SELECT status, exit_reason, cleanup_status FROM devrail_runs WHERE id=$1",
            )
            .bind(timeout_run.id)
            .fetch_one(&pool)
            .await
            .expect("read timeout state");
            if state.0 == "failed" {
                assert_eq!(state.1.as_deref(), Some("timeout"));
                assert_eq!(state.2, "completed");
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timeout run did not stop"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        tokio::fs::remove_dir_all(&workspace)
            .await
            .expect("remove controlled test workspace");
    }
}
