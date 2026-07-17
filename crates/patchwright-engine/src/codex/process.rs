use std::collections::VecDeque;
use std::env;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use nix::errno::Errno;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use uuid::Uuid;

use super::protocol::MAX_LINE_BYTES;

const COMPATIBLE_VERSION_PREFIX: &str = "codex-cli 0.144.";
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const ALLOWED_ENVIRONMENT_KEYS: &[&str] = &[
    "CODEX_HOME",
    "HOME",
    "LANG",
    "LC_ALL",
    "PATH",
    "TMPDIR",
    "USER",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VersionCompatibility {
    Compatible,
    Warning {
        expected_prefix: &'static str,
        actual: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExecutable {
    path: PathBuf,
    version: String,
    compatibility: VersionCompatibility,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexProcessState {
    Starting,
    Ready,
    Stopping,
    Exited,
    Failed,
}

#[derive(Clone, Debug)]
pub struct CodexProcessConfig {
    pub initialization_timeout: Duration,
    pub request_timeout: Duration,
    pub shutdown_grace: Duration,
    pub stderr_capacity: usize,
}

impl Default for CodexProcessConfig {
    fn default() -> Self {
        Self {
            initialization_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
            shutdown_grace: Duration::from_secs(2),
            stderr_capacity: 64 * 1024,
        }
    }
}

#[derive(Clone)]
pub struct CodexProcessFactory {
    executable: CodexExecutable,
    config: CodexProcessConfig,
}

impl CodexProcessFactory {
    #[must_use]
    pub fn new(executable: CodexExecutable, config: CodexProcessConfig) -> Self {
        Self { executable, config }
    }

    pub fn launch(
        &self,
        task_key: impl Into<String>,
        worktree: &Path,
    ) -> Result<CodexProcess, CodexProcessError> {
        let task_key = task_key.into();
        if task_key.trim().is_empty() {
            return Err(CodexProcessError::InvalidTaskKey);
        }
        let worktree = worktree
            .canonicalize()
            .map_err(|source| CodexProcessError::Io {
                operation: "resolve task worktree",
                source,
            })?;
        if !worktree.is_dir() {
            return Err(CodexProcessError::InvalidWorktree(worktree));
        }

        let mut command = Command::new(self.executable.path());
        command
            .arg("app-server")
            .current_dir(&worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command.as_std_mut().process_group(0);
        apply_allowlisted_environment(&mut command);
        let mut child = command.spawn().map_err(|source| CodexProcessError::Io {
            operation: "launch Codex app-server",
            source,
        })?;
        let process_group_id = child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .ok_or(CodexProcessError::MissingProcessId)?;
        let stdin = child
            .stdin
            .take()
            .ok_or(CodexProcessError::MissingPipe("Codex app-server stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or(CodexProcessError::MissingPipe("Codex app-server stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or(CodexProcessError::MissingPipe("Codex app-server stderr"))?;
        let stderr_ring = Arc::new(Mutex::new(VecDeque::with_capacity(
            self.config.stderr_capacity,
        )));
        let stderr_task = capture_stderr(
            stderr,
            Arc::clone(&stderr_ring),
            self.config.stderr_capacity,
        );

        Ok(CodexProcess {
            task_key,
            generation: Uuid::new_v4(),
            worktree,
            process_group_id,
            child,
            stdin,
            stdout: BufReader::new(stdout),
            stderr_ring,
            stderr_task: Some(stderr_task),
            config: self.config.clone(),
            state: CodexProcessState::Starting,
            group_active: true,
        })
    }
}

pub struct CodexProcess {
    task_key: String,
    generation: Uuid,
    worktree: PathBuf,
    process_group_id: i32,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr_ring: Arc<Mutex<VecDeque<u8>>>,
    stderr_task: Option<JoinHandle<()>>,
    config: CodexProcessConfig,
    state: CodexProcessState,
    group_active: bool,
}

impl CodexProcess {
    #[must_use]
    pub fn task_key(&self) -> &str {
        &self.task_key
    }

    #[must_use]
    pub fn generation(&self) -> Uuid {
        self.generation
    }

    #[must_use]
    pub fn worktree(&self) -> &Path {
        &self.worktree
    }

    #[must_use]
    pub fn process_group_id(&self) -> i32 {
        self.process_group_id
    }

    #[must_use]
    pub fn state(&self) -> CodexProcessState {
        self.state
    }

    pub fn mark_ready(&mut self) -> Result<(), CodexProcessError> {
        if self.state != CodexProcessState::Starting {
            return Err(CodexProcessError::InvalidStateTransition {
                from: self.state,
                to: CodexProcessState::Ready,
            });
        }
        self.state = CodexProcessState::Ready;
        Ok(())
    }

    pub async fn read_initialization_line(&mut self) -> Result<String, CodexProcessError> {
        self.read_line_with_timeout(self.config.initialization_timeout, "initialize Codex")
            .await
    }

    pub async fn read_line(&mut self) -> Result<String, CodexProcessError> {
        self.read_line_with_timeout(self.config.request_timeout, "read Codex response")
            .await
    }

    pub async fn read_line_for(&mut self, duration: Duration) -> Result<String, CodexProcessError> {
        self.read_line_with_timeout(duration, "poll Codex event")
            .await
    }

    pub async fn write_line(&mut self, line: &str) -> Result<(), CodexProcessError> {
        if line.as_bytes().contains(&b'\n') || line.len() > MAX_LINE_BYTES {
            return Err(CodexProcessError::InvalidProtocolLine);
        }
        let request_timeout = self.config.request_timeout;
        timeout(request_timeout, async {
            self.stdin.write_all(line.as_bytes()).await?;
            self.stdin.write_all(b"\n").await?;
            self.stdin.flush().await
        })
        .await
        .map_err(|_| CodexProcessError::Timeout {
            operation: "write Codex request",
            duration: request_timeout,
        })?
        .map_err(|source| CodexProcessError::Io {
            operation: "write Codex request",
            source,
        })
    }

    pub async fn wait_for_exit(&mut self) -> Result<CodexProcessExit, CodexProcessError> {
        let status = self
            .child
            .wait()
            .await
            .map_err(|source| CodexProcessError::Io {
                operation: "wait for Codex app-server",
                source,
            })?;
        self.cleanup_descendants();
        self.finish_stderr_capture().await;
        self.state = if status.success() {
            CodexProcessState::Exited
        } else {
            CodexProcessState::Failed
        };
        Ok(CodexProcessExit::from(status))
    }

    pub async fn terminate(&mut self) -> Result<(), CodexProcessError> {
        if matches!(
            self.state,
            CodexProcessState::Exited | CodexProcessState::Failed
        ) {
            return Ok(());
        }
        self.state = CodexProcessState::Stopping;
        self.signal_group(Signal::SIGTERM)?;
        let status =
            if let Ok(result) = timeout(self.config.shutdown_grace, self.child.wait()).await {
                result.map_err(|source| CodexProcessError::Io {
                    operation: "wait for Codex app-server termination",
                    source,
                })?
            } else {
                self.signal_group(Signal::SIGKILL)?;
                self.child
                    .wait()
                    .await
                    .map_err(|source| CodexProcessError::Io {
                        operation: "wait for killed Codex app-server",
                        source,
                    })?
            };
        let _ = status;
        // The app-server leader may exit on TERM while descendants in the same recorded
        // process group ignore it and keep inherited pipes open. Always sweep the owned
        // group before awaiting pipe-capture shutdown; ESRCH is already treated as success.
        self.sweep_group_after_leader_exit()?;
        self.group_active = false;
        self.finish_stderr_capture().await;
        self.state = CodexProcessState::Exited;
        Ok(())
    }

    pub async fn stderr_snapshot(&self) -> String {
        let mut ring = self.stderr_ring.lock().await;
        String::from_utf8_lossy(ring.make_contiguous()).into_owned()
    }

    async fn read_line_with_timeout(
        &mut self,
        duration: Duration,
        operation: &'static str,
    ) -> Result<String, CodexProcessError> {
        let mut bytes = Vec::new();
        let count = timeout(duration, self.stdout.read_until(b'\n', &mut bytes))
            .await
            .map_err(|_| CodexProcessError::Timeout {
                operation,
                duration,
            })?
            .map_err(|source| CodexProcessError::Io { operation, source })?;
        if count == 0 {
            return Err(CodexProcessError::UnexpectedEof);
        }
        if bytes.len() > MAX_LINE_BYTES + 1 {
            return Err(CodexProcessError::ProtocolLineTooLarge(bytes.len()));
        }
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
        }
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        String::from_utf8(bytes).map_err(|_| CodexProcessError::InvalidUtf8)
    }

    fn signal_group(&self, signal: Signal) -> Result<(), CodexProcessError> {
        match killpg(Pid::from_raw(self.process_group_id), signal) {
            Ok(()) | Err(Errno::ESRCH) => Ok(()),
            Err(source) => Err(CodexProcessError::Signal {
                process_group_id: self.process_group_id,
                signal,
                source,
            }),
        }
    }

    fn sweep_group_after_leader_exit(&self) -> Result<(), CodexProcessError> {
        match killpg(Pid::from_raw(self.process_group_id), Signal::SIGKILL) {
            Ok(()) | Err(Errno::ESRCH | Errno::EPERM) => Ok(()),
            Err(source) => Err(CodexProcessError::Signal {
                process_group_id: self.process_group_id,
                signal: Signal::SIGKILL,
                source,
            }),
        }
    }

    fn cleanup_descendants(&mut self) {
        if self.group_active {
            let _ = self.signal_group(Signal::SIGKILL);
            self.group_active = false;
        }
    }

    async fn finish_stderr_capture(&mut self) {
        if let Some(task) = self.stderr_task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for CodexProcess {
    fn drop(&mut self) {
        if self.group_active {
            let _ = killpg(Pid::from_raw(self.process_group_id), Signal::SIGKILL);
            self.group_active = false;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodexProcessExit {
    pub code: Option<i32>,
    pub success: bool,
}

impl From<ExitStatus> for CodexProcessExit {
    fn from(status: ExitStatus) -> Self {
        Self {
            code: status.code(),
            success: status.success(),
        }
    }
}

impl CodexExecutable {
    pub async fn discover(explicit_path: Option<&Path>) -> Result<Self, CodexProcessError> {
        let path = match explicit_path {
            Some(path) => path.to_path_buf(),
            None => discover_on_path()
                .ok_or_else(|| CodexProcessError::MissingExecutable(PathBuf::from("codex")))?,
        };
        let metadata = std::fs::metadata(&path)
            .map_err(|_| CodexProcessError::MissingExecutable(path.clone()))?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err(CodexProcessError::NotExecutable(path));
        }
        let path = path
            .canonicalize()
            .map_err(|source| CodexProcessError::Io {
                operation: "canonicalize executable",
                source,
            })?;

        let mut command = Command::new(&path);
        command
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command.as_std_mut().process_group(0);
        apply_allowlisted_environment(&mut command);
        let child = command.spawn().map_err(|source| CodexProcessError::Io {
            operation: "launch Codex version probe",
            source,
        })?;
        let process_group_id = child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .ok_or(CodexProcessError::MissingProcessId)?;
        let output =
            if let Ok(result) = timeout(VERSION_PROBE_TIMEOUT, child.wait_with_output()).await {
                result.map_err(|source| CodexProcessError::Io {
                    operation: "probe Codex version",
                    source,
                })?
            } else {
                let _ = killpg(Pid::from_raw(process_group_id), Signal::SIGKILL);
                return Err(CodexProcessError::VersionProbeTimeout {
                    path,
                    duration: VERSION_PROBE_TIMEOUT,
                });
            };
        if !output.status.success() {
            return Err(CodexProcessError::VersionProbeFailed {
                path,
                status: output.status.code(),
            });
        }
        let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if version.is_empty() {
            return Err(CodexProcessError::EmptyVersion);
        }
        let compatibility = if version.starts_with(COMPATIBLE_VERSION_PREFIX) {
            VersionCompatibility::Compatible
        } else {
            VersionCompatibility::Warning {
                expected_prefix: COMPATIBLE_VERSION_PREFIX,
                actual: version.clone(),
            }
        };
        Ok(Self {
            path,
            version,
            compatibility,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub fn compatibility(&self) -> &VersionCompatibility {
        &self.compatibility
    }
}

#[derive(Debug, Error)]
pub enum CodexProcessError {
    #[error("Codex executable does not exist: {0}")]
    MissingExecutable(PathBuf),
    #[error("Codex path is not an executable file: {0}")]
    NotExecutable(PathBuf),
    #[error("Codex version probe failed for {path} with status {status:?}")]
    VersionProbeFailed { path: PathBuf, status: Option<i32> },
    #[error("Codex version probe timed out after {duration:?}: {path}")]
    VersionProbeTimeout { path: PathBuf, duration: Duration },
    #[error("Codex version probe returned an empty version")]
    EmptyVersion,
    #[error("task key must not be empty")]
    InvalidTaskKey,
    #[error("Codex task worktree is not a directory: {0}")]
    InvalidWorktree(PathBuf),
    #[error("launched Codex app-server has no process id")]
    MissingProcessId,
    #[error("launched process is missing {0}")]
    MissingPipe(&'static str),
    #[error("invalid process state transition from {from:?} to {to:?}")]
    InvalidStateTransition {
        from: CodexProcessState,
        to: CodexProcessState,
    },
    #[error("timed out after {duration:?} while attempting to {operation}")]
    Timeout {
        operation: &'static str,
        duration: Duration,
    },
    #[error("Codex app-server stdout ended before a protocol line arrived")]
    UnexpectedEof,
    #[error("Codex app-server emitted a non-UTF-8 protocol line")]
    InvalidUtf8,
    #[error("Codex protocol line is invalid")]
    InvalidProtocolLine,
    #[error("Codex protocol line exceeded the bound with {0} bytes")]
    ProtocolLineTooLarge(usize),
    #[error("failed to send {signal:?} to process group {process_group_id}: {source}")]
    Signal {
        process_group_id: i32,
        signal: Signal,
        source: Errno,
    },
    #[error("failed to {operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
}

fn discover_on_path() -> Option<PathBuf> {
    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|directory| directory.join("codex"))
            .find(|candidate| candidate.is_file())
    })
}

fn apply_allowlisted_environment(command: &mut Command) {
    command.env_clear();
    for key in ALLOWED_ENVIRONMENT_KEYS {
        if let Some(value) = env::var_os(key) {
            command.env(key, value);
        }
    }
}

fn capture_stderr(
    mut stderr: tokio::process::ChildStderr,
    ring: Arc<Mutex<VecDeque<u8>>>,
    capacity: usize,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buffer = [0_u8; 4096];
        loop {
            let Ok(count) = stderr.read(&mut buffer).await else {
                break;
            };
            if count == 0 {
                break;
            }
            if capacity == 0 {
                continue;
            }
            let mut ring = ring.lock().await;
            ring.extend(&buffer[..count]);
            let excess = ring.len().saturating_sub(capacity);
            if excess > 0 {
                ring.drain(..excess);
            }
        }
    })
}
