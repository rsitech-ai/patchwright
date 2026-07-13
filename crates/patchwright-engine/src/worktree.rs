use anyhow::{Context, Result, bail};
use std::{path::Path, process::Command};

pub struct WorktreeService;

impl WorktreeService {
    pub fn prepare(repository: &Path, destination: &Path, branch: &str) -> Result<()> {
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
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(["worktree", "add"])
            .arg(destination)
            .args(["-b", branch])
            .output()
            .context("create git worktree")?;
        if !output.status.success() {
            bail!(
                "git worktree add failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }
}
