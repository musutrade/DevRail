//! The only component allowed to start Codex.  The browser talks to the API;
//! this worker owns the controlled app-server process and its JSONL streams.

use crate::repositories::devrail_runs;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
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
}

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
    Interrupt,
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
}

impl HarnessSupervisor {
    pub fn new(
        pool: PgPool,
        command: String,
        max_concurrency: usize,
        max_duration_secs: i64,
        workspace_root: String,
        graceful_interrupt_secs: i64,
    ) -> Self {
        Self {
            pool,
            command: Arc::new(command),
            max_duration: Duration::from_secs(max_duration_secs as u64),
            graceful_interrupt: Duration::from_secs(graceful_interrupt_secs as u64),
            workspace_root: Arc::new(PathBuf::from(workspace_root)),
            slots: Arc::new(Semaphore::new(max_concurrency)),
            controls: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn launch(&self, launch: RunLaunch) -> Result<(), SupervisorError> {
        if !launch.cwd.starts_with(self.workspace_root.as_ref()) {
            return Err(SupervisorError::Workspace);
        }
        let slot = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| SupervisorError::Capacity)?;
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
        let mut child = command.spawn().map_err(|e| {
            if let Ok(mut map) = self.controls.try_lock() {
                map.remove(&launch.run_id);
            }
            SupervisorError::Spawn(e.to_string())
        })?;
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
            run_process(supervisor, launch, child, stdin, stdout, stderr, rx, slot).await;
        });
        Ok(())
    }

    pub async fn interrupt(&self, run_id: i64) -> Result<(), SupervisorError> {
        let sender = self
            .controls
            .lock()
            .await
            .get(&run_id)
            .cloned()
            .ok_or(SupervisorError::ControlUnavailable)?;
        sender
            .send(ControlMessage::Interrupt)
            .await
            .map_err(|_| SupervisorError::ControlUnavailable)
    }

    pub async fn recover_stale_runs(&self) -> Result<u64, sqlx::Error> {
        devrail_runs::recover_stale_runs(&self.pool).await
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Process runner receives all isolated stream handles"
)]
async fn run_process(
    supervisor: HarnessSupervisor,
    launch: RunLaunch,
    mut child: Child,
    mut stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    stderr: tokio::process::ChildStderr,
    mut controls: mpsc::Receiver<ControlMessage>,
    _slot: tokio::sync::OwnedSemaphorePermit,
) {
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
    let _ = write_json(
        &mut stdin,
        json!({"id":"thread-start","method":"thread/start","params":{"cwd":launch.cwd}}),
    )
    .await;
    let _ = write_json(
        &mut stdin,
        json!({"id":"turn-start","method":"turn/start","params":{"input":launch.input}}),
    )
    .await;
    let _ = persist_started(&pool, &launch).await;
    let _ = persist_event(
        &pool,
        &launch,
        "run_started",
        None,
        json!({"cwd": launch.cwd}),
        Some("Harness 已启动"),
    )
    .await;
    let mut out_line = String::new();
    let mut err_line = String::new();
    let mut stderr_summary = String::new();
    let mut protocol_failed = false;
    let mut timeout_sleep = Box::pin(tokio::time::sleep(supervisor.max_duration));

    loop {
        tokio::select! {
            command = controls.recv() => {
                if matches!(command, Some(ControlMessage::Interrupt)) {
                    let _ = write_json(&mut stdin, json!({"method":"turn/interrupt","params":{}})).await;
                    let status = tokio::time::timeout(supervisor.graceful_interrupt, child.wait()).await;
                    let exit_code = match status { Ok(Ok(s)) => s.code(), _ => { let _ = child.start_kill(); child.wait().await.ok().and_then(|s| s.code()) } };
                    let _ = finish_run(&pool, &launch, "cancelled", "interrupted", exit_code, Some(&stderr_summary), Some("运行已由用户中断")).await;
                    break;
                }
            }
            result = out_reader.read_line(&mut out_line) => {
                match result {
                    Ok(0) => {},
                    Ok(_) => {
                        let line = out_line.trim();
                        if !line.is_empty() && !handle_stdout(&pool, &launch, line).await { protocol_failed = true; let _ = child.start_kill(); }
                        out_line.clear();
                    }
                    Err(_) => { protocol_failed = true; let _ = child.start_kill(); }
                }
            }
            result = err_reader.read_line(&mut err_line) => {
                match result {
                    Ok(0) => {},
                    Ok(_) => { append_summary(&mut stderr_summary, err_line.trim()); err_line.clear(); }
                    Err(_) => {}
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
                let (status, reason, recovery) = if protocol_failed || code != Some(0) { ("failed", "process_exit", Some("检查 Harness stderr 摘要并重试")) } else { ("completed", "completed", None) };
                let _ = finish_run(&pool, &launch, status, reason, code, Some(&stderr_summary), recovery).await;
                break;
            }
        }
    }
    supervisor.controls.lock().await.remove(&launch.run_id);
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
    let _ = persist_event(
        pool,
        launch,
        &event_type,
        source_id.as_deref(),
        payload,
        summary.as_deref(),
    )
    .await;
    true
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
        launch.run_id,
        launch.organization_id,
        launch.department_id,
        launch.owner_user_id,
        event_type,
        source_id,
        &idempotency,
        &payload,
        summary,
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
    let trace = uuid::Uuid::new_v4().to_string();
    devrail_runs::update_run_terminal(
        &mut tx,
        launch.run_id,
        status,
        reason,
        code,
        stderr.filter(|s| !s.is_empty()),
        &trace,
        recovery,
    )
    .await?;
    let task_status = match status {
        "completed" => "succeeded",
        "cancelled" => "cancelled",
        _ => "failed",
    };
    devrail_runs::update_task_status(&mut tx, launch.task_id, task_status).await?;
    devrail_runs::append_event(
        &mut tx,
        launch.run_id,
        launch.organization_id,
        launch.department_id,
        launch.owner_user_id,
        "turn_complete",
        None,
        &format!("terminal:{status}:{reason}"),
        &json!({"status":status,"exitReason":reason,"exitCode":code}),
        Some(reason),
    )
    .await?;
    tx.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
