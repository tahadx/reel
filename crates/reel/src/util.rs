use crate::paths::Paths;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn have(tool: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(tool);
        candidate.is_file()
    })
}

pub struct RunOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(program: &str, args: &[&str]) -> Option<RunOutput> {
    match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(output) => Some(RunOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }),
        Err(err) => {
            debug(&format!("run {program} {args:?} could not spawn: {err}"));
            None
        }
    }
}

pub fn run_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = run(program, args)?;
    if !output.success {
        debug(&format!("run {program} {args:?} failed: {}", output.stderr));
        return None;
    }
    Some(output.stdout)
}

pub fn run_ok(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn notify(title: &str, body: Option<&str>, urgency: &str) {
    if !have("omarchy-notification-send") {
        return;
    }
    let mut cmd = Command::new("omarchy-notification-send");
    cmd.arg("-u").arg(urgency).arg("-t").arg("5000");
    cmd.arg(title);
    if let Some(body) = body {
        cmd.arg(body);
    }
    let _ = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

pub fn osd(message: &str, duration_ms: u32) {
    if !have("omarchy-osd") {
        return;
    }
    let _ = Command::new("omarchy-osd")
        .arg("-m")
        .arg(message)
        .arg("-d")
        .arg(duration_ms.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

pub fn expand_home(raw: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    if raw == "~" {
        home
    } else if let Some(rest) = raw.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else {
        raw.to_string()
    }
}

pub fn user_videos_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_VIDEOS_DIR") {
        let dir = PathBuf::from(dir);
        if !dir.as_os_str().is_empty() {
            return dir;
        }
    }

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let user_dirs = home.join(".config/user-dirs.dirs");
    if let Ok(text) = std::fs::read_to_string(user_dirs) {
        for line in text.lines() {
            let line = line.trim();
            if let Some(value) = line.strip_prefix("XDG_VIDEOS_DIR=") {
                let value = value
                    .trim_matches('"')
                    .replace("$HOME", &home.display().to_string());
                if !value.is_empty() {
                    return PathBuf::from(value);
                }
            }
        }
    }
    home.join("Videos")
}

pub fn default_output_dir() -> PathBuf {
    user_videos_dir().join("Reel")
}

/// The shell treats every value it does not recognise as `off`.
pub fn is_on(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "enabled"
    )
}

pub fn normalize_switch(value: &str) -> bool {
    is_on(value)
}

pub fn pick_choice(value: &str, fallback: &str, allowed: &[&str]) -> String {
    if allowed.contains(&value) {
        value.to_string()
    } else {
        fallback.to_string()
    }
}

pub fn at_least_int(value: &str, minimum: i64) -> i64 {
    let parsed = value.trim().parse::<i64>().unwrap_or(minimum);
    parsed.max(minimum)
}

pub fn has_audio_stream(file: &Path) -> bool {
    let Some(out) = run_stdout(
        "ffprobe",
        &[
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "csv=p=0",
            &file.display().to_string(),
        ],
    ) else {
        return false;
    };
    out.contains("audio")
}

pub fn duration_of(file: &Path) -> f64 {
    let Some(out) = run_stdout(
        "ffprobe",
        &[
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
            &file.display().to_string(),
        ],
    ) else {
        return 0.0;
    };
    out.lines()
        .next()
        .and_then(|line| line.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
}

pub fn format_duration(total: i64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

pub fn notifications_on(paths: &Paths) -> bool {
    is_on(&crate::config::cfg(paths, "notifications", "off"))
}

pub fn file_stem_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Spawn the internal save pass without keeping the caller alive.
// The fields all come from one finished take; grouping them adds a type that
// would be constructed only here.
#[allow(clippy::too_many_arguments)]
pub fn spawn_finalize(
    paths: &Paths,
    file: &Path,
    slowmo: &str,
    preview: &std::path::Path,
    started_at: i64,
    target: &str,
    target_kind: &str,
    target_label: &str,
) -> std::io::Result<u32> {
    let exe = std::env::current_exe()?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log)?;
    let err = log.try_clone()?;
    let child = Command::new(exe)
        .arg("__finalize")
        .arg("--file")
        .arg(file)
        .arg("--slowmo")
        .arg(slowmo)
        .arg("--preview")
        .arg(preview)
        .arg("--started-at")
        .arg(started_at.to_string())
        .arg("--target")
        .arg(target)
        .arg("--target-kind")
        .arg(target_kind)
        .arg("--target-label")
        .arg(target_label)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err))
        .spawn()?;
    Ok(child.id())
}

pub fn debug(message: &str) {
    if std::env::var_os("REEL_DEBUG").is_some() {
        eprintln!("[reel] {message}");
    }
}
