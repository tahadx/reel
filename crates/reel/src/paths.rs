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
    pub session_telemetry: PathBuf,
    pub config_dir: PathBuf,
    pub config: PathBuf,
    pub previews: PathBuf,
    /// Durable list of captures that must stay out of history.
    pub pending: PathBuf,
}

impl Paths {
    /// A `Paths` rooted in a temporary directory, for unit tests.
    #[cfg(test)]
    pub fn for_test(root: &Path) -> Paths {
        let root = root.to_path_buf();
        Paths {
            state: root.join("state.json"),
            last: root.join("last.json"),
            lock: root.join("lock"),
            log: root.join("reel.log"),
            pid_file: root.join("reel.pid"),
            socket: root.join("reel.sock"),
            starting: root.join("starting.pid"),
            cancel: root.join("cancel"),
            webcam_pid: root.join("webcam.pid"),
            session_telemetry: root.join("session.telemetry.jsonl"),
            config_dir: root.join("config"),
            config: root.join("config.json"),
            previews: root.join("previews"),
            pending: root.join("pending-quarantine.json"),
            runtime: root,
        }
    }

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

        // Runtime state can live on a tmpfs that disappears at logout, so the
        // ledger of captures that still need trimming is kept in the durable
        // state directory instead.
        let state_home = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/state"));
        let state_dir = state_home.join("omarchy/reel");
        let pending = state_dir.join("pending-quarantine.json");

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
            session_telemetry: runtime.join("session.telemetry.jsonl"),
            config: config_dir.join("reel.json"),
            config_dir,
            previews,
            pending,
            runtime,
        };

        let _ = fs::create_dir_all(&paths.runtime);
        let _ = fs::set_permissions(&paths.runtime, fs::Permissions::from_mode(0o700));
        let _ = fs::create_dir_all(&paths.previews);
        let _ = fs::set_permissions(&paths.previews, fs::Permissions::from_mode(0o700));
        if let Some(dir) = paths.pending.parent() {
            let _ = fs::create_dir_all(dir);
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }
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

/// Raw captures that still contain the countdown are written here first. The
/// name keeps them out of the recent list until the countdown has been trimmed
/// away, so a failed save cannot offer overlay footage as an ordinary take.
pub fn untrimmed_path(file: &Path) -> PathBuf {
    let stem = file
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = file
        .extension()
        .map(|ext| ext.to_string_lossy().to_string())
        .unwrap_or_default();
    let parent = file.parent().unwrap_or_else(|| Path::new(""));
    if ext.is_empty() {
        parent.join(format!("{stem}-untrimmed"))
    } else {
        parent.join(format!("{stem}-untrimmed.{ext}"))
    }
}

/// `<take-stem>.telemetry.jsonl` next to a saved take.
// Called by `telemetry::persist`, which feat/auto-zoom wires into finalize.
#[allow(dead_code)]
pub fn telemetry_path(file: &Path) -> PathBuf {
    let stem = file
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default();
    let parent = file.parent().unwrap_or_else(|| Path::new(""));
    parent.join(format!("{stem}.telemetry.jsonl"))
}

/// The normal name a quarantined raw capture would be saved under. Older
/// state files only recorded `file`, so this recovers the final path when the
/// capture was already moved aside.
pub fn trimmed_path(file: &Path) -> PathBuf {
    let stem = file
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = stem.strip_suffix("-untrimmed").unwrap_or(&stem);
    let ext = file
        .extension()
        .map(|ext| ext.to_string_lossy().to_string())
        .unwrap_or_default();
    let parent = file.parent().unwrap_or_else(|| Path::new(""));
    if ext.is_empty() {
        parent.join(stem)
    } else {
        parent.join(format!("{stem}.{ext}"))
    }
}

/// A free `-untrimmed` name. If a stale quarantine already owns the first
/// name, number it before the marker so history still skips the file.
pub fn quarantine_path(file: &Path) -> PathBuf {
    let first = untrimmed_path(file);
    if !first.exists() {
        return first;
    }
    let stem = file
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = file
        .extension()
        .map(|ext| ext.to_string_lossy().to_string())
        .unwrap_or_default();
    let parent = file.parent().unwrap_or_else(|| Path::new(""));
    let mut suffix = 2;
    loop {
        let name = if ext.is_empty() {
            format!("{stem}-{suffix}-untrimmed")
        } else {
            format!("{stem}-{suffix}-untrimmed.{ext}")
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
        suffix += 1;
    }
}

/// Move a raw capture out of the ordinary take name before a fallible save
/// pass. Never overwrite another quarantine.
pub fn quarantine_capture(file: &Path) -> std::io::Result<PathBuf> {
    let target = quarantine_path(file);
    std::fs::rename(file, &target)?;
    Ok(target)
}

/// True for a raw capture that never made it through the countdown trim.
pub fn is_untrimmed(file: &Path) -> bool {
    file.file_stem()
        .map(|stem| {
            stem.to_string_lossy()
                .to_ascii_lowercase()
                .ends_with("-untrimmed")
        })
        .unwrap_or(false)
}

/// True for a half-written save pass that died before the rename.
pub fn is_partial(file: &Path) -> bool {
    file.file_name()
        .map(|name| {
            name.to_string_lossy()
                .to_ascii_lowercase()
                .contains(".part.")
        })
        .unwrap_or(false)
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

    #[test]
    fn telemetry_names_follow_the_take() {
        let file = Path::new("/home/a/Videos/Reel/reel-1.2.3.mp4");
        assert_eq!(
            telemetry_path(file),
            Path::new("/home/a/Videos/Reel/reel-1.2.3.telemetry.jsonl")
        );
    }

    #[test]
    fn untrimmed_names_keep_the_folder_and_extension() {
        let file = Path::new("/home/a/Videos/Reel/reel-1.2.3.mp4");

        assert_eq!(
            untrimmed_path(file),
            Path::new("/home/a/Videos/Reel/reel-1.2.3-untrimmed.mp4")
        );
        assert!(is_untrimmed(Path::new(
            "/home/a/Videos/Reel/reel-1.2.3-untrimmed.mp4"
        )));
        assert!(!is_untrimmed(file));
    }

    #[test]
    fn trimmed_names_undo_the_quarantine_marker() {
        let raw = Path::new("/home/a/Videos/Reel/reel-1.2.3-untrimmed.mp4");

        assert_eq!(
            trimmed_path(raw),
            Path::new("/home/a/Videos/Reel/reel-1.2.3.mp4")
        );
        assert_eq!(
            trimmed_path(Path::new("/home/a/Videos/Reel/reel-1.2.3.mp4")),
            Path::new("/home/a/Videos/Reel/reel-1.2.3.mp4")
        );
    }

    #[test]
    fn a_stale_quarantine_gets_a_new_numbered_name() {
        let root = std::env::temp_dir().join(format!(
            "reel-paths-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&root).unwrap();
        let file = root.join("reel-1.mp4");
        assert_eq!(quarantine_path(&file), root.join("reel-1-untrimmed.mp4"));
        fs::write(root.join("reel-1-untrimmed.mp4"), b"old").unwrap();
        assert_eq!(quarantine_path(&file), root.join("reel-1-2-untrimmed.mp4"));
        let _ = fs::remove_dir_all(root);
    }
}
