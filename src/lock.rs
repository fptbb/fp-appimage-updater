use anyhow::{Context, Result, bail};
use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub struct FileLock {
    path: PathBuf,
}

impl FileLock {
    pub fn acquire(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create lock directory: {}", parent.display())
            })?;
        }

        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => Ok(Self { path }),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                bail!(
                    "Another fp-appimage-updater process is already running (lock file exists at {}).",
                    path.display()
                );
            }
            Err(err) => Err(err)
                .with_context(|| format!("Failed to acquire process lock at {}", path.display())),
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
