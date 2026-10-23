use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct Paths {
    pub runtime: PathBuf,
    pub state: PathBuf,
    pub last: PathBuf,
    pub lock: PathBuf,
    pub log: PathBuf,
    pub pid_file: PathBuf,
    pub socket: PathBuf,
    pub starting: PathBuf,
    pub cancel: PathBuf,
    pub webcam_pid: PathBuf,
    pub config_dir: PathBuf,
    pub config: PathBuf,
    pub previews: PathBuf,
}

impl Paths {
    pub fn discover() -> Paths {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));

        let runtime = match std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
            Some(dir) if dir.is_dir() => dir.join("reel"),
            _ => {
                let state_home = std::env::var_os("XDG_STATE_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".local/state"));
                state_home.join("omarchy/reel/run")
            }
        };

        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));

        let config_dir = config_home.join("omarchy");

        let cache_home = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"));
        let previews = cache_home.join("reel/previews");

        let paths = Paths {
            state: runtime.join("state.json"),
            last: runtime.join("last.json"),
            lock: runtime.join("lock"),
            log: runtime.join("reel.log"),
            pid_file: runtime.join("recorder.pid"),
            socket: runtime.join("recorder.sock"),
            starting: runtime.join("starting"),
            cancel: runtime.join("cancel"),
            webcam_pid: runtime.join("webcam.pid"),
            config: config_dir.join("reel.json"),
            config_dir,
            previews,
            runtime,
        };

        let _ = fs::create_dir_all(&paths.runtime);
        let _ = fs::set_permissions(&paths.runtime, fs::Permissions::from_mode(0o700));
        let _ = fs::create_dir_all(&paths.previews);
        let _ = fs::set_permissions(&paths.previews, fs::Permissions::from_mode(0o700));
        paths
    }

    /// Preview cache for a take, keyed by its full path so equal filenames in
    /// different recording folders cannot overwrite each other.
    pub fn preview_path(&self, file: &Path) -> PathBuf {
        self.previews.join(preview_file_name(file))
    }
}

/// The old location, next to the recording. Used to adopt previews made by
/// older Reel versions without leaving them in the recordings folder.
pub fn legacy_preview_path(file: &Path) -> PathBuf {
    PathBuf::from(format!("{}-preview.jpg", file.with_extension("").display()))
}

fn preview_file_name(file: &Path) -> String {
    let stem = file
        .file_stem()
        .map(|stem| stem.to_string_lossy())
        .unwrap_or_default();
    format!("{stem}-{:016x}-preview.jpg", path_hash(file))
}

fn path_hash(path: &Path) -> u64 {
    // FNV-1a keeps cache names stable across Rust versions and builds.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in path.as_os_str().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

struct LocalTime {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

fn local_time() -> LocalTime {
    // SAFETY: time/localtime_r write into a valid tm owned by this thread and
    // are passed non-null pointers for their whole call.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        LocalTime {
            year: tm.tm_year + 1900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
        }
    }
}

/// `2026-10-20 23:59:59` — the format the existing log uses.
pub fn log_stamp() -> String {
    let t = local_time();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

/// `2026-10-20_23-59-59` — used in recording file names.
pub fn file_stamp() -> String {
    let t = local_time();
    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

pub fn log(paths: &Paths, message: &str) {
    let line = format!("[{}] {}\n", log_stamp(), message);
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn reset_log(paths: &Paths) {
    let _ = File::create(&paths.log);
}

pub fn write_atomic(path: &Path, body: &str) -> Result<()> {
    let dir = path
        .parent()
        .context("state path has no parent directory")?;
    let tmp = dir.join(format!(".state.{}.{}", std::process::id(), now_secs()));
    {
        let mut file =
            File::create(&tmp).with_context(|| format!("cannot write {}", tmp.display()))?;
        file.write_all(body.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all().ok();
    }
    fs::rename(&tmp, path).with_context(|| format!("cannot replace {}", path.display()))?;
    Ok(())
}

/// Background webcam pid, if one is recorded.
pub fn read_pid(path: &Path) -> Option<i32> {
    let raw = fs::read_to_string(path).ok()?;
    raw.trim().parse::<i32>().ok()
}

pub fn write_pid(path: &Path, pid: u32) -> Result<()> {
    write_atomic(path, &pid.to_string())
}

pub fn remove_quiet(path: &Path) {
    let _ = fs::remove_file(path);
}

pub fn kill(pid: i32, sig: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    // SAFETY: kill with a plain pid and signal number has no memory effects.
    unsafe { libc::kill(pid, sig) == 0 }
}

pub fn alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    kill(pid, 0)
}

/// Exclusive lock on `$XDG_RUNTIME_DIR/reel/lock`, waiting up to five seconds.
/// Held for the lifetime of the guard.
pub struct Lock {
    _file: File,
}

impl Lock {
    pub fn acquire(paths: &Paths) -> Result<Lock> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&paths.lock)
            .context("cannot open the Reel lock")?;

        let fd = file.as_raw_fd();
        let deadline = SystemTime::now() + Duration::from_secs(5);
        loop {
            // SAFETY: fd belongs to `file` and lives until the guard is dropped.
            let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
            if rc == 0 {
                return Ok(Lock { _file: file });
            }
            if SystemTime::now() >= deadline {
                anyhow::bail!("Another Reel action is still running.");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // Closing the descriptor releases the flock.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_names_are_stable_and_directory_specific() {
        let one = Path::new("/home/a/Videos/Reel/reel-1.mp4");
        let two = Path::new("/home/b/Videos/Reel/reel-1.mp4");

        assert_eq!(preview_file_name(one), preview_file_name(one));
        assert_ne!(preview_file_name(one), preview_file_name(two));
        assert!(preview_file_name(one).ends_with("-preview.jpg"));
    }
}
