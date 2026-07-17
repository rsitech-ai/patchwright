use crate::RepositoryService;
use patchwright_core::{
    Capability, InstructionDigest, RepositoryBinding, RiskClass, SensitivePath, Task, TaskContract,
    TaskContractDraft, TaskSource, VerificationCommand,
};
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, path::Path, process::Command};
use thiserror::Error;

const MAX_EVIDENCE_BYTES: u64 = 4 * 1024 * 1024;

pub struct RepositoryPlanner;

impl RepositoryPlanner {
    pub fn assess(task: &Task, binding: &RepositoryBinding) -> Result<TaskContract, PlanningError> {
        if task.repository_binding_id != Some(binding.id()) {
            return Err(PlanningError::BindingMismatch);
        }
        let captured_sha = task
            .source
            .head_sha()
            .or_else(|| task.source.base_sha())
            .or_else(|| binding.default_branch_sha())
            .ok_or(PlanningError::SourceShaMissing)?;
        let inspection = RepositoryService::inspect(Path::new(&task.repository_path))
            .map_err(|error| PlanningError::Repository(error.to_string()))?;
        if inspection.dirty {
            return Err(PlanningError::RepositoryDirty);
        }
        if !commit_exists(&inspection.root, captured_sha)? {
            return Err(PlanningError::SourceChanged);
        }

        let mut detected = DetectedContractBoundary::default();
        detect_pair(
            &inspection.root,
            captured_sha,
            "Cargo.toml",
            "Cargo.lock",
            VerificationCommand::new("cargo", ["test", "--workspace"])
                .map_err(|error| PlanningError::Contract(error.to_string()))?,
            &mut detected,
        )?;
        detect_pair(
            &inspection.root,
            captured_sha,
            "Package.swift",
            "Package.resolved",
            VerificationCommand::new("swift", ["test"])
                .map_err(|error| PlanningError::Contract(error.to_string()))?,
            &mut detected,
        )?;
        if detected.commands.is_empty() {
            return Err(PlanningError::UnsupportedRepository);
        }

        let instruction_digests =
            match read_regular_evidence_at_commit(&inspection.root, captured_sha, "AGENTS.md")? {
                Some(content) => vec![
                    InstructionDigest::new("AGENTS.md", digest(&content), 1)
                        .map_err(|error| PlanningError::Contract(error.to_string()))?,
                ],
                None => vec![
                    InstructionDigest::new("resolvedInstructions", digest(b"[]"), 0)
                        .map_err(|error| PlanningError::Contract(error.to_string()))?,
                ],
            };

        let source_sha256 = digest(
            &serde_json::to_vec(&(&task.source, captured_sha))
                .map_err(|error| PlanningError::Contract(error.to_string()))?,
        );
        let repository_sha256 = digest_evidence(&detected.evidence);
        let (goal, acceptance_criteria, required_capabilities) = contract_intent(task)?;
        TaskContract::try_from(TaskContractDraft {
            task_id: task.id,
            source: task.source.clone(),
            repository_binding_id: binding.id(),
            goal,
            acceptance_criteria,
            base_sha: task
                .source
                .base_sha()
                .or_else(|| binding.default_branch_sha())
                .map(ToOwned::to_owned),
            head_sha: task.source.head_sha().map(ToOwned::to_owned),
            source_sha256,
            repository_sha256,
            instruction_digests,
            verification_commands: detected.commands,
            required_capabilities,
            risk: RiskClass::Moderate,
            sensitive_paths: detected.sensitive_paths,
            dependencies: Vec::new(),
        })
        .map_err(|error| PlanningError::Contract(error.to_string()))
    }
}

#[derive(Default)]
struct DetectedContractBoundary {
    evidence: Vec<(&'static str, Vec<u8>)>,
    commands: Vec<VerificationCommand>,
    sensitive_paths: Vec<SensitivePath>,
}

fn detect_pair(
    root: &Path,
    captured_sha: &str,
    manifest: &'static str,
    lockfile: &'static str,
    command: VerificationCommand,
    detected: &mut DetectedContractBoundary,
) -> Result<(), PlanningError> {
    let manifest_content = read_regular_evidence_at_commit(root, captured_sha, manifest)?;
    let lockfile_content = read_regular_evidence_at_commit(root, captured_sha, lockfile)?;
    if manifest_content.is_none() && lockfile_content.is_none() {
        return Ok(());
    }
    let (Some(manifest_content), Some(lockfile_content)) = (manifest_content, lockfile_content)
    else {
        return Err(PlanningError::IncompleteEvidence { manifest, lockfile });
    };
    detected.evidence.push((manifest, manifest_content));
    detected.evidence.push((lockfile, lockfile_content));
    detected.commands.push(command);
    detected.sensitive_paths.push(
        SensitivePath::new(
            lockfile,
            "Dependency resolution is part of the verification boundary",
        )
        .map_err(|error| PlanningError::Contract(error.to_string()))?,
    );
    Ok(())
}

fn commit_exists(root: &Path, captured_sha: &str) -> Result<bool, PlanningError> {
    let object = format!("{captured_sha}^{{commit}}");
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "-e", &object])
        .status()
        .map_err(|error| PlanningError::Repository(error.to_string()))?;
    Ok(status.success())
}

fn read_regular_evidence_at_commit(
    root: &Path,
    captured_sha: &str,
    relative_path: &str,
) -> Result<Option<Vec<u8>>, PlanningError> {
    let tree = git_output(root, &["ls-tree", "-z", captured_sha, "--", relative_path])?;
    if tree.is_empty() {
        return Ok(None);
    }
    let Some(tab) = tree.iter().position(|byte| *byte == b'\t') else {
        return Err(PlanningError::Evidence(
            relative_path.into(),
            "invalid git tree entry".into(),
        ));
    };
    let header = std::str::from_utf8(&tree[..tab])
        .map_err(|error| PlanningError::Evidence(relative_path.into(), error.to_string()))?;
    let mode = header.split_ascii_whitespace().next().unwrap_or_default();
    if !matches!(mode, "100644" | "100755") {
        return Err(PlanningError::Evidence(
            relative_path.into(),
            "must be a regular file in the captured commit".into(),
        ));
    }
    let object = format!("{captured_sha}:{relative_path}");
    let size = git_output(root, &["cat-file", "-s", &object])?;
    let size = std::str::from_utf8(&size)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .ok_or_else(|| PlanningError::Evidence(relative_path.into(), "invalid blob size".into()))?;
    if size > MAX_EVIDENCE_BYTES {
        return Err(PlanningError::Evidence(
            relative_path.into(),
            "file exceeds the evidence size limit".into(),
        ));
    }
    let content = git_output(root, &["cat-file", "blob", &object])?;
    if content.len() as u64 != size {
        return Err(PlanningError::Evidence(
            relative_path.into(),
            "blob size changed while assessing".into(),
        ));
    }
    Ok(Some(content))
}

fn git_output(root: &Path, arguments: &[&str]) -> Result<Vec<u8>, PlanningError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .map_err(|error| PlanningError::Repository(error.to_string()))?;
    if !output.status.success() {
        return Err(PlanningError::Repository(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(output.stdout)
}

fn contract_intent(task: &Task) -> Result<(String, Vec<String>, Vec<Capability>), PlanningError> {
    let repository = task
        .source
        .repository_full_name()
        .ok_or(PlanningError::UnsupportedSource)?;
    let number = task
        .source
        .item_number()
        .ok_or(PlanningError::UnsupportedSource)?;
    match &task.source {
        TaskSource::GitHubIssue(_) => Ok((
            format!(
                "Resolve GitHub issue #{number} in {repository}: {}",
                task.title
            ),
            vec![
                "The issue outcome is implemented and every captured verification command passes."
                    .into(),
            ],
            vec![
                Capability::CreateBranch,
                Capability::PushBranch,
                Capability::CreatePullRequest,
                Capability::PostComment,
                Capability::CreateCheckRun,
                Capability::CloseIssue,
            ],
        )),
        TaskSource::GitHubPullRequest(_) => Ok((
            format!(
                "Complete GitHub pull request #{number} in {repository}: {}",
                task.title
            ),
            vec![
                "Requested changes are complete and every captured verification command passes."
                    .into(),
            ],
            vec![
                Capability::PushBranch,
                Capability::PostComment,
                Capability::PostReview,
                Capability::ResolveThread,
                Capability::CreateCheckRun,
                Capability::UpdatePullRequestBranch,
                Capability::ReadyPullRequest,
                Capability::ClosePullRequest,
                Capability::EnqueuePullRequest,
                Capability::MergePullRequest,
            ],
        )),
        TaskSource::LocalRequest => Err(PlanningError::UnsupportedSource),
    }
}

fn digest_evidence(evidence: &[(&str, Vec<u8>)]) -> String {
    let mut hasher = Sha256::new();
    for (name, content) in evidence {
        hasher.update(name.len().to_be_bytes());
        hasher.update(name.as_bytes());
        hasher.update(content.len().to_be_bytes());
        hasher.update(content);
    }
    hex(hasher.finalize())
}

fn digest(value: &[u8]) -> String {
    hex(Sha256::digest(value))
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().fold(
        String::with_capacity(bytes.as_ref().len() * 2),
        |mut output, byte| {
            write!(&mut output, "{byte:02x}").expect("writing to String is infallible");
            output
        },
    )
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum PlanningError {
    #[error("task repository binding does not match")]
    BindingMismatch,
    #[error("task has no captured source SHA")]
    SourceShaMissing,
    #[error("task source is unsupported")]
    UnsupportedSource,
    #[error("unsupported repository: no trusted manifest and lockfile pair was found")]
    UnsupportedRepository,
    #[error("repository evidence is incomplete: {manifest} requires {lockfile}")]
    IncompleteEvidence {
        manifest: &'static str,
        lockfile: &'static str,
    },
    #[error("repository has uncommitted or untracked changes")]
    RepositoryDirty,
    #[error("repository HEAD differs from the captured source commit")]
    SourceChanged,
    #[error("repository inspection failed: {0}")]
    Repository(String),
    #[error("repository evidence {0} is invalid: {1}")]
    Evidence(String, String),
    #[error("planned contract is invalid: {0}")]
    Contract(String),
}
