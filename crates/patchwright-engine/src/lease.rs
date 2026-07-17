use anyhow::{Context, Result, bail};
use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};
use std::fs::{File, OpenOptions, Permissions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// An operating-system-owned lease that prevents two engines from recovering
/// and mutating the same event database at the same time.
pub(crate) struct DatabaseLease {
    #[allow(dead_code)]
    lock: Flock<File>,
}

impl DatabaseLease {
    pub(crate) fn acquire(database_path: &Path) -> Result<Self> {
        let lock_path = lock_path(database_path);
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(&lock_path)
            .with_context(|| format!("open database lease {}", lock_path.display()))?;
        std::fs::set_permissions(&lock_path, Permissions::from_mode(0o600))
            .context("restrict database lease permissions")?;
        let lock = match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
            Ok(lock) => lock,
            Err((_file, Errno::EAGAIN)) => {
                bail!("database is already in use by another Patchwright engine")
            }
            Err((_file, error)) => return Err(error).context("acquire database lease"),
        };
        Ok(Self { lock })
    }
}

fn lock_path(database_path: &Path) -> PathBuf {
    let mut value = database_path.as_os_str().to_owned();
    value.push(".lock");
    PathBuf::from(value)
}
