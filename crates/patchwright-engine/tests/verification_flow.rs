use patchwright_core::{
    Capability, CredentialHealth, RepositoryBinding, RepositoryBindingDraft,
    RepositoryPermissionSnapshot, RiskClass, Task, TaskContract, TaskContractDraft, TaskSource,
    TaskState, VerificationCommand,
};
use patchwright_engine::{EventStore, VerificationError, verify_task_for_delivery};
use std::{os::unix::fs::PermissionsExt, process::Command, sync::Mutex};
use tempfile::TempDir;

#[tokio::test]
async fn verification_runs_commands_in_order_and_persists_only_bounded_digests() {
    let fixture = Fixture::new();
    let marker = fixture.root.path().join("order.txt");
    let first = fixture.script(
        "first",
        &format!("printf 'first\\n' >> '{}'", marker.display()),
    );
    let second = fixture.script(
        "second",
        &format!("printf 'second\\n' >> '{}'", marker.display()),
    );
    let (task, store) = fixture.task_with_commands(vec![
        VerificationCommand::new(first.to_string_lossy(), Vec::<String>::new()).unwrap(),
        VerificationCommand::new(second.to_string_lossy(), Vec::<String>::new()).unwrap(),
    ]);

    let verified = verify_task_for_delivery(task.id, &store).await.unwrap();

    assert_eq!(verified.state, TaskState::AwaitingDeliveryApproval);
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "first\nsecond\n");
    let evidence = store
        .lock()
        .unwrap()
        .verification_evidence(task.id)
        .unwrap();
    assert_eq!(evidence.len(), 2);
    assert_eq!(evidence[0].ordinal, 1);
    assert_eq!(evidence[1].ordinal, 2);
    assert!(evidence.iter().all(|item| item.success));
    let encoded = serde_json::to_string(&evidence).unwrap();
    assert!(!encoded.contains("first\\nsecond"));
}

#[tokio::test]
async fn failed_command_keeps_task_verifying_and_records_failure_evidence() {
    let fixture = Fixture::new();
    let failing = fixture.script("fail", "printf 'private output'; exit 7");
    let (task, store) = fixture.task_with_commands(vec![
        VerificationCommand::new(failing.to_string_lossy(), Vec::<String>::new()).unwrap(),
    ]);

    let error = verify_task_for_delivery(task.id, &store).await.unwrap_err();

    assert!(matches!(
        error,
        VerificationError::CommandFailed { ordinal: 1 }
    ));
    assert_eq!(
        store
            .lock()
            .unwrap()
            .load_task(task.id)
            .unwrap()
            .unwrap()
            .state,
        TaskState::Verifying
    );
    let evidence = store
        .lock()
        .unwrap()
        .verification_evidence(task.id)
        .unwrap();
    assert_eq!(evidence.len(), 1);
    assert!(!evidence[0].success);
    assert_eq!(evidence[0].exit_code, Some(7));
    assert!(
        !serde_json::to_string(&evidence)
            .unwrap()
            .contains("private output")
    );
}

#[tokio::test]
async fn worktree_change_during_verification_invalidates_the_result() {
    let fixture = Fixture::new();
    let mutating = fixture.script("mutate", "printf 'changed' >> README.md");
    let (task, store) = fixture.task_with_commands(vec![
        VerificationCommand::new(mutating.to_string_lossy(), Vec::<String>::new()).unwrap(),
    ]);

    let error = verify_task_for_delivery(task.id, &store).await.unwrap_err();

    assert!(matches!(error, VerificationError::StaleResult));
    assert_eq!(
        store
            .lock()
            .unwrap()
            .load_task(task.id)
            .unwrap()
            .unwrap()
            .state,
        TaskState::Verifying
    );
}

struct Fixture {
    root: TempDir,
    repository: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repository");
        std::fs::create_dir(&repository).unwrap();
        git(&repository, &["init", "-b", "main"]);
        std::fs::write(repository.join("README.md"), "fixture\n").unwrap();
        git(&repository, &["add", "README.md"]);
        git(
            &repository,
            &[
                "-c",
                "user.name=Patchwright Test",
                "-c",
                "user.email=test@patchwright.local",
                "commit",
                "-m",
                "fixture",
            ],
        );
        Self { root, repository }
    }

    fn script(&self, name: &str, body: &str) -> std::path::PathBuf {
        let path = self.root.path().join(name);
        std::fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn task_with_commands(&self, commands: Vec<VerificationCommand>) -> (Task, Mutex<EventStore>) {
        let binding = RepositoryBinding::try_from(RepositoryBindingDraft {
            github_repository_id: 42,
            full_name: "octocat/hello".into(),
            installation_id: 84,
            clone_url: "https://github.com/octocat/hello.git".into(),
            html_url: "https://github.com/octocat/hello".into(),
            default_branch: "main".into(),
            user_checkout: Some(self.repository.to_string_lossy().into_owned()),
            managed_clone: None,
            state_root: self
                .root
                .path()
                .join("state")
                .to_string_lossy()
                .into_owned(),
            worktree_root: self
                .root
                .path()
                .join("worktrees")
                .to_string_lossy()
                .into_owned(),
            default_branch_sha: Some(git(&self.repository, &["rev-parse", "HEAD"])),
            default_branch_committed_at: Some(chrono::Utc::now()),
            permissions: RepositoryPermissionSnapshot::read_only(),
            credential_health: CredentialHealth::Healthy,
        })
        .unwrap();
        let mut task = Task::new(
            "Verify fixture",
            self.repository.to_string_lossy().into_owned(),
        )
        .unwrap();
        for state in [
            TaskState::Assessing,
            TaskState::Planned,
            TaskState::AwaitingPreparationApproval,
            TaskState::Preparing,
            TaskState::Implementing,
            TaskState::Verifying,
        ] {
            task.transition(state).unwrap();
        }
        task.repository_binding_id = Some(binding.id());
        task.contract_version = 1;
        let contract = TaskContract::try_from(TaskContractDraft {
            task_id: task.id,
            source: TaskSource::LocalRequest,
            repository_binding_id: binding.id(),
            goal: "Prove the contract".into(),
            acceptance_criteria: vec!["All commands pass".into()],
            base_sha: None,
            head_sha: Some(git(&self.repository, &["rev-parse", "HEAD"])),
            source_sha256: "b".repeat(64),
            repository_sha256: "c".repeat(64),
            instruction_digests: vec![],
            verification_commands: commands,
            required_capabilities: vec![Capability::PushBranch],
            risk: RiskClass::Moderate,
            sensitive_paths: vec![],
            dependencies: vec![],
        })
        .unwrap();
        let store = EventStore::open(&self.root.path().join("events.sqlite")).unwrap();
        store.save_repository_binding(&binding).unwrap();
        store.save_task(&task, "ready to verify").unwrap();
        store.save_task_contract(&contract).unwrap();
        (task, Mutex::new(store))
    }
}

fn git(repository: &std::path::Path, arguments: &[&str]) -> String {
    let output = Command::new("/usr/bin/git")
        .args(arguments)
        .current_dir(repository)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
