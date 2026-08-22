use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

static CANCELLED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn handle_signal(_signal: libc::c_int) {
    CANCELLED.store(true, Ordering::SeqCst);
}

pub fn install_signal_handlers() {
    #[cfg(unix)]
    unsafe {
        let handler = handle_signal as *const () as libc::sighandler_t;
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
}

pub fn cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}

#[derive(Debug)]
pub struct Task {
    pub label: String,
    program: OsString,
    args: Vec<OsString>,
    cwd: PathBuf,
    env: Vec<(OsString, OsString)>,
    env_remove: Vec<OsString>,
    timeout: Duration,
    log: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskResult {
    pub label: String,
    pub passed: bool,
    pub timed_out: bool,
    pub cancelled: bool,
    pub duration_ms: u128,
    pub log: String,
    pub detail: Option<String>,
}

#[derive(Debug)]
pub struct CapturedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub fn capture(
    program: &str,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
) -> Result<CapturedOutput> {
    capture_command(program, args, cwd, timeout, true)
}

pub fn capture_cleanup(
    program: &str,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
) -> Result<CapturedOutput> {
    capture_command(program, args, cwd, timeout, false)
}

fn capture_command(
    program: &str,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
    observe_cancel: bool,
) -> Result<CapturedOutput> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate_process_tree(&mut command);
    let mut child = command
        .spawn()
        .with_context(|| format!("start internal command {program}"))?;
    let stdout = child
        .stdout
        .take()
        .context("internal command stdout was not captured")?;
    let stderr = child
        .stderr
        .take()
        .context("internal command stderr was not captured")?;
    let stdout_reader = thread::spawn(move || read_all(stdout));
    let stderr_reader = thread::spawn(move || read_all(stderr));
    let started = Instant::now();

    let (status, timed_out, was_cancelled) = loop {
        if let Some(status) = child.try_wait()? {
            break (status, false, false);
        }
        if observe_cancel && cancelled() {
            break (terminate(&mut child)?, false, true);
        }
        if started.elapsed() >= timeout {
            break (terminate(&mut child)?, true, false);
        }
        thread::sleep(Duration::from_millis(50));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| anyhow::anyhow!("internal command stdout reader panicked"))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("internal command stderr reader panicked"))??;

    if was_cancelled {
        bail!("internal command {program} was cancelled");
    }
    if timed_out {
        bail!(
            "internal command {program} timed out after {} ms",
            timeout.as_millis()
        );
    }
    Ok(CapturedOutput {
        status,
        stdout,
        stderr,
    })
}

fn read_all(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    reader.read_to_end(&mut output)?;
    Ok(output)
}

impl Task {
    pub fn new(
        label: impl Into<String>,
        program: impl AsRef<OsStr>,
        cwd: &Path,
        log: PathBuf,
    ) -> Self {
        Self {
            label: label.into(),
            program: program.as_ref().to_os_string(),
            args: Vec::new(),
            cwd: cwd.to_path_buf(),
            env: Vec::new(),
            env_remove: Vec::new(),
            timeout: Duration::from_secs(180),
            log,
        }
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|arg| arg.as_ref().to_os_string()));
        self
    }

    pub fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.env
            .push((key.as_ref().to_os_string(), value.as_ref().to_os_string()));
        self
    }

    pub fn env_remove(mut self, key: impl AsRef<OsStr>) -> Self {
        self.env_remove.push(key.as_ref().to_os_string());
        self
    }

    pub fn timeout(mut self, seconds: u64) -> Self {
        self.timeout = Duration::from_secs(seconds);
        self
    }

    pub fn run(self) -> Result<TaskResult> {
        if let Some(parent) = self.log.parent() {
            fs::create_dir_all(parent)?;
        }
        let stdout = File::create(&self.log)
            .with_context(|| format!("create log {}", self.log.display()))?;
        let stderr = stdout.try_clone()?;
        let started = Instant::now();
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .current_dir(&self.cwd)
            .envs(self.env);
        for name in self.env_remove {
            command.env_remove(name);
        }
        command
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        isolate_process_tree(&mut command);
        let mut child = command
            .spawn()
            .with_context(|| format!("start {}", self.program.to_string_lossy()))?;

        let (status, timed_out, was_cancelled) = loop {
            if let Some(status) = child.try_wait()? {
                break (status, false, false);
            }
            if cancelled() {
                break (terminate(&mut child)?, false, true);
            }
            if started.elapsed() >= self.timeout {
                break (terminate(&mut child)?, true, false);
            }
            thread::sleep(Duration::from_millis(100));
        };

        Ok(TaskResult {
            label: self.label,
            passed: status.success() && !timed_out && !was_cancelled,
            timed_out,
            cancelled: was_cancelled,
            duration_ms: started.elapsed().as_millis(),
            log: self.log.to_string_lossy().to_string(),
            detail: if was_cancelled {
                Some("cancelled".to_string())
            } else if timed_out {
                Some("timed out".to_string())
            } else if status.success() {
                None
            } else {
                status.code().map(|code| format!("exit code {code}"))
            },
        })
    }
}

#[cfg(unix)]
fn isolate_process_tree(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

#[cfg(not(unix))]
fn isolate_process_tree(_command: &mut Command) {}

#[cfg(unix)]
fn terminate(child: &mut Child) -> std::io::Result<ExitStatus> {
    let process_group = -(child.id() as i32);
    // The child is its process-group leader, so this also stops spawned test/build processes.
    unsafe {
        libc::kill(process_group, libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(50));
    }
    unsafe {
        libc::kill(process_group, libc::SIGKILL);
    }
    child.wait()
}

#[cfg(not(unix))]
fn terminate(child: &mut Child) -> std::io::Result<ExitStatus> {
    child.kill()?;
    child.wait()
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::*;

    #[cfg(unix)]
    #[test]
    fn timeout_terminates_the_task() {
        let log = std::env::temp_dir().join(format!("arc-flow-timeout-{}.log", std::process::id()));
        let result = Task::new("timeout fixture", "sleep", Path::new("."), log.clone())
            .args(["5"])
            .timeout(0)
            .run()
            .expect("run timeout fixture");

        assert!(!result.passed);
        assert!(result.timed_out);
        let _ = fs::remove_file(log);
    }

    #[cfg(unix)]
    #[test]
    fn task_can_remove_an_inherited_environment_variable() {
        let log = std::env::temp_dir().join(format!("arc-flow-env-{}.log", std::process::id()));
        let result = Task::new("environment fixture", "env", Path::new("."), log.clone())
            .env("ARC_FLOW_REMOVE_FIXTURE", "must-not-leak")
            .env_remove("ARC_FLOW_REMOVE_FIXTURE")
            .run()
            .expect("run environment fixture");

        assert!(result.passed);
        let output = fs::read_to_string(&log).expect("read environment log");
        assert!(!output.contains("ARC_FLOW_REMOVE_FIXTURE"));
        let _ = fs::remove_file(log);
    }

    #[cfg(unix)]
    #[test]
    fn task_runs_in_an_isolated_session() {
        let log = std::env::temp_dir().join(format!("arc-flow-session-{}.log", std::process::id()));
        let result = Task::new("session fixture", "sh", Path::new("."), log.clone())
            .args(["-c", "ps -o sid= -p $$"])
            .run()
            .expect("run session fixture");

        assert!(result.passed);
        let child_session = fs::read_to_string(&log)
            .expect("read session log")
            .trim()
            .parse::<i32>()
            .expect("parse child session id");
        let parent_session = unsafe { libc::getsid(0) };
        assert_ne!(child_session, parent_session);
        let _ = fs::remove_file(log);
    }

    #[cfg(unix)]
    #[test]
    fn captured_command_has_a_hard_timeout() {
        let args = vec!["-c".to_string(), "sleep 5".to_string()];
        let error = capture("sh", &args, Path::new("/tmp"), Duration::from_millis(100))
            .expect_err("capture must time out");

        assert!(error.to_string().contains("timed out"));
    }
}
