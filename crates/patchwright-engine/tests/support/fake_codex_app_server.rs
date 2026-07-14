use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct FakeCodexAppServer {
    path: PathBuf,
}

impl FakeCodexAppServer {
    pub fn create(root: &Path, version: &str, body: &str) -> Self {
        let path = root.join("codex");
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf '%s\\n' '{version}'\n  exit 0\nfi\nif [ \"$1\" != \"app-server\" ]; then\n  exit 64\nfi\n{body}\n"
        );
        fs::write(&path, script).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).unwrap();
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
