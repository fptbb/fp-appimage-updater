use anyhow::{Context, Result, bail};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub struct FileLock {
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockState {
    Missing,
    Active { pid: u32 },
    Stale { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LockMetadata {
    pid: u32,
    boot_id: String,
}

impl LockMetadata {
    fn current() -> Result<Self> {
        Ok(Self {
            pid: std::process::id(),
            boot_id: current_boot_id()?,
        })
    }

    fn serialize(&self) -> String {
        format!("pid={}\nboot_id={}\n", self.pid, self.boot_id)
    }

    fn parse(content: &str) -> Result<Self> {
        let mut pid = None;
        let mut boot_id = None;

        for line in content.lines() {
            if let Some(value) = line.strip_prefix("pid=") {
                pid = Some(
                    value
                        .trim()
                        .parse::<u32>()
                        .with_context(|| format!("Invalid pid value in lockfile: {value}"))?,
                );
            } else if let Some(value) = line.strip_prefix("boot_id=") {
                let value = value.trim();
                if !value.is_empty() {
                    boot_id = Some(value.to_string());
                }
            }
        }

        Ok(Self {
            pid: pid.context("Missing pid in lockfile")?,
            boot_id: boot_id.context("Missing boot_id in lockfile")?,
        })
    }
}

impl FileLock {
    pub fn acquire(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create lock directory: {}", parent.display())
            })?;
        }

        let current = LockMetadata::current()?;
        loop {
            match Self::inspect(&path)? {
                LockState::Missing | LockState::Stale { .. } => {
                    let _ = fs::remove_file(&path);
                    match Self::install_lock(&path, &current) {
                        Ok(()) => return Ok(Self { path }),
                        Err(LockAcquireError::Occupied) => continue,
                        Err(LockAcquireError::Io(e)) => {
                            return Err(e).with_context(|| {
                                format!("Failed to acquire process lock at {}", path.display())
                            });
                        }
                    }
                }
                LockState::Active { .. } => {
                    bail!(
                        "Another fp-appimage-updater process is already running (lock file exists at {}).",
                        path.display()
                    );
                }
            }
        }
    }

    pub fn inspect(path: impl AsRef<Path>) -> Result<LockState> {
        let path = path.as_ref();

        if !path.exists() {
            return Ok(LockState::Missing);
        }

        let boot_id = current_boot_id()?;
        let boot_time = current_boot_time()?;

        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(LockState::Missing),
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("Failed to read process lock at {}", path.display()));
            }
        };

        match LockMetadata::parse(&content) {
            Ok(metadata) => {
                if metadata.boot_id != boot_id {
                    return Ok(LockState::Stale {
                        reason: format!(
                            "lockfile belongs to a previous boot (pid {})",
                            metadata.pid
                        ),
                    });
                }

                if metadata.pid == 0 {
                    return Ok(LockState::Stale {
                        reason: "lockfile contains an invalid pid".to_string(),
                    });
                }

                if process_is_running(metadata.pid) {
                    Ok(LockState::Active { pid: metadata.pid })
                } else {
                    Ok(LockState::Stale {
                        reason: format!("pid {} is no longer running", metadata.pid),
                    })
                }
            }
            Err(_) => {
                let metadata = match fs::metadata(path) {
                    Ok(metadata) => metadata,
                    Err(err) if err.kind() == ErrorKind::NotFound => return Ok(LockState::Missing),
                    Err(err) => {
                        return Err(err).with_context(|| {
                            format!("Failed to stat process lock at {}", path.display())
                        });
                    }
                };

                let modified = metadata.modified().with_context(|| {
                    format!(
                        "Failed to read process lock timestamp at {}",
                        path.display()
                    )
                })?;

                if modified < boot_time {
                    Ok(LockState::Stale {
                        reason: "legacy lockfile from before the current boot".to_string(),
                    })
                } else {
                    Ok(LockState::Stale {
                        reason: "lockfile has an invalid format".to_string(),
                    })
                }
            }
        }
    }

    fn install_lock(path: &Path, current: &LockMetadata) -> Result<(), LockAcquireError> {
        let temp_path = temp_lock_path(path);
        let write_result = (|| -> Result<()> {
            let mut temp = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
                .with_context(|| {
                    format!(
                        "Failed to create temporary lockfile: {}",
                        temp_path.display()
                    )
                })?;
            temp.write_all(current.serialize().as_bytes())
                .with_context(|| {
                    format!(
                        "Failed to write temporary lockfile: {}",
                        temp_path.display()
                    )
                })?;
            temp.sync_all().with_context(|| {
                format!(
                    "Failed to flush temporary lockfile: {}",
                    temp_path.display()
                )
            })?;
            Ok(())
        })();

        if let Err(e) = write_result {
            let _ = fs::remove_file(&temp_path);
            return Err(LockAcquireError::Io(e));
        }

        match fs::hard_link(&temp_path, path) {
            Ok(()) => {
                let _ = fs::remove_file(&temp_path);
                Ok(())
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&temp_path);
                Err(LockAcquireError::Occupied)
            }
            Err(e) => {
                let _ = fs::remove_file(&temp_path);
                Err(LockAcquireError::Io(anyhow::Error::new(e).context(
                    format!("Failed to install lockfile at {}", path.display()),
                )))
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug)]
enum LockAcquireError {
    Occupied,
    Io(anyhow::Error),
}

fn temp_lock_path(path: &Path) -> PathBuf {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "process.lock".to_string());

    path.with_file_name(format!(".{name}.{pid}.{nanos}.tmp"))
}

fn current_boot_id() -> Result<String> {
    let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .context("Failed to read system boot id")?;
    Ok(boot_id.trim().to_string())
}

fn current_boot_time() -> Result<SystemTime> {
    let stat = fs::read_to_string("/proc/stat").context("Failed to read system boot time")?;
    let btime = stat
        .lines()
        .find_map(|line| line.strip_prefix("btime "))
        .context("Failed to find system boot time")?
        .parse::<u64>()
        .context("Failed to parse system boot time")?;

    Ok(UNIX_EPOCH + Duration::from_secs(btime))
}

fn process_is_running(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }

    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}
