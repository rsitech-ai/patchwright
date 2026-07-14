use anyhow::{Context, Result, bail};
use std::{fs::OpenOptions, io::Write, path::Path, process::Command};

const ASKPASS_SCRIPT: &str = r#"#!/bin/sh
case "$1" in
  *Username*) exec /usr/bin/printf '%s\n' "$PATCHWRIGHT_GIT_USERNAME" ;;
  *) exec /usr/bin/printf '%s\n' "$PATCHWRIGHT_GIT_TOKEN" ;;
esac
"#;

pub struct WorktreeService;

pub struct GitTransport;

impl GitTransport {
    pub fn clone_repository(
        clone_url: &str,
        destination: &Path,
        state_root: &Path,
        token: &str,
    ) -> Result<()> {
        if token.is_empty() {
            bail!("GitHub installation token is empty");
        }
        if destination.exists() {
            bail!("managed clone destination already exists");
        }
        let parent = destination
            .parent()
            .context("managed clone destination has no parent")?;
        std::fs::create_dir_all(parent).context("create managed clone parent")?;
        let askpass = prepare_askpass(state_root)?;
        let output = Command::new("git")
            .args(["clone", "--no-checkout", "--origin", "origin"])
            .arg(clone_url)
            .arg(destination)
            .env("GIT_ASKPASS", &askpass)
            .env("GIT_ASKPASS_REQUIRE", "force")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("PATCHWRIGHT_GIT_USERNAME", "x-access-token")
            .env("PATCHWRIGHT_GIT_TOKEN", token)
            .output()
            .context("clone managed repository")?;
        if !output.status.success() {
            if destination.exists() {
                let _ = std::fs::remove_dir_all(destination);
            }
            bail!(
                "git clone failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    pub fn push_branch(
        repository: &Path,
        branch: &str,
        expected_head_sha: &str,
        state_root: &Path,
        token: &str,
    ) -> Result<()> {
        if branch.trim().is_empty()
            || branch.starts_with('-')
            || branch.contains(char::is_whitespace)
        {
            bail!("invalid push branch name");
        }
        if expected_head_sha.len() != 40
            || !expected_head_sha
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            bail!("invalid expected push head");
        }
        let inspection = crate::RepositoryService::inspect(repository)?;
        if inspection.dirty {
            bail!("task worktree has uncommitted changes");
        }
        if inspection.head_sha != expected_head_sha {
            bail!("task worktree head changed before push");
        }
        let askpass = prepare_askpass(state_root)?;
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(["push", "origin"])
            .arg(format!("HEAD:refs/heads/{branch}"))
            .env("GIT_ASKPASS", &askpass)
            .env("GIT_ASKPASS_REQUIRE", "force")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("PATCHWRIGHT_GIT_USERNAME", "x-access-token")
            .env("PATCHWRIGHT_GIT_TOKEN", token)
            .output()
            .context("push task branch")?;
        if !output.status.success() {
            bail!(
                "git push failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }
}

fn prepare_askpass(state_root: &Path) -> Result<std::path::PathBuf> {
    std::fs::create_dir_all(state_root).context("create repository state root")?;
    let path = state_root.join("git-askpass.sh");
    if path.exists() {
        let metadata = std::fs::symlink_metadata(&path).context("inspect Git askpass helper")?;
        if metadata.file_type().is_symlink()
            || std::fs::read_to_string(&path).context("read Git askpass helper")? != ASKPASS_SCRIPT
        {
            bail!("Git askpass helper is not trusted");
        }
    } else {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .context("create Git askpass helper")?;
        file.write_all(ASKPASS_SCRIPT.as_bytes())
            .context("write Git askpass helper")?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .context("restrict Git askpass helper")?;
    }
    Ok(path)
}

impl WorktreeService {
    pub fn prepare(repository: &Path, destination: &Path, branch: &str) -> Result<()> {
        Self::prepare_at(repository, destination, branch, None)
    }

    pub fn prepare_at(
        repository: &Path,
        destination: &Path,
        branch: &str,
        start_point: Option<&str>,
    ) -> Result<()> {
        if branch.trim().is_empty()
            || branch.starts_with('-')
            || branch.contains(char::is_whitespace)
        {
            bail!("invalid worktree branch name");
        }
        if destination.exists() {
            bail!(
                "worktree destination already exists: {}",
                destination.display()
            );
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).context("create worktree parent")?;
        }
        if let Some(start_point) = start_point
            && (start_point.len() != 40
                || !start_point.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            bail!("invalid worktree start point");
        }
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(repository)
            .args(["worktree", "add", "-b", branch])
            .arg(destination);
        if let Some(start_point) = start_point {
            command.arg(start_point);
        }
        let output = command.output().context("create git worktree")?;
        if !output.status.success() {
            bail!(
                "git worktree add failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }
}
