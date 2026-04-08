use fp_appimage_updater::lock::FileLock;
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "fp-appimage-updater-{name}-{}-{nanos}",
        std::process::id()
    ))
}

fn write_lock_file(path: &Path, pid: u32, boot_id: &str) {
    fs::write(path, format!("pid={pid}\nboot_id={boot_id}\n"))
        .expect("failed to write test lockfile");
}

fn set_mtime(path: &Path, modified: SystemTime) {
    let modified = modified
        .duration_since(UNIX_EPOCH)
        .expect("invalid modified time");
    let ts = libc::timespec {
        tv_sec: modified.as_secs() as libc::time_t,
        tv_nsec: modified.subsec_nanos() as libc::c_long,
    };
    let times = [ts, ts];
    let c_path = CString::new(path.as_os_str().as_bytes()).expect("path contains null byte");

    let result = unsafe { libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), 0) };
    assert_eq!(result, 0, "failed to set mtime for {:?}", path);
}

fn current_boot_time() -> SystemTime {
    let stat = fs::read_to_string("/proc/stat").expect("failed to read system boot time");
    let btime = stat
        .lines()
        .find_map(|line| line.strip_prefix("btime "))
        .expect("failed to find system boot time")
        .parse::<u64>()
        .expect("failed to parse system boot time");

    UNIX_EPOCH + Duration::from_secs(btime)
}

#[test]
fn stale_lock_from_previous_boot_is_removed() {
    let dir = unique_temp_dir("stale-lock");
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    let lock_path = dir.join("process.lock");
    let current_pid = std::process::id();
    write_lock_file(
        &lock_path,
        current_pid,
        "00000000-0000-0000-0000-000000000000",
    );

    let lock = FileLock::acquire(&lock_path).expect("stale lock should be cleaned up");

    let content = fs::read_to_string(&lock_path).expect("failed to read new lockfile");
    assert!(content.contains(&format!("pid={current_pid}")));
    assert!(content.contains("boot_id="));

    drop(lock);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn live_lock_same_boot_blocks_acquisition() {
    let dir = unique_temp_dir("live-lock");
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    let lock_path = dir.join("process.lock");
    let current_pid = std::process::id();
    let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .expect("failed to read current boot id");
    write_lock_file(&lock_path, current_pid, boot_id.trim());

    let err = FileLock::acquire(&lock_path).unwrap_err();
    assert!(err.to_string().contains("already running"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn legacy_empty_lock_before_boot_is_removed() {
    let dir = unique_temp_dir("legacy-lock");
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    let lock_path = dir.join("process.lock");
    fs::write(&lock_path, "").expect("failed to create legacy lockfile");

    let boot_time = current_boot_time();
    set_mtime(&lock_path, boot_time - Duration::from_secs(60));

    let lock = FileLock::acquire(&lock_path).expect("legacy stale lock should be cleaned up");
    drop(lock);

    let _ = fs::remove_dir_all(&dir);
}
