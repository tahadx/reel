use crate::cli::{ConfigAction, ConfigArgs};
use crate::paths::{self, Paths};
use crate::util;
use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::fs;
use std::io::Read;
use std::path::PathBuf;

pub fn defaults() -> Value {
    serde_json::json!({
        "desktopAudio": true,
        "microphoneAudio": false,
        "noiseCancellation": false,
        "desktopDevice": "",
        "microphoneDevice": "",
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
        "manualZoom": "on",
        "zoomMax": 2.0,
        "panStep": 60,
        "autoZoom": false,
        "autoZoomDepth": 2.0
    })
}

/// Settings are a small object by design. A larger file is corrupt or planted;
/// ignore it rather than turn it into the shell's status output.
pub const MAX_CONFIG_BYTES: u64 = 256 * 1024;

pub fn load(paths: &Paths) -> Value {
    let empty = || Value::Object(Map::new());
    let Ok(metadata) = fs::metadata(&paths.config) else {
        return empty();
    };
    if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES {
        return empty();
    }
    let Ok(file) = fs::File::open(&paths.config) else {
        return empty();
    };
    let mut text = String::new();
    if file
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut text)
        .is_err()
        || text.len() as u64 > MAX_CONFIG_BYTES
    {
        return empty();
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(value @ Value::Object(_)) => value,
        _ => empty(),
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

/// Keys the retired live-compositor zoom wrote beside `autoZoom`. A file that
/// holds any of them predates the save-pass setting, so its `autoZoom` is
/// stale data and must be ignored — the contract names these keys and says not
/// to delete them, so the read is skipped rather than the file rewritten.
fn stale_auto_zoom(settings: &Value) -> bool {
    [
        "autoZoomMode",
        "autoZoomIdle",
        "autoZoomMax",
        "focusAutoZoom",
    ]
    .iter()
    .any(|key| settings.get(*key).is_some())
}

/// The panel's save-pass zoom key. It is deliberately not `autoZoom`: the
/// retired live-zoom build used that name, and its value still sits in old
/// config files, so the two must never be confused.
pub const SAVE_ZOOM_KEY: &str = "zoomOnClicks";

fn switch_value(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "on" | "true" | "yes" | "1"
        ),
        _ => false,
    }
}

/// Auto zoom for a take: one on/off choice, no styles. `zoomOnClicks` is the
/// panel's key. `autoZoom` is read only for a file that has no trace of the
/// retired live-zoom build, so a stale value from that build cannot zoom a
/// take the panel shows as off. Anything unrecognised is off.
pub fn auto_zoom(settings: &Value) -> bool {
    if let Some(value) = settings.get(SAVE_ZOOM_KEY) {
        return switch_value(value);
    }
    if stale_auto_zoom(settings) {
        return false;
    }
    settings.get("autoZoom").map(switch_value).unwrap_or(false)
}

/// Auto zoom depth for a take. Only the three contract depths are accepted;
/// anything else falls back to 2x, like the project defaults.
pub fn auto_zoom_depth(settings: &Value) -> f64 {
    let parsed = match settings.get("autoZoomDepth") {
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    crate::project::valid_depth(parsed.unwrap_or(2.0))
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
    fn an_oversized_config_is_ignored() {
        let root = std::env::temp_dir().join(format!(
            "reel-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&root).unwrap();
        let paths = Paths::for_test(&root);
        fs::write(&paths.config, "x".repeat(MAX_CONFIG_BYTES as usize + 1)).unwrap();
        assert!(load(&paths).as_object().unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }

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

    #[test]
    fn auto_zoom_accepts_the_panel_and_legacy_shapes() {
        for value in [
            serde_json::json!("on"),
            serde_json::json!("ON"),
            serde_json::json!(" true "),
            serde_json::json!(true),
            serde_json::json!(1),
        ] {
            assert!(auto_zoom(&serde_json::json!({ "autoZoom": value })));
        }
        for value in [
            serde_json::json!("off"),
            serde_json::json!("no"),
            serde_json::json!(false),
            serde_json::json!(0),
            serde_json::json!("cinematic"),
        ] {
            assert!(!auto_zoom(&serde_json::json!({ "autoZoom": value })));
        }
        assert!(!auto_zoom(&serde_json::json!({})));
        // The old style key is gone; it must not switch auto zoom on.
        assert!(!auto_zoom(
            &serde_json::json!({ "autoZoomStyle": "cinematic" })
        ));
    }

    #[test]
    fn auto_zoom_ignores_a_file_left_by_the_live_zoom_build() {
        let stale = serde_json::json!({
            "autoZoom": "on",
            "autoZoomIdle": 3200,
            "autoZoomMax": 2,
            "autoZoomMode": "live_compositor",
            "focusAutoZoom": true
        });
        assert!(!auto_zoom(&stale));
        // A file without the retired keys still reads the old key.
        assert!(auto_zoom(&serde_json::json!({ "autoZoom": "on" })));
    }

    #[test]
    fn the_panel_key_beats_the_stale_one() {
        let both = serde_json::json!({
            "autoZoom": "on",
            "autoZoomMode": "live_compositor",
            "zoomOnClicks": "off"
        });
        assert!(!auto_zoom(&both));
        let on = serde_json::json!({ "autoZoom": "off", "autoZoomMode": "live_compositor", "zoomOnClicks": "on" });
        assert!(auto_zoom(&on));
    }

    #[test]
    fn auto_zoom_depth_accepts_numbers_and_strings() {
        assert_eq!(
            auto_zoom_depth(&serde_json::json!({ "autoZoomDepth": 1.5 })),
            1.5
        );
        assert_eq!(
            auto_zoom_depth(&serde_json::json!({ "autoZoomDepth": "2.5" })),
            2.5
        );
        assert_eq!(
            auto_zoom_depth(&serde_json::json!({ "autoZoomDepth": " 2.0 " })),
            2.0
        );
    }

    #[test]
    fn auto_zoom_depth_falls_back_to_two() {
        assert_eq!(auto_zoom_depth(&serde_json::json!({})), 2.0);
        assert_eq!(
            auto_zoom_depth(&serde_json::json!({ "autoZoomDepth": 9 })),
            2.0
        );
        assert_eq!(
            auto_zoom_depth(&serde_json::json!({ "autoZoomDepth": "deep" })),
            2.0
        );
    }
}
