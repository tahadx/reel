use crate::paths::{self, Paths};
use crate::state;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A save job needs a moment to publish its pid after it is spawned. During
/// that window a job with no pid is alive, not dead.
const STARTUP_GRACE_SECS: i64 = 20;
const MAX_JOB_BYTES: u64 = 64 * 1024;

/// Save jobs live outside `state.json`, one small file each. The recording
/// state can then return to idle as soon as the encoder stops while any number
/// of saves keep running beside it.
pub fn create(
    paths: &Paths,
    file: &Path,
    output: &Path,
    trim_start: f64,
    started_at: i64,
) -> Result<String> {
    fs::create_dir_all(&paths.saves)
        .with_context(|| format!("cannot create {}", paths.saves.display()))?;
    let _ = fs::set_permissions(&paths.saves, fs::Permissions::from_mode(0o700));
    let id = unique_id(paths);
    let job = json!({
        "id": id,
        "file": file.display().to_string(),
        "output": output.display().to_string(),
        "trimStart": trim_start,
        "startedAt": started_at,
        "createdAt": paths::now_secs(),
        "pid": 0,
    });
    paths::write_atomic(&job_path(paths, &id), &serde_json::to_string(&job)?)?;
    Ok(id)
}

fn unique_id(paths: &Paths) -> String {
    let stamp = paths::now_millis();
    let pid = std::process::id();
    let mut suffix = 0;
    loop {
        let id = if suffix == 0 {
            format!("{stamp}-{pid}")
        } else {
            format!("{stamp}-{pid}-{suffix}")
        };
        if !job_path(paths, &id).exists() {
            return id;
        }
        suffix += 1;
    }
}

pub fn job_path(paths: &Paths, id: &str) -> PathBuf {
    paths.saves.join(format!("{id}.json"))
}

pub fn telemetry_path(paths: &Paths, id: &str) -> PathBuf {
    paths.saves.join(format!("{id}.telemetry.jsonl"))
}

/// Copy the live telemetry file into the job so a new take can clear the
/// runtime file immediately without taking this save's pointer data with it.
pub fn snapshot_telemetry(paths: &Paths, id: &str) -> Option<PathBuf> {
    if !paths.session_telemetry.is_file() {
        return None;
    }
    let target = telemetry_path(paths, id);
    match fs::copy(&paths.session_telemetry, &target) {
        Ok(_) => {
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(0o600));
            Some(target)
        }
        Err(err) => {
            paths::log(
                paths,
                &format!(
                    "save job {id}: cannot snapshot telemetry at {}: {err}",
                    target.display()
                ),
            );
            None
        }
    }
}

pub fn set_pid(paths: &Paths, id: &str, pid: u32) {
    update(paths, id, |job| {
        job["pid"] = json!(pid);
    });
}

/// Keep the job pointing at the file the save pass really owns after a late
/// quarantine rename.
pub fn update_paths(paths: &Paths, id: &str, file: &Path, output: &Path) {
    update(paths, id, |job| {
        job["file"] = json!(file.display().to_string());
        job["output"] = json!(output.display().to_string());
    });
}

fn update(paths: &Paths, id: &str, change: impl FnOnce(&mut Value)) {
    let path = job_path(paths, id);
    let Some(mut job) = read_job(&path) else {
        return;
    };
    change(&mut job);
    if let Ok(body) = serde_json::to_string(&job) {
        let _ = paths::write_atomic(&path, &body);
    }
}

pub fn remove(paths: &Paths, id: &str) {
    paths::remove_quiet(&job_path(paths, id));
    paths::remove_quiet(&telemetry_path(paths, id));
}

/// Every live save, oldest first. Dead jobs are cleaned by `reconcile`.
pub fn list(paths: &Paths) -> Vec<Value> {
    let Ok(entries) = fs::read_dir(&paths.saves) else {
        return Vec::new();
    };
    let mut jobs: Vec<Value> = entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
        .filter_map(|entry| read_job(&entry.path()))
        .collect();
    jobs.sort_by_key(|job| {
        (
            job.get("startedAt").and_then(Value::as_i64).unwrap_or(0),
            job.get("createdAt").and_then(Value::as_i64).unwrap_or(0),
        )
    });
    jobs
}

pub fn count(paths: &Paths) -> usize {
    list(paths).len()
}

/// Drop jobs whose process died. The take's raw file is left in place; only a
/// save that never published its output leaves a file behind, and it stays
/// under its hidden capture name so it cannot appear in history.
pub fn reconcile(paths: &Paths) {
    let Ok(entries) = fs::read_dir(&paths.saves) else {
        return;
    };
    let now = paths::now_secs();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .file_name()
            .map(|name| name.to_string_lossy().ends_with(".json"))
            .unwrap_or(false)
        {
            continue;
        }
        let id = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_default();
        let Some(job) = read_job(&path) else {
            remove(paths, &id);
            continue;
        };
        let pid = job.get("pid").and_then(Value::as_i64).unwrap_or(0) as i32;
        if pid > 0 && paths::alive(pid) {
            continue;
        }
        let created = job.get("createdAt").and_then(Value::as_i64).unwrap_or(now);
        if pid <= 0 && now.saturating_sub(created) < STARTUP_GRACE_SECS {
            continue;
        }

        let file = job.get("file").and_then(Value::as_str).unwrap_or_default();
        paths::log(
            paths,
            &format!("save job {id}: stopped before the take was published ({file})"),
        );
        remove(paths, &id);

        // Do not touch a recording that is live now. When the engine is idle,
        // surface the loss once; the raw capture is still on disk.
        if state::status(paths) != "idle" || !list(paths).is_empty() {
            continue;
        }
        let message = if file.is_empty() {
            "The save pass stopped before the take was published.".to_string()
        } else {
            format!("The save pass stopped. Untrimmed footage is still at {file}.")
        };
        let mut next = state::idle_preserving_last(paths);
        state::set(&mut next, "error", json!(message));
        let _ = state::write(paths, &next);
    }
}

fn read_job(path: &Path) -> Option<Value> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_JOB_BYTES {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&text)
        .ok()
        .filter(Value::is_object)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths(label: &str) -> Paths {
        let root = std::env::temp_dir().join(format!(
            "reel-saves-{label}-{}-{}",
            std::process::id(),
            paths::now_millis()
        ));
        fs::create_dir_all(&root).unwrap();
        Paths::for_test(&root)
    }

    #[test]
    fn several_saves_can_run_beside_a_recording() {
        let paths = temp_paths("many");
        let mut ids = Vec::new();
        for index in 0..10 {
            let file = paths.runtime.join(format!("take-{index}.capture.mp4"));
            let output = paths.runtime.join(format!("take-{index}.mp4"));
            let id = create(&paths, &file, &output, 0.0, paths::now_secs()).unwrap();
            set_pid(&paths, &id, std::process::id());
            ids.push(id);
        }

        let jobs = list(&paths);
        assert_eq!(jobs.len(), 10);
        assert!(jobs[0].get("output").is_some());

        for id in ids {
            remove(&paths, &id);
        }
        assert!(list(&paths).is_empty());
        let _ = fs::remove_dir_all(paths.runtime);
    }

    #[test]
    fn a_dead_save_is_dropped_without_touching_a_live_take() {
        let paths = temp_paths("dead");
        let file = paths.runtime.join("take.capture.mp4");
        let output = paths.runtime.join("take.mp4");
        let id = create(&paths, &file, &output, 0.0, paths::now_secs()).unwrap();
        set_pid(&paths, &id, 999_999);
        let mut job = read_job(&job_path(&paths, &id)).unwrap();
        job["createdAt"] = json!(paths::now_secs() - 60);
        paths::write_atomic(
            &job_path(&paths, &id),
            &serde_json::to_string(&job).unwrap(),
        )
        .unwrap();

        state::write(
            &paths,
            &json!({ "status": "recording", "pid": std::process::id() }),
        )
        .unwrap();
        reconcile(&paths);
        assert!(list(&paths).is_empty());
        assert_eq!(state::status(&paths), "recording");
        let _ = fs::remove_dir_all(paths.runtime);
    }
}
