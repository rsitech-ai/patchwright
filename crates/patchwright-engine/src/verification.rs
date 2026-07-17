use crate::{CommandRunner, CommandSpec, EventStore, RepositoryService, TaskCheckpoint};
use chrono::{DateTime, Utc};
use patchwright_core::{Task, TaskId, TaskState, VerificationCommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Mutex, time::Duration};
use thiserror::Error;
use uuid::Uuid;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationEvidence {
    pub run_id: Uuid,
    pub task_id: TaskId,
    pub ordinal: u32,
    pub command_sha256: String,
    pub program: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout_sha256: String,
    pub stdout_bytes: u64,
    pub stderr_sha256: String,
    pub stderr_bytes: u64,
    pub failure_kind: Option<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum VerificationError {
    #[error("task was not found")]
    TaskMissing,
    #[error("task contract was not found")]
    ContractMissing,
    #[error("task state {0} cannot be verified")]
    InvalidState(TaskState),
    #[error("task worktree is dirty")]
    DirtyWorktree,
    #[error("verification command {ordinal} failed")]
    CommandFailed { ordinal: u32 },
    #[error("task or worktree changed while verification was running")]
    StaleResult,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub async fn verify_task_for_delivery(
    task_id: TaskId,
    store: &Mutex<EventStore>,
) -> Result<Task, VerificationError> {
    let (mut task, contract) = {
        let store = store
            .lock()
            .map_err(|_| anyhow::anyhow!("event store lock poisoned"))?;
        let mut task = store
            .load_task(task_id)?
            .ok_or(VerificationError::TaskMissing)?;
        if task.state == TaskState::AwaitingDeliveryApproval {
            return Ok(task);
        }
        if task.state == TaskState::Implementing {
            task.transition(TaskState::Verifying)
                .map_err(anyhow::Error::from)?;
            store.save_task(&task, "Implementation submitted for verification")?;
        }
        if task.state != TaskState::Verifying {
            return Err(VerificationError::InvalidState(task.state));
        }
        let contract = store
            .task_contract(task_id)?
            .ok_or(VerificationError::ContractMissing)?;
        (task, contract)
    };

    let worktree = Path::new(&task.repository_path);
    let before = RepositoryService::inspect(worktree)?;
    if before.dirty {
        return Err(VerificationError::DirtyWorktree);
    }
    run_commands(task_id, worktree, contract.verification_commands(), store).await?;

    let after = RepositoryService::inspect(worktree)?;
    if after.dirty || after.head_sha != before.head_sha {
        return Err(VerificationError::StaleResult);
    }
    let store = store
        .lock()
        .map_err(|_| anyhow::anyhow!("event store lock poisoned"))?;
    if store.load_task(task_id)?.as_ref() != Some(&task) {
        return Err(VerificationError::StaleResult);
    }
    task.transition(TaskState::Reviewing)
        .map_err(anyhow::Error::from)?;
    store.save_task(
        &task,
        "Contract verification commands passed at the captured commit",
    )?;
    task.transition(TaskState::AwaitingDeliveryApproval)
        .map_err(anyhow::Error::from)?;
    let checkpoint = TaskCheckpoint::new(
        task.id,
        task.state,
        "Verified commit is awaiting exact GitHub delivery approval",
    )
    .map_err(anyhow::Error::from)?;
    task.checkpoint_id = Some(checkpoint.id());
    store.save_task_with_checkpoint(
        &task,
        "Task is ready for approval-bound GitHub delivery",
        &checkpoint,
    )?;
    Ok(task)
}

async fn run_commands(
    task_id: TaskId,
    worktree: &Path,
    commands: &[VerificationCommand],
    store: &Mutex<EventStore>,
) -> Result<(), VerificationError> {
    let run_id = Uuid::new_v4();
    for (index, verification) in commands.iter().enumerate() {
        let ordinal = u32::try_from(index + 1).map_err(|_| anyhow::anyhow!("too many commands"))?;
        let started_at = Utc::now();
        let executable = match CommandRunner::resolve_executable(verification.program()) {
            Ok(executable) => executable,
            Err(error) => {
                let evidence = failed_evidence(
                    run_id,
                    task_id,
                    ordinal,
                    verification.program(),
                    verification.args(),
                    started_at,
                    "executableResolution",
                    &error.to_string(),
                );
                save_evidence(store, &evidence)?;
                return Err(VerificationError::CommandFailed { ordinal });
            }
        };
        match CommandRunner::run(CommandSpec {
            executable,
            arguments: verification.args().to_vec(),
            working_directory: worktree.to_owned(),
            timeout: COMMAND_TIMEOUT,
        })
        .await
        {
            Ok(output) => {
                let evidence = VerificationEvidence {
                    run_id,
                    task_id,
                    ordinal,
                    command_sha256: command_digest(verification.program(), verification.args()),
                    program: verification.program().to_owned(),
                    success: output.success,
                    exit_code: output.exit_code,
                    stdout_sha256: output.stdout_sha256,
                    stdout_bytes: output.stdout_bytes,
                    stderr_sha256: output.stderr_sha256,
                    stderr_bytes: output.stderr_bytes,
                    failure_kind: (!output.success).then(|| "nonzeroExit".to_owned()),
                    started_at,
                    completed_at: Utc::now(),
                };
                save_evidence(store, &evidence)?;
                if !output.success {
                    return Err(VerificationError::CommandFailed { ordinal });
                }
            }
            Err(error) => {
                let failure_kind = if error.to_string().contains("timed out") {
                    "timeout"
                } else if error.to_string().contains("byte limit") {
                    "outputLimit"
                } else {
                    "runnerError"
                };
                let evidence = failed_evidence(
                    run_id,
                    task_id,
                    ordinal,
                    verification.program(),
                    verification.args(),
                    started_at,
                    failure_kind,
                    &error.to_string(),
                );
                save_evidence(store, &evidence)?;
                return Err(VerificationError::CommandFailed { ordinal });
            }
        }
    }
    Ok(())
}

fn save_evidence(
    store: &Mutex<EventStore>,
    evidence: &VerificationEvidence,
) -> Result<(), VerificationError> {
    store
        .lock()
        .map_err(|_| anyhow::anyhow!("event store lock poisoned"))?
        .save_verification_evidence(evidence)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn failed_evidence(
    run_id: Uuid,
    task_id: TaskId,
    ordinal: u32,
    program: &str,
    args: &[String],
    started_at: DateTime<Utc>,
    failure_kind: &str,
    error: &str,
) -> VerificationEvidence {
    VerificationEvidence {
        run_id,
        task_id,
        ordinal,
        command_sha256: command_digest(program, args),
        program: program.to_owned(),
        success: false,
        exit_code: None,
        stdout_sha256: digest(b""),
        stdout_bytes: 0,
        stderr_sha256: digest(error.as_bytes()),
        stderr_bytes: error.len() as u64,
        failure_kind: Some(failure_kind.to_owned()),
        started_at,
        completed_at: Utc::now(),
    }
}

fn command_digest(program: &str, args: &[String]) -> String {
    let encoded = serde_json::to_vec(&(program, args)).expect("command digest serialization");
    digest(&encoded)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
