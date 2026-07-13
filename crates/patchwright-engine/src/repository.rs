use anyhow::{Context, Result, bail};
use patchwright_core::{
    EffectiveInstructions, InstructionKind, InstructionResolver, InstructionSource,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryInspection {
    pub root: PathBuf,
    pub branch: String,
    pub head_sha: String,
    pub dirty: bool,
}

pub struct RepositoryService;

impl RepositoryService {
    pub fn inspect(path: &Path) -> Result<RepositoryInspection> {
        let root_output = git(path, &["rev-parse", "--show-toplevel"])?;
        let root = PathBuf::from(root_output.trim())
            .canonicalize()
            .context("canonicalize repository root")?;
        let branch = git(&root, &["branch", "--show-current"])?.trim().to_owned();
        let head_sha = git(&root, &["rev-parse", "HEAD"])?.trim().to_owned();
        let dirty = !git(&root, &["status", "--porcelain=v1"])?.trim().is_empty();
        Ok(RepositoryInspection {
            root,
            branch,
            head_sha,
            dirty,
        })
    }

    pub fn resolve_instructions(
        root: &Path,
        changed_paths: &[PathBuf],
    ) -> Result<EffectiveInstructions> {
        let root = root
            .canonicalize()
            .context("canonicalize instruction root")?;
        let mut candidates = BTreeSet::new();
        let root_agents = root.join("AGENTS.md");
        if root_agents.is_file() {
            candidates.insert(root_agents);
        }
        for changed_path in changed_paths {
            let changed_path = changed_path
                .canonicalize()
                .with_context(|| format!("canonicalize {}", changed_path.display()))?;
            if !changed_path.starts_with(&root) {
                bail!(
                    "changed path escapes repository: {}",
                    changed_path.display()
                );
            }
            let mut directory = changed_path.parent();
            while let Some(current) = directory {
                if !current.starts_with(&root) {
                    break;
                }
                let candidate = current.join("AGENTS.md");
                if candidate.is_file() {
                    candidates.insert(candidate);
                }
                if current == root {
                    break;
                }
                directory = current.parent();
            }
        }
        let mut sources = Vec::with_capacity(candidates.len());
        for path in candidates {
            let kind = if path == root.join("AGENTS.md") {
                InstructionKind::RootAgents
            } else {
                InstructionKind::DirectoryAgents
            };
            sources.push(InstructionSource::new(
                kind,
                path.strip_prefix(&root)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
                std::fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?,
            ));
        }
        Ok(InstructionResolver::resolve(sources))
    }
}

fn git(repository: &Path, arguments: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .output()
        .with_context(|| format!("run git {arguments:?}"))?;
    if !output.status.success() {
        bail!(
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout).context("git returned non-UTF-8 output")
}
