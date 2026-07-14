use patchwright_core::InstructionKind;
use patchwright_engine::{
    CommandRunner, CommandSpec, GitTransport, RepositoryService, WorktreeService,
};
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

fn git_output(repository: &std::path::Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?} failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn ephemeral_transport_pushes_only_the_exact_checked_out_head() {
    let fixture = tempfile::tempdir().unwrap();
    let remote = fixture.path().join("remote.git");
    let repository = fixture.path().join("repository");
    fs::create_dir_all(&repository).unwrap();
    git(
        fixture.path(),
        &["init", "--bare", remote.to_str().unwrap()],
    );
    git(
        fixture.path(),
        &["init", "-b", "main", repository.to_str().unwrap()],
    );
    fs::write(repository.join("README.md"), "transport\n").unwrap();
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
    git(
        &repository,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    let head = RepositoryService::inspect(&repository).unwrap().head_sha;

    GitTransport::push_branch(
        &repository,
        "patchwright/test-task",
        &head,
        fixture.path().join("state").as_path(),
        "fixture-token",
    )
    .unwrap();

    let pushed = git_output(
        fixture.path(),
        &[
            "--git-dir",
            remote.to_str().unwrap(),
            "rev-parse",
            "refs/heads/patchwright/test-task",
        ],
    );
    assert_eq!(pushed, head);
    assert!(
        GitTransport::push_branch(
            &repository,
            "patchwright/test-task",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            fixture.path().join("state").as_path(),
            "fixture-token",
        )
        .is_err()
    );
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
