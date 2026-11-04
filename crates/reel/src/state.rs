use crate::paths::{self, Paths};
use anyhow::Result;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

pub fn idle() -> Value {
    json!({ "status": "idle" })
}

pub fn idle_with_last(last: &Value) -> Value {
    json!({ "status": "idle", "last": last })
}

fn saved_last(paths: &Paths) -> Option<Value> {
    let text = fs::read_to_string(&paths.last).ok()?;
    serde_json::from_str::<Value>(&text)
        .ok()
        .filter(Value::is_object)
}

/// Set idle without dropping the saved take. The panel uses its target to
/// keep showing the selected area after a cancel, discard, or failed start.
pub fn idle_preserving_last(paths: &Paths) -> Value {
    match saved_last(paths) {
        Some(last) => idle_with_last(&last),
        None => idle(),
    }
}

pub fn idle_with_error_preserving_last(paths: &Paths, message: &str) -> Value {
    let mut state = idle_preserving_last(paths);
    if let Some(object) = state.as_object_mut() {
        object.insert("error".into(), Value::String(message.into()));
    }
    state
}

/// Idle with an error, keeping the capture the error is about. Losing those
/// fields leaves the panel pointing at the previous take and forgets which
/// file is withheld, so a failed quarantine must not use the plain
/// `idle_with_error_preserving_last`.
pub fn idle_with_error_keeping_capture(paths: &Paths, message: &str) -> Value {
    let current = read(paths).unwrap_or_else(idle);
    let mut state = idle_preserving_last(paths);
    for key in [
        "file",
        "output",
        "trimStart",
        "target",
        "targetKind",
        "targetLabel",
    ] {
        if let Some(value) = current.get(key).cloned() {
            set(&mut state, key, value);
        }
    }
    set(&mut state, "status", json!("idle"));
    set(&mut state, "error", json!(message));
    if let Some(file) = current.get("file").cloned() {
        set(&mut state, "pendingCapture", file);
    }
    state
}

pub fn read_or_idle(paths: &Paths) -> Value {
    read(paths).unwrap_or_else(idle)
}

pub fn read(paths: &Paths) -> Option<Value> {
    let text = fs::read_to_string(&paths.state).ok()?;
    serde_json::from_str::<Value>(&text)
        .ok()
        .filter(Value::is_object)
}

pub fn write(paths: &Paths, state: &Value) -> Result<()> {
    let body = serde_json::to_string(state)?;
    paths::write_atomic(&paths.state, &body)
}

pub fn field<'a>(state: &'a Value, key: &str) -> Option<&'a Value> {
    state.get(key).filter(|value| !value.is_null())
}

pub fn field_str(state: &Value, key: &str) -> Option<String> {
    match field(state, key)? {
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

pub fn field_i64(state: &Value, key: &str) -> Option<i64> {
    match field(state, key)? {
        Value::Number(value) => value.as_i64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

pub fn status(paths: &Paths) -> String {
    read(paths)
        .and_then(|state| field_str(&state, "status"))
        .unwrap_or_else(|| "idle".into())
}

/// Whole seconds of recorded material, excluding paused stretches.
pub fn elapsed(paths: &Paths) -> i64 {
    let Some(state) = read(paths) else {
        return 0;
    };
    let now = paths::now_secs();
    let started_at = field_i64(&state, "startedAt").unwrap_or(now);
    let paused_total = field_i64(&state, "pausedTotal").unwrap_or(0);
    let paused_at = field_i64(&state, "pausedAt").unwrap_or(0);
    let mut elapsed = now - started_at - paused_total;
    if field_str(&state, "status").as_deref() == Some("paused") {
        elapsed = paused_at - started_at - paused_total;
    }
    elapsed.max(0)
}

/// Flip a state file left behind by a recorder that exited on its own back to
/// idle, the way `status` does before it answers.
///
/// Reconciliation also retries a quarantine that a read-only recordings folder
/// denied earlier. The capture then moves aside and stops being withheld.
pub fn reconcile(paths: &Paths) {
    let recovered = crate::quarantine::retry(paths);
    let Some(mut state) = read(paths) else {
        return;
    };

    for (old, new) in &recovered {
        if field_str(&state, "file").as_deref() != Some(old.as_str()) {
            continue;
        }
        set(&mut state, "file", json!(new));
        let output = field_str(&state, "output").unwrap_or_default();
        if output.is_empty() || output == old.as_str() {
            set(&mut state, "output", json!(old));
        }
        set(&mut state, "pendingCapture", Value::Null);
        set(
            &mut state,
            "error",
            json!(format!("Untrimmed footage is at {new}.")),
        );
        let _ = write(paths, &state);
    }

    let status = field_str(&state, "status").unwrap_or_default();
    if status == "starting" {
        let pid = paths::read_pid(&paths.starting).unwrap_or(0);
        if pid > 0 && paths_pid_alive(pid) {
            return;
        }
        state["status"] = json!("idle");
        state["error"] = json!("The selection ended without a target.");
        let _ = write(paths, &state);
        paths::remove_quiet(&paths.starting);
        return;
    }
    if status == "finalizing" {
        let pid = field_i64(&state, "finalizePid").unwrap_or(0) as i32;
        let at = field_i64(&state, "finalizeAt").unwrap_or(0);
        let now = paths::now_secs();
        if paths_pid_alive(pid) {
            return;
        }
        // The save job needs a moment to start and publish its pid. Do not
        // race the stop path while it is still shutting the encoder down.
        if pid <= 0 && at > 0 && now.saturating_sub(at) < 20 {
            return;
        }
        state["status"] = json!("idle");
        let recovery = recover_capture(paths, &mut state);
        state["error"] = note_recovery(
            &mut state,
            "The save pass stopped.",
            recovery,
            "The save pass stopped. The take is still in your recordings folder.",
        );
        let _ = write(paths, &state);
        return;
    }
    if status != "recording" && status != "paused" {
        return;
    }
    let pid = field_i64(&state, "pid").unwrap_or(0) as i32;
    if paths_pid_alive(pid) {
        return;
    }
    state["status"] = json!("idle");
    let recovery = recover_capture(paths, &mut state);
    state["error"] = note_recovery(
        &mut state,
        "The recorder exited without a stop.",
        recovery,
        "The recorder exited without a stop.",
    );
    let _ = write(paths, &state);
    crate::zoom::disable_recording_zoom(paths);
}

/// Quarantine a countdown capture that predates the `output` field before it
/// can appear in history. An ordinary take with no countdown needs no move.
fn quarantine_legacy_capture(paths: &Paths, state: &mut Value) -> Option<String> {
    let file = field_str(state, "file")?;
    let capture = Path::new(&file);
    if !capture.is_file() || paths::is_untrimmed(capture) {
        return None;
    }
    let output = field_str(state, "output").unwrap_or_default();
    if !output.is_empty() && output != file {
        return None;
    }
    let trim_start = field_str(state, "trimStart")
        .and_then(|raw| raw.parse::<f64>().ok())
        .unwrap_or(0.0);
    if trim_start <= 0.0 {
        return None;
    }

    let quarantined = crate::quarantine::capture(paths, capture).ok()?;
    let quarantined = quarantined.display().to_string();
    set(state, "file", json!(quarantined.clone()));
    set(
        state,
        "output",
        json!(if output.is_empty() { file } else { output }),
    );
    set(state, "pendingCapture", Value::Null);
    Some(quarantined)
}

struct Recovery {
    capture: String,
    /// True once the capture really sits under an `-untrimmed` name.
    quarantined: bool,
}

fn recover_capture(paths: &Paths, state: &mut Value) -> Option<Recovery> {
    if let Some(capture) = quarantine_legacy_capture(paths, state) {
        return Some(Recovery {
            capture,
            quarantined: true,
        });
    }
    untrimmed_capture(state).map(|capture| Recovery {
        quarantined: paths::is_untrimmed(Path::new(&capture)),
        capture,
    })
}

/// Word the failure so the user knows the file is kept but withheld. A capture
/// whose folder refused the move is still on disk under its ordinary name, and
/// the ledger keeps history from offering it.
fn note_recovery(
    state: &mut Value,
    prefix: &str,
    recovery: Option<Recovery>,
    fallback: &str,
) -> Value {
    match recovery {
        Some(Recovery {
            capture,
            quarantined: true,
        }) => {
            set(state, "pendingCapture", Value::Null);
            json!(format!("{prefix} Untrimmed footage is at {capture}."))
        }
        Some(Recovery {
            capture,
            quarantined: false,
        }) => {
            set(state, "pendingCapture", json!(capture));
            json!(format!(
                "{prefix} The countdown capture at {capture} is withheld from history and \
                 sharing until it can be moved aside."
            ))
        }
        None => json!(fallback),
    }
}

/// A capture that still needs its countdown trimmed off, if it is on disk.
fn untrimmed_capture(state: &Value) -> Option<String> {
    let capture = field_str(state, "file")?;
    if !Path::new(&capture).is_file() {
        return None;
    }
    let output = field_str(state, "output").unwrap_or_default();
    if output.is_empty() {
        let trim_start = field_str(state, "trimStart")
            .and_then(|raw| raw.parse::<f64>().ok())
            .unwrap_or(0.0);
        return (trim_start > 0.0).then_some(capture);
    }
    (capture != output).then_some(capture)
}

fn paths_pid_alive(pid: i32) -> bool {
    pid > 0 && paths::alive(pid)
}

pub fn set(state: &mut Value, key: &str, value: Value) {
    if let Some(map) = state.as_object_mut() {
        map.insert(key.to_string(), value);
    }
}

/// Compact the fields the panel reads: state + now + elapsed + active.
pub fn status_json(paths: &Paths) -> Value {
    reconcile(paths);
    let mut state = read_or_idle(paths);
    let now = paths::now_secs();
    let elapsed = elapsed(paths);
    let active = matches!(
        field_str(&state, "status").as_deref(),
        Some("recording") | Some("paused")
    );
    set(&mut state, "now", json!(now));
    set(&mut state, "elapsed", json!(elapsed));
    set(&mut state, "active", json!(active));
    state
}

/// Replace the whole state with `status` while keeping the other fields.
pub fn with_status(mut state: Value, status: &str) -> Value {
    set(&mut state, "status", json!(status));
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_paths() -> Paths {
        let root = std::env::temp_dir().join(format!(
            "reel-state-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::create_dir_all(&root);
        Paths::for_test(&root)
    }

    #[test]
    fn a_dead_finalize_job_returns_to_idle() {
        let paths = temp_paths();
        let mut state = json!({
            "status": "finalizing",
            "file": "/tmp/reel-test.mp4",
            "finalizePid": 999_999,
            "finalizeAt": paths::now_secs() - 30,
        });
        write(&paths, &state).unwrap();
        reconcile(&paths);
        state = read(&paths).unwrap();
        assert_eq!(field_str(&state, "status").as_deref(), Some("idle"));
        assert!(field_str(&state, "error").unwrap().contains("save pass"));
        let _ = fs::remove_dir_all(paths.runtime);
    }

    #[test]
    fn a_dead_save_names_untrimmed_footage() {
        let paths = temp_paths();
        let videos = paths.runtime.join("Videos");
        fs::create_dir_all(&videos).unwrap();
        let raw = videos.join("reel-1-untrimmed.mp4");
        fs::write(&raw, b"raw").unwrap();
        let state = json!({
            "status": "finalizing",
            "file": raw.display().to_string(),
            "output": videos.join("reel-1.mp4").display().to_string(),
            "finalizePid": 999_999,
            "finalizeAt": paths::now_secs() - 30,
        });
        write(&paths, &state).unwrap();
        reconcile(&paths);
        let state = read(&paths).unwrap();
        assert_eq!(field_str(&state, "status").as_deref(), Some("idle"));
        assert!(field_str(&state, "error").unwrap().contains("Untrimmed"));
        let _ = fs::remove_dir_all(paths.runtime);
    }

    #[test]
    fn a_dead_recorder_quarantines_a_legacy_countdown() {
        let paths = temp_paths();
        let videos = paths.runtime.join("Videos");
        fs::create_dir_all(&videos).unwrap();
        let file = videos.join("reel-legacy.mp4");
        fs::write(&file, b"raw").unwrap();
        let state = json!({
            "status": "recording",
            "file": file.display().to_string(),
            "trimStart": 3.0,
            "pid": 999_999,
        });
        write(&paths, &state).unwrap();
        reconcile(&paths);

        let state = read(&paths).unwrap();
        assert_eq!(field_str(&state, "status").as_deref(), Some("idle"));
        let capture = field_str(&state, "file").unwrap();
        assert!(capture.ends_with("-untrimmed.mp4"));
        assert!(!file.exists());
        assert!(Path::new(&capture).is_file());
        assert_eq!(
            field_str(&state, "output").as_deref(),
            Some(file.display().to_string().as_str())
        );
        assert!(field_str(&state, "error").unwrap().contains("Untrimmed"));
        let _ = fs::remove_dir_all(paths.runtime);
    }

    #[test]
    fn a_denied_quarantine_withholds_the_capture() {
        // SAFETY: geteuid has no memory effects and cannot fail.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        use std::os::unix::fs::PermissionsExt;

        let paths = temp_paths();
        let videos = paths.runtime.join("Videos");
        fs::create_dir_all(&videos).unwrap();
        let file = videos.join("reel-legacy.mp4");
        fs::write(&file, b"raw").unwrap();
        // The recordings folder turns read-only after the capture was created,
        // so the quarantine rename is denied while the file stays readable.
        fs::set_permissions(&videos, fs::Permissions::from_mode(0o555)).unwrap();

        let state = json!({
            "status": "recording",
            "file": file.display().to_string(),
            "trimStart": 3.0,
            "pid": 999_999,
        });
        write(&paths, &state).unwrap();
        reconcile(&paths);

        let state = read(&paths).unwrap();
        assert_eq!(field_str(&state, "status").as_deref(), Some("idle"));
        assert_eq!(
            field_str(&state, "file").as_deref(),
            Some(file.display().to_string().as_str())
        );
        assert!(field_str(&state, "error").unwrap().contains("withheld"));
        assert_eq!(
            field_str(&state, "pendingCapture").as_deref(),
            Some(file.display().to_string().as_str())
        );
        assert!(crate::quarantine::is_blocked(&paths, &file));
        assert!(file.is_file(), "the raw recovery copy is kept");

        // Once the folder is writable again the retry moves the capture aside
        // and the record is released.
        fs::set_permissions(&videos, fs::Permissions::from_mode(0o755)).unwrap();
        reconcile(&paths);

        let state = read(&paths).unwrap();
        let capture = field_str(&state, "file").unwrap();
        assert!(capture.ends_with("-untrimmed.mp4"));
        assert!(!file.exists());
        assert!(Path::new(&capture).is_file());
        assert!(!crate::quarantine::is_blocked(&paths, &file));
        assert!(field_str(&state, "pendingCapture").is_none());
        assert!(field_str(&state, "error").unwrap().contains("Untrimmed"));
        let _ = fs::remove_dir_all(paths.runtime);
    }

    #[test]
    fn a_new_finalize_job_gets_a_grace_period() {
        let paths = temp_paths();
        let state = json!({
            "status": "finalizing",
            "file": "/tmp/reel-test.mp4",
            "finalizePid": 0,
            "finalizeAt": paths::now_secs(),
        });
        write(&paths, &state).unwrap();
        reconcile(&paths);
        let state = read(&paths).unwrap();
        assert_eq!(field_str(&state, "status").as_deref(), Some("finalizing"));
        let _ = fs::remove_dir_all(paths.runtime);
    }

    #[test]
    fn a_live_finalize_job_stays_finalizing() {
        let paths = temp_paths();
        let state = json!({
            "status": "finalizing",
            "file": "/tmp/reel-test.mp4",
            "finalizePid": std::process::id(),
            "finalizeAt": paths::now_secs() - 30,
        });
        write(&paths, &state).unwrap();
        reconcile(&paths);
        let state = read(&paths).unwrap();
        assert_eq!(field_str(&state, "status").as_deref(), Some("finalizing"));
        let _ = fs::remove_dir_all(paths.runtime);
    }
}
