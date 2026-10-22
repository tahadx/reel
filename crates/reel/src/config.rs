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
        "cursor": true,
        "webcam": "off",
        "slowmo": "off",
        "countdown": 0,
        "fps": 60,
        "quality": "very_high",
        "codec": "auto",
        "resolution": "0x0",
        "container": "mp4",
        "outputDir": "",
        "filenamePrefix": "",
        "webcamDevice": "",
        "notifications": false
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
