use anyhow::{Context, Result, bail};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
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
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        configure_verification_environment(&mut command);
        command.as_std_mut().process_group(0);
        let mut child = command.spawn().context("start verification command")?;
        let process_group = child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .map(Pid::from_raw)
            .context("verification command has no process id")?;
        let stdout = child.stdout.take().context("capture command stdout")?;
        let stderr = child.stderr.take().context("capture command stderr")?;
        let mut stdout_task = tokio::spawn(read_bounded(stdout));
        let mut stderr_task = tokio::spawn(read_bounded(stderr));
        let execution = timeout(spec.timeout, async {
            let status = child
                .wait()
                .await
                .context("wait for verification command")?;
            let (stdout, stderr) = tokio::try_join!(
                join_output(&mut stdout_task, "stdout"),
                join_output(&mut stderr_task, "stderr")
            )?;
            Ok::<_, anyhow::Error>((status, stdout, stderr))
        })
        .await;
        let (status, stdout, stderr) = match execution {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => {
                terminate_process_group(process_group, &mut child).await;
                abort_output_readers(&mut stdout_task, &mut stderr_task).await;
                return Err(error);
            }
            Err(_) => {
                terminate_process_group(process_group, &mut child).await;
                abort_output_readers(&mut stdout_task, &mut stderr_task).await;
                bail!("command timed out after {} ms", spec.timeout.as_millis());
            }
        };
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

fn configure_verification_environment(command: &mut Command) {
    let mut path = Vec::<PathBuf>::new();
    if let Some(home) = std::env::var_os("HOME") {
        path.push(PathBuf::from(home).join(".cargo/bin"));
    }
    path.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ]);
    let joined = std::env::join_paths(path).unwrap_or_else(|_| OsString::from("/usr/bin:/bin"));
    command.env("PATH", joined);
    for key in [
        "HOME",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "TMPDIR",
        "DEVELOPER_DIR",
        "SDKROOT",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "LANG",
        "LC_ALL",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

fn validate_executable(path: &Path) -> Result<PathBuf> {
    let resolved = path
        .canonicalize()
        .with_context(|| format!("resolve command executable {}", path.display()))?;
    let metadata = resolved
        .metadata()
        .with_context(|| format!("inspect command executable {}", resolved.display()))?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        bail!("command executable is not an executable regular file");
    }
    Ok(path.to_path_buf())
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

async fn join_output(task: &mut JoinHandle<Result<Vec<u8>>>, stream: &str) -> Result<Vec<u8>> {
    task.await
        .with_context(|| format!("join verification command {stream} reader"))?
}

async fn terminate_process_group(process_group: Pid, child: &mut tokio::process::Child) {
    let _ = killpg(process_group, Signal::SIGTERM);
    let first_wait = timeout(TERMINATION_GRACE, child.wait()).await;
    // The leader may have already exited while a descendant keeps an inherited pipe
    // open. Sweep the owned group even when waiting for the leader completed normally.
    let _ = killpg(process_group, Signal::SIGKILL);
    if first_wait.is_err() {
        let _ = timeout(TERMINATION_GRACE, child.wait()).await;
    }
}

async fn abort_output_readers(
    stdout_task: &mut JoinHandle<Result<Vec<u8>>>,
    stderr_task: &mut JoinHandle<Result<Vec<u8>>>,
) {
    tokio::join!(
        abort_output_reader(stdout_task),
        abort_output_reader(stderr_task)
    );
}

async fn abort_output_reader(task: &mut JoinHandle<Result<Vec<u8>>>) {
    if task.is_finished() {
        return;
    }
    task.abort();
    let _ = timeout(TERMINATION_GRACE, task).await;
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
