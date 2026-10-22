use crate::paths::{self, Paths};
use anyhow::Result;
use serde_json::{json, Value};
use std::fs;

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
pub fn reconcile(paths: &Paths) {
    let Some(mut state) = read(paths) else {
        return;
    };
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
    if status != "recording" && status != "paused" {
        return;
    }
    let pid = field_i64(&state, "pid").unwrap_or(0) as i32;
    if paths_pid_alive(pid) {
        return;
    }
    state["status"] = json!("idle");
    state["error"] = json!("The recorder exited without a stop.");
    let _ = write(paths, &state);
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
