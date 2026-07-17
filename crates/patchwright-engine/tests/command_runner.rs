use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use patchwright_engine::{CommandRunner, CommandSpec};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[tokio::test]
async fn trusted_toolchain_environment_runs_the_resolved_cargo() {
    let directory = tempfile::tempdir().unwrap();
    let executable = CommandRunner::resolve_executable("cargo").unwrap();
    let output = CommandRunner::run(CommandSpec {
        executable,
        arguments: vec!["--version".into()],
        working_directory: directory.path().to_path_buf(),
        timeout: Duration::from_secs(10),
    })
    .await
    .unwrap();

    assert!(output.success, "cargo failed: {}", output.stderr);
    assert!(
        output.stdout.starts_with("cargo "),
        "unexpected cargo output; stdout={:?} stderr={:?}",
        output.stdout,
        output.stderr
    );
}

#[tokio::test]
async fn timeout_covers_pipe_drain_and_kills_inherited_pipe_descendants() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("inherited-pipes");
    let process_group_path = directory.path().join("process-group.pid");
    let descendant_path = directory.path().join("descendant.pid");
    fs::write(
        &executable,
        format!(
            "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\n(sleep 60) &\nprintf '%s' \"$!\" > '{}'\nexit 0\n",
            process_group_path.display(),
            descendant_path.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();

    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        CommandRunner::run(CommandSpec {
            executable,
            arguments: Vec::new(),
            working_directory: directory.path().to_path_buf(),
            timeout: Duration::from_millis(500),
        }),
    )
    .await;

    let process_group = fs::read_to_string(&process_group_path)
        .ok()
        .and_then(|value| value.parse::<i32>().ok());
    if result.is_err()
        && let Some(process_group) = process_group
    {
        let _ = killpg(Pid::from_raw(process_group), Signal::SIGKILL);
    }

    assert!(
        matches!(result, Ok(Err(ref error)) if error.to_string().contains("command timed out after 500 ms")),
        "the command runner did not apply its timeout to inherited pipe drain: {result:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(2));

    let descendant = read_pid(&descendant_path);
    for _ in 0..20 {
        if !pid_exists(descendant) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("descendant {descendant} survived the command timeout");
}

fn read_pid(path: &std::path::Path) -> i32 {
    fs::read_to_string(path).unwrap().parse::<i32>().unwrap()
}

fn pid_exists(pid: i32) -> bool {
    Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
