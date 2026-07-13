use patchwright_core::InstructionKind;
use patchwright_engine::{CommandRunner, CommandSpec, RepositoryService, WorktreeService};
use std::{fs, process::Command, time::Duration};

fn git(repository: &std::path::Path, arguments: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .status()
        .unwrap();
    assert!(status.success(), "git {arguments:?} failed");
}

#[tokio::test]
async fn repository_to_isolated_worktree_preserves_base_and_uses_argv() {
    let fixture = tempfile::tempdir().unwrap();
    let repository = fixture.path().join("repository");
    fs::create_dir_all(repository.join("Sources/Feature")).unwrap();
    git(fixture.path(), &["init", repository.to_str().unwrap()]);
    fs::write(
        repository.join("AGENTS.md"),
        "network: deny\nrun root tests",
    )
    .unwrap();
    fs::write(repository.join("Sources/AGENTS.md"), "run feature tests").unwrap();
    fs::write(
        repository.join("Sources/Feature/file.swift"),
        "let value = 1\n",
    )
    .unwrap();
    git(&repository, &["add", "."]);
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

    let inspection = RepositoryService::inspect(&repository).unwrap();
    assert!(!inspection.dirty);
    let instructions = RepositoryService::resolve_instructions(
        &repository,
        &[repository.join("Sources/Feature/file.swift")],
    )
    .unwrap();
    assert_eq!(
        instructions
            .sources
            .iter()
            .map(|source| source.kind)
            .collect::<Vec<_>>(),
        vec![
            InstructionKind::RootAgents,
            InstructionKind::DirectoryAgents
        ]
    );

    let worktree = fixture.path().join("worktree");
    WorktreeService::prepare(&repository, &worktree, "agent/test-task").unwrap();
    assert!(worktree.join("Sources/Feature/file.swift").exists());
    assert!(!RepositoryService::inspect(&repository).unwrap().dirty);

    let injection_marker = fixture.path().join("injected");
    let output = CommandRunner::run(CommandSpec {
        executable: "/usr/bin/printf".into(),
        arguments: vec![
            "%s".into(),
            format!("$(touch {})", injection_marker.display()),
        ],
        working_directory: worktree,
        timeout: Duration::from_secs(2),
        environment: vec![],
    })
    .await
    .unwrap();
    assert!(output.success);
    assert!(output.stdout.contains("$(touch"));
    assert!(!injection_marker.exists());
}
