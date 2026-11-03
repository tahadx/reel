//! Durable record of captures that still contain their countdown.
//!
//! The `-untrimmed` file name is the primary "not an ordinary take" marker, but
//! it only exists once the rename succeeds. A recordings folder can turn
//! read-only between the capture and the save pass; the rename then fails while
//! the raw file stays readable under its ordinary name. This ledger remembers
//! that path so history and Share skip it no matter what it is called, and the
//! entry is cleared as soon as a later attempt moves the file aside.
//!
//! The record is independent of the rename and of the state file: it survives a
//! restarted engine, a cleared runtime directory, and a state file that was
//! replaced by the next take.

use crate::paths::{self, Paths};
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const VERSION: i64 = 1;

/// Canonical key for a capture. Falls back to the path as given when the file
/// has already been moved, which is the normal case after quarantine.
pub fn key(file: &Path) -> String {
    fs::canonicalize(file)
        .unwrap_or_else(|_| file.to_path_buf())
        .display()
        .to_string()
}

/// Move a raw capture out of the ordinary take list, keeping the ledger in
/// step: a success releases any earlier record, a failure records the path so
/// the capture stays out of history until a later attempt succeeds.
pub fn capture(paths: &Paths, file: &Path) -> std::io::Result<PathBuf> {
    let before = key(file);
    match paths::quarantine_capture(file) {
        Ok(target) => {
            clear_key(paths, &before);
            Ok(target)
        }
        Err(err) => {
            record(paths, file, "countdown");
            Err(err)
        }
    }
}

/// Remember that `file` is a raw capture that must not be shared.
pub fn record(paths: &Paths, file: &Path, reason: &str) {
    let path = key(file);
    let mut entries = ledger(paths);
    if entries.iter().any(|entry| entry["path"] == json!(path)) {
        return;
    }
    entries.push(json!({
        "path": path,
        "reason": reason,
        "at": paths::now_secs(),
    }));
    if let Err(err) = write(paths, &entries) {
        paths::log(paths, &format!("quarantine: cannot record {path}: {err}"));
    }
}

/// Forget a capture once its countdown has been moved aside.
#[cfg(test)]
pub fn clear(paths: &Paths, file: &Path) {
    clear_key(paths, &key(file));
}

fn clear_key(paths: &Paths, path: &str) {
    let mut entries = ledger(paths);
    let before = entries.len();
    entries.retain(|entry| entry["path"] != json!(path));
    if entries.len() != before {
        if let Err(err) = write(paths, &entries) {
            paths::log(paths, &format!("quarantine: cannot clear {path}: {err}"));
        }
    }
}

/// Every path that must stay out of history.
pub fn blocked(paths: &Paths) -> BTreeSet<String> {
    ledger(paths)
        .into_iter()
        .filter_map(|entry| entry["path"].as_str().map(str::to_string))
        .collect()
}

#[cfg(test)]
pub fn is_blocked(paths: &Paths, file: &Path) -> bool {
    ledger(paths)
        .iter()
        .any(|entry| entry["path"] == json!(key(file)))
}

/// Retry every recorded capture. A recordings folder that was read-only when
/// the save pass failed can become writable again; the capture is then moved
/// aside and released. Returns the `(old, new)` pairs so the caller can fix the
/// state file, which may still name the ordinary path.
pub fn retry(paths: &Paths) -> Vec<(String, String)> {
    let mut recovered = Vec::new();
    for entry in ledger(paths) {
        let Some(raw) = entry["path"].as_str() else {
            continue;
        };
        let file = PathBuf::from(raw);
        if !file.is_file() || paths::is_untrimmed(&file) {
            // Nothing left to protect: the file is gone, or a previous attempt
            // already moved it.
            clear_key(paths, raw);
            continue;
        }
        match paths::quarantine_capture(&file) {
            Ok(target) => {
                let new = target.display().to_string();
                paths::log(paths, &format!("quarantine: recovered {raw} -> {new}"));
                clear_key(paths, raw);
                recovered.push((raw.to_string(), new));
            }
            Err(_) => continue,
        }
    }
    recovered
}

fn ledger(paths: &Paths) -> Vec<Value> {
    let Ok(text) = fs::read_to_string(&paths.pending) else {
        return Vec::new();
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|payload| payload.get("captures").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

fn write(paths: &Paths, entries: &[Value]) -> Result<()> {
    if let Some(dir) = paths.pending.parent() {
        fs::create_dir_all(dir)?;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    let body = serde_json::to_string(&json!({
        "version": VERSION,
        "captures": entries,
    }))?;
    paths::write_atomic(&paths.pending, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_paths() -> Paths {
        let root = std::env::temp_dir().join(format!(
            "reel-quarantine-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::create_dir_all(&root);
        Paths::for_test(&root)
    }

    fn read_only(dir: &Path) -> bool {
        // root ignores directory permissions, so the denied-rename cases cannot
        // be exercised there.
        // SAFETY: geteuid has no memory effects and cannot fail.
        if unsafe { libc::geteuid() } == 0 {
            return false;
        }
        fs::set_permissions(dir, fs::Permissions::from_mode(0o555)).is_ok()
    }

    #[test]
    fn a_denied_move_records_the_ordinary_path() {
        let paths = temp_paths();
        let dir = paths.runtime.join("Videos");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("reel-1.mp4");
        fs::write(&file, b"raw").unwrap();
        if !read_only(&dir) {
            let _ = fs::remove_dir_all(paths.runtime);
            return;
        }

        assert!(capture(&paths, &file).is_err());
        assert!(file.is_file());
        assert!(is_blocked(&paths, &file));

        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        let recovered = retry(&paths);
        assert_eq!(recovered.len(), 1);
        assert!(!is_blocked(&paths, &file));
        assert!(!file.exists());
        assert!(Path::new(&recovered[0].1).is_file());
        let _ = fs::remove_dir_all(paths.runtime);
    }

    #[test]
    fn a_successful_move_releases_the_record() {
        let paths = temp_paths();
        let dir = paths.runtime.join("Videos");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("reel-1.mp4");
        fs::write(&file, b"raw").unwrap();

        record(&paths, &file, "countdown");
        assert!(is_blocked(&paths, &file));

        let target = capture(&paths, &file).unwrap();
        assert!(target.ends_with("reel-1-untrimmed.mp4"));
        assert!(!is_blocked(&paths, &file));
        let _ = fs::remove_dir_all(paths.runtime);
    }

    #[test]
    fn a_deleted_capture_is_forgotten() {
        let paths = temp_paths();
        let file = paths.runtime.join("gone.mp4");
        record(&paths, &file, "countdown");
        assert!(is_blocked(&paths, &file));

        assert!(retry(&paths).is_empty());
        assert!(!is_blocked(&paths, &file));
        let _ = fs::remove_dir_all(paths.runtime);
    }
}
