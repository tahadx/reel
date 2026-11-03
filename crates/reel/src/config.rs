use crate::cli::{ConfigAction, ConfigArgs};
use crate::paths::{self, Paths};
use crate::util;
use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::fs;
use std::path::PathBuf;

pub fn defaults() -> Value {
    serde_json::json!({
        "desktopAudio": true,
        "microphoneAudio": false,
        "noiseCancellation": false,
        "cursor": true,
        "webcam": "off",
        "countdown": 0,
        "fps": 60,
        "quality": "very_high",
        "codec": "auto",
        "resolution": "0x0",
        "container": "mp4",
        "outputDir": "",
        "filenamePrefix": "",
        "webcamDevice": "",
        "notifications": false,
        "openWithOmacut": true,
        "manualZoom": "on",
        "zoomMax": 2.0,
        "panStep": 60,
        "captureMode": "auto"
    })
}

pub fn load(paths: &Paths) -> Value {
    let Ok(text) = fs::read_to_string(&paths.config) else {
        return Value::Object(Map::new());
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(value @ Value::Object(_)) => value,
        _ => Value::Object(Map::new()),
    }
}

fn raw(settings: &Value, key: &str) -> Option<String> {
    match settings.get(key)? {
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

pub fn cfg_raw(paths: &Paths, key: &str) -> Option<String> {
    let settings = load(paths);
    raw(&settings, key).filter(|value| !value.is_empty())
}

pub fn cfg(paths: &Paths, key: &str, fallback: &str) -> String {
    cfg_raw(paths, key).unwrap_or_else(|| fallback.to_string())
}

/// Read one key from an already-loaded settings object. Start resolves many
/// options at once; loading and parsing reel.json once avoids repeating that
/// work for every key.
pub fn cfg_in(settings: &Value, key: &str, fallback: &str) -> String {
    raw(settings, key)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

/// Zoom depth as the engine understands it. The panel stores the choice as a
/// numeric string (`"3.0"`) while older configs and the CLI use a number, so
/// both must parse. Anything outside the supported range is clamped here, which
/// keeps every caller on the same 1.1x–4x scale.
pub fn zoom_max_value(value: Option<&Value>) -> Option<f64> {
    let parsed = match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    parsed
        .filter(|zoom| zoom.is_finite())
        .map(|zoom| zoom.clamp(1.1, 4.0))
}

pub fn output_dir(paths: &Paths) -> PathBuf {
    let configured = cfg(paths, "outputDir", "");
    if !configured.is_empty() {
        PathBuf::from(util::expand_home(&configured))
    } else {
        util::default_output_dir()
    }
}

fn write(paths: &Paths, settings: &Value) -> Result<()> {
    fs::create_dir_all(&paths.config_dir)
        .with_context(|| format!("cannot create {}", paths.config_dir.display()))?;
    let body = serde_json::to_string_pretty(settings)?;
    paths::write_atomic(&paths.config, &body)
}

pub fn run(paths: &Paths, args: ConfigArgs) -> Result<()> {
    match args.action.unwrap_or(ConfigAction::Show) {
        ConfigAction::Path => {
            println!("{}", paths.config.display());
        }
        ConfigAction::Show => {
            let settings = load(paths);
            println!("{}", serde_json::to_string_pretty(&settings)?);
        }
        ConfigAction::Init => {
            if paths.config.is_file() {
                return Ok(());
            }
            write(paths, &defaults())?;
        }
        ConfigAction::Apply { patch } => {
            let patch = if patch.trim().is_empty() {
                let mut input = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)
                    .context("cannot read the settings patch")?;
                input
            } else {
                patch
            };
            let patch: Value =
                serde_json::from_str(&patch).context("config apply expects a JSON object.")?;
            let Value::Object(patch) = patch else {
                anyhow::bail!("config apply expects a JSON object.");
            };
            let mut settings = load(paths);
            let map = settings.as_object_mut().expect("config is an object");
            for (key, value) in patch {
                map.insert(key, value);
            }
            write(paths, &settings)?;
        }
        ConfigAction::Set { key, value } => {
            let parsed = serde_json::from_str::<Value>(&value)
                .unwrap_or_else(|_| Value::String(value.clone()));
            let mut settings = load(paths);
            let map = settings.as_object_mut().expect("config is an object");
            map.insert(key, parsed);
            write(paths, &settings)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cfg_in_uses_the_loaded_object_and_fallback() {
        let settings = serde_json::json!({ "fps": 120, "empty": "" });
        assert_eq!(cfg_in(&settings, "fps", "60"), "120");
        assert_eq!(cfg_in(&settings, "empty", "fallback"), "fallback");
        assert_eq!(cfg_in(&settings, "missing", "fallback"), "fallback");
    }

    #[test]
    fn zoom_max_accepts_numbers_and_numeric_strings() {
        let number = serde_json::json!(3.0);
        let string = serde_json::json!("2.5");
        assert_eq!(zoom_max_value(Some(&number)), Some(3.0));
        assert_eq!(zoom_max_value(Some(&string)), Some(2.5));
        assert_eq!(zoom_max_value(Some(&serde_json::json!(" 1.5 "))), Some(1.5));
    }

    #[test]
    fn zoom_max_rejects_junk_and_clamps_the_range() {
        assert_eq!(zoom_max_value(Some(&serde_json::json!("deep"))), None);
        assert_eq!(zoom_max_value(Some(&serde_json::json!(null))), None);
        assert_eq!(zoom_max_value(None), None);
        assert_eq!(zoom_max_value(Some(&serde_json::json!("0.8"))), Some(1.1));
        assert_eq!(zoom_max_value(Some(&serde_json::json!(9))), Some(4.0));
    }
}
