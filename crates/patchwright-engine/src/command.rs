use anyhow::{Context, Result, bail};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    os::unix::{fs::PermissionsExt, process::CommandExt as _},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command, task::JoinHandle, time::timeout};

const MAX_COMMAND_OUTPUT_BYTES: usize = 1024 * 1024;
const TERMINATION_GRACE: Duration = Duration::from_secs(1);

#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
    pub timeout: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_sha256: String,
    pub stdout_bytes: u64,
    pub stderr_sha256: String,
    pub stderr_bytes: u64,
}

pub struct CommandRunner;

impl CommandRunner {
    pub fn resolve_executable(program: &str) -> Result<PathBuf> {
        if program.trim() != program || program.is_empty() || program.chars().any(char::is_control)
        {
            bail!("command executable is invalid");
        }
        let program_path = Path::new(program);
        if program_path.is_absolute() {
            return validate_executable(program_path);
        }
        if program_path.components().count() != 1 {
            bail!("relative command executable must be a basename");
        }
        let mut roots = vec![
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
            PathBuf::from("/usr/sbin"),
            PathBuf::from("/sbin"),
        ];
        if let Some(home) = std::env::var_os("HOME") {
            roots.insert(0, PathBuf::from(home).join(".cargo/bin"));
        }
        roots
            .into_iter()
            .map(|root| root.join(program))
            .find(|candidate| candidate.exists())
            .map_or_else(
                || bail!("command executable is not in the trusted tool paths"),
                |candidate| validate_executable(&candidate),
            )
    }

    pub async fn run(spec: CommandSpec) -> Result<CommandOutput> {
        let executable = validate_executable(&spec.executable)?;
        let working_directory = spec
            .working_directory
            .canonicalize()
            .context("canonicalize command working directory")?;
        let mut command = Command::new(&executable);
        command
            .args(&spec.arguments)
            .current_dir(working_directory)
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command.as_std_mut().process_group(0);
        let mut child = command.spawn().context("start verification command")?;
        let process_group = child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .map(Pid::from_raw)
            .context("verification command has no process id")?;
        let stdout = child.stdout.take().context("capture command stdout")?;
        let stderr = child.stderr.take().context("capture command stderr")?;
        let stdout_task = tokio::spawn(read_bounded(stdout));
        let stderr_task = tokio::spawn(read_bounded(stderr));
        let status = if let Ok(result) = timeout(spec.timeout, child.wait()).await {
            result.context("wait for verification command")?
        } else {
            terminate_process_group(process_group, &mut child).await;
            stdout_task.abort();
            stderr_task.abort();
            bail!("command timed out after {} ms", spec.timeout.as_millis());
        };
        let stdout = join_output(stdout_task, "stdout").await?;
        let stderr = join_output(stderr_task, "stderr").await?;
        Ok(CommandOutput {
            success: status.success(),
            exit_code: status.code(),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            stdout_sha256: digest(&stdout),
            stdout_bytes: stdout.len() as u64,
            stderr_sha256: digest(&stderr),
            stderr_bytes: stderr.len() as u64,
        })
    }
}

fn validate_executable(path: &Path) -> Result<PathBuf> {
    let path = path
        .canonicalize()
        .with_context(|| format!("resolve command executable {}", path.display()))?;
    let metadata = path
        .metadata()
        .with_context(|| format!("inspect command executable {}", path.display()))?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        bail!("command executable is not an executable regular file");
    }
    Ok(path)
}

async fn read_bounded(mut reader: impl tokio::io::AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    (&mut reader)
        .take((MAX_COMMAND_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .context("read verification command output")?;
    if bytes.len() > MAX_COMMAND_OUTPUT_BYTES {
        bail!("verification command output exceeded the byte limit");
    }
    Ok(bytes)
}

async fn join_output(task: JoinHandle<Result<Vec<u8>>>, stream: &str) -> Result<Vec<u8>> {
    task.await
        .with_context(|| format!("join verification command {stream} reader"))?
}

async fn terminate_process_group(process_group: Pid, child: &mut tokio::process::Child) {
    let _ = killpg(process_group, Signal::SIGTERM);
    if timeout(TERMINATION_GRACE, child.wait()).await.is_err() {
        let _ = killpg(process_group, Signal::SIGKILL);
        let _ = child.wait().await;
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
