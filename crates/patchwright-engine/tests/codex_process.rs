#[path = "support/fake_codex_app_server.rs"]
mod fake_codex_app_server;

use fake_codex_app_server::FakeCodexAppServer;
use patchwright_engine::codex::process::{
    CodexExecutable, CodexProcessConfig, CodexProcessError, CodexProcessFactory, CodexProcessState,
    VersionCompatibility,
};
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;
use tempfile::tempdir;

#[tokio::test]
async fn discovers_the_exact_executable_and_reports_version_compatibility() {
    let root = tempdir().unwrap();
    let compatible = FakeCodexAppServer::create(root.path(), "codex-cli 0.144.2", "exit 0");
    let executable = CodexExecutable::discover(Some(compatible.path()))
        .await
        .unwrap();
    assert_eq!(executable.path(), compatible.path().canonicalize().unwrap());
    assert_eq!(executable.version(), "codex-cli 0.144.2");
    assert_eq!(
        executable.compatibility(),
        &VersionCompatibility::Compatible
    );

    let mismatch_root = tempdir().unwrap();
    let mismatch = FakeCodexAppServer::create(mismatch_root.path(), "codex-cli 0.145.0", "exit 0");
    let executable = CodexExecutable::discover(Some(mismatch.path()))
        .await
        .unwrap();
    assert!(matches!(
        executable.compatibility(),
        VersionCompatibility::Warning { .. }
    ));
}

#[tokio::test]
async fn rejects_missing_or_non_executable_codex_paths() {
    let root = tempdir().unwrap();
    let missing = root.path().join("missing-codex");
    assert!(matches!(
        CodexExecutable::discover(Some(&missing)).await,
        Err(CodexProcessError::MissingExecutable(_))
    ));

    let regular_file = root.path().join("not-executable");
    std::fs::write(&regular_file, "codex-cli 0.144.2").unwrap();
    assert!(matches!(
        CodexExecutable::discover(Some(&regular_file)).await,
        Err(CodexProcessError::NotExecutable(_))
    ));
}

#[tokio::test]
async fn bounds_a_hung_version_probe_and_cleans_up_its_process_group() {
    let root = tempdir().unwrap();
    let executable = root.path().join("hung-codex");
    let descendant_path = root.path().join("descendant.pid");
    std::fs::write(
        &executable,
        format!(
            "#!/bin/sh\n(sleep 60) &\nprintf '%s' \"$!\" > '{}'\nwait\n",
            descendant_path.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();

    let started = std::time::Instant::now();
    let result = CodexExecutable::discover(Some(&executable)).await;

    assert!(matches!(
        result,
        Err(CodexProcessError::VersionProbeTimeout { .. })
    ));
    assert!(started.elapsed() < Duration::from_secs(3));
    let descendant_pid = std::fs::read_to_string(descendant_path)
        .unwrap()
        .parse::<i32>()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert!(!pid_exists(descendant_pid));
}

#[tokio::test]
async fn launches_independent_task_process_groups_in_their_worktrees_without_secrets() {
    let root = tempdir().unwrap();
    let fake = FakeCodexAppServer::create(
        root.path(),
        "codex-cli 0.144.2",
        r#"printf '{\"pid\":%s,\"cwd\":\"%s\",\"gh\":\"%s\",\"openai\":\"%s\"}\n' "$$" "$PWD" "${GH_TOKEN-unset}" "${OPENAI_API_KEY-unset}"
while IFS= read -r line; do printf '%s\n' "$line"; done"#,
    );
    let executable = CodexExecutable::discover(Some(fake.path())).await.unwrap();
    let factory = CodexProcessFactory::new(executable, CodexProcessConfig::default());
    let first_worktree = root.path().join("task-one");
    let second_worktree = root.path().join("task-two");
    std::fs::create_dir_all(&first_worktree).unwrap();
    std::fs::create_dir_all(&second_worktree).unwrap();

    let mut first = factory.launch("task-one", &first_worktree).unwrap();
    let mut second = factory.launch("task-two", &second_worktree).unwrap();
    assert_eq!(first.state(), CodexProcessState::Starting);
    let first_ready: Value =
        serde_json::from_str(&first.read_initialization_line().await.unwrap()).unwrap();
    let second_ready: Value =
        serde_json::from_str(&second.read_initialization_line().await.unwrap()).unwrap();
    assert_ne!(first_ready["pid"], second_ready["pid"]);
    assert_eq!(
        first_ready["cwd"],
        first_worktree.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(
        second_ready["cwd"],
        second_worktree.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(first_ready["gh"], "unset");
    assert_eq!(first_ready["openai"], "unset");
    assert_ne!(first.process_group_id(), second.process_group_id());

    first.mark_ready().unwrap();
    assert_eq!(first.state(), CodexProcessState::Ready);
    first.write_line(r#"{\"ping\":1}"#).await.unwrap();
    assert_eq!(first.read_line().await.unwrap(), r#"{\"ping\":1}"#);
    first.terminate().await.unwrap();
    second.terminate().await.unwrap();
    assert_eq!(first.state(), CodexProcessState::Exited);
    assert_eq!(second.state(), CodexProcessState::Exited);
}

#[tokio::test]
async fn bounds_stderr_reports_early_exit_and_times_out_hung_initialization() {
    let root = tempdir().unwrap();
    let noisy = FakeCodexAppServer::create(
        root.path(),
        "codex-cli 0.144.2",
        "i=0; while [ $i -lt 100 ]; do printf '0123456789' >&2; i=$((i + 1)); done; exit 23",
    );
    let executable = CodexExecutable::discover(Some(noisy.path())).await.unwrap();
    let config = CodexProcessConfig {
        stderr_capacity: 64,
        ..CodexProcessConfig::default()
    };
    let factory = CodexProcessFactory::new(executable, config);
    let mut process = factory.launch("noisy", root.path()).unwrap();
    let exit = process.wait_for_exit().await.unwrap();
    assert_eq!(exit.code, Some(23));
    assert_eq!(process.state(), CodexProcessState::Failed);
    let stderr = process.stderr_snapshot().await;
    assert!(stderr.len() <= 64);
    assert!(stderr.ends_with("0123456789"));

    let hung_root = tempdir().unwrap();
    let hung = FakeCodexAppServer::create(hung_root.path(), "codex-cli 0.144.2", "sleep 60");
    let executable = CodexExecutable::discover(Some(hung.path())).await.unwrap();
    let config = CodexProcessConfig {
        initialization_timeout: Duration::from_millis(50),
        shutdown_grace: Duration::from_millis(50),
        ..CodexProcessConfig::default()
    };
    let factory = CodexProcessFactory::new(executable, config);
    let mut process = factory.launch("hung", hung_root.path()).unwrap();
    assert!(matches!(
        process.read_initialization_line().await,
        Err(CodexProcessError::Timeout { .. })
    ));
    process.terminate().await.unwrap();
    assert_eq!(process.state(), CodexProcessState::Exited);
}

#[tokio::test]
async fn termination_cleans_up_the_owned_process_group() {
    let root = tempdir().unwrap();
    let fake = FakeCodexAppServer::create(
        root.path(),
        "codex-cli 0.144.2",
        "trap 'exit 0' TERM; (trap '' TERM; sleep 60) & child=$!; printf '%s\\n' \"$child\"; wait",
    );
    let executable = CodexExecutable::discover(Some(fake.path())).await.unwrap();
    let config = CodexProcessConfig {
        shutdown_grace: Duration::from_millis(100),
        ..CodexProcessConfig::default()
    };
    let factory = CodexProcessFactory::new(executable, config);
    let mut process = factory.launch("tree", root.path()).unwrap();
    let descendant_pid = process
        .read_initialization_line()
        .await
        .unwrap()
        .parse::<i32>()
        .unwrap();

    process.terminate().await.unwrap();
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert!(!pid_exists(descendant_pid));
}

fn pid_exists(pid: i32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
}
