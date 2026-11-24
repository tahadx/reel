//! The CPU capture fallback.
//!
//! Omarchy's own recorder prefers `gpu-screen-recorder` and records with
//! `wf-recorder` when it is missing or dies before the first frame. Reel keeps
//! the same order: a GPU take stays exactly as it is, and a machine without a
//! usable GPU recorder still gets a take. `wf-recorder` encodes on the CPU and
//! captures one PulseAudio source, so desktop and microphone audio are mixed
//! through a temporary null sink, and pause/resume is unavailable.

use crate::paths::{self, Paths};
use crate::target;
use crate::util;
use std::fs;
use std::time::Duration;

pub const BINARY: &str = "wf-recorder";

pub fn available() -> bool {
    util::have(BINARY)
}

/// One `--audio` source for wf-recorder, plus the requested sources that could
/// not be recorded so the caller can say why the take is quieter than asked.
#[derive(Debug, Default)]
pub struct Audio {
    pub device: Option<String>,
    pub missing: Vec<&'static str>,
}

/// Map the recorder's desktop device id onto a PulseAudio source. Reel stores
/// the ids `gpu-screen-recorder --list-audio-devices` prints, which are already
/// PipeWire node names (`<sink>.monitor`, `alsa_input...`).
fn resolve_desktop(device: &str, default_sink: &str) -> Option<String> {
    if device == "default_output" {
        if default_sink.is_empty() {
            return None;
        }
        return Some(format!("{default_sink}.monitor"));
    }
    // Application capture has no single source, and a combined device already
    // belongs to a running mix.
    if device.is_empty() || device.starts_with("app:") || device.contains('|') {
        return None;
    }
    if device.ends_with(".monitor") {
        Some(device.to_string())
    } else {
        Some(format!("{device}.monitor"))
    }
}

fn resolve_microphone(device: &str, default_source: &str) -> Option<String> {
    if device == "default_input" {
        // pactl answers its placeholder, never an empty string, when there is
        // no default source.
        if default_source.is_empty() || default_source == "@DEFAULT_SOURCE@" {
            return None;
        }
        return Some(default_source.to_string());
    }
    if device.is_empty() {
        None
    } else {
        Some(device.to_string())
    }
}

fn default_sink() -> String {
    util::run_stdout_within("pactl", &["get-default-sink"], Duration::from_secs(1))
        .unwrap_or_default()
}

fn default_source() -> String {
    util::run_stdout_within("pactl", &["get-default-source"], Duration::from_secs(1))
        .unwrap_or_default()
}

fn load_module(args: &[&str]) -> Option<String> {
    let id = util::run_stdout_within("pactl", args, Duration::from_secs(3))?;
    let id = id.trim();
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(id.to_string())
}

/// The module ids of the temporary mix, one per line in load order.
fn save_modules(paths: &Paths, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let body = ids.join("\n");
    let _ = paths::write_atomic(&paths.pa_modules, &body);
}

/// Unload the temporary null sink and its loopbacks. Unloading in reverse
/// drops the loopbacks before the sink they feed. Safe to call when no mix was
/// created, and from the separate `stop` process.
pub fn cleanup_audio(paths: &Paths) {
    let Ok(text) = fs::read_to_string(&paths.pa_modules) else {
        return;
    };
    for id in text
        .lines()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .rev()
    {
        let _ = util::run_stdout_within("pactl", &["unload-module", id], Duration::from_secs(2));
    }
    paths::remove_quiet(&paths.pa_modules);
}

/// Resolve the take's audio into the single source wf-recorder takes. `None`
/// disables a side; `Some(id)` is the device already resolved from settings.
/// With both desktop and microphone requested, a throwaway null sink carries
/// both in one track; its module ids are left in the runtime directory for
/// `stop`.
pub fn audio(paths: &Paths, desktop: Option<&str>, microphone: Option<&str>) -> Audio {
    if desktop.is_none() && microphone.is_none() {
        return Audio::default();
    }

    // A crashed start can leave the mix behind. Clear it before reusing the
    // sink name.
    cleanup_audio(paths);

    let sink = default_sink();
    let source = default_source();
    let desktop_source = desktop.and_then(|device| resolve_desktop(device, &sink));
    let microphone_source = microphone.and_then(|device| resolve_microphone(device, &source));
    let desktop = desktop.is_some();
    let microphone = microphone.is_some();

    let mut missing = Vec::new();
    if desktop && desktop_source.is_none() {
        missing.push("desktop");
    }
    if microphone && microphone_source.is_none() {
        missing.push("microphone");
    }

    match (desktop_source, microphone_source) {
        (Some(desktop_source), Some(microphone_source)) => {
            let sink_id = load_module(&[
                "load-module",
                "module-null-sink",
                "sink_name=reel_screenrec",
                "sink_properties=device.description=reel_screenrec",
            ]);
            let Some(sink_id) = sink_id else {
                // No device to mix into: desktop audio records alone, as it
                // does on the stock recorder.
                missing.push("microphone");
                return Audio {
                    device: Some(desktop_source),
                    missing,
                };
            };

            let mut ids = vec![sink_id];
            let mut desktop_ok = false;
            let mut microphone_ok = false;
            if let Some(id) = load_module(&[
                "load-module",
                "module-loopback",
                &format!("source={desktop_source}"),
                "sink=reel_screenrec",
                "latency_msec=20",
            ]) {
                ids.push(id);
                desktop_ok = true;
            }
            if let Some(id) = load_module(&[
                "load-module",
                "module-loopback",
                &format!("source={microphone_source}"),
                "sink=reel_screenrec",
                "latency_msec=20",
            ]) {
                ids.push(id);
                microphone_ok = true;
            }

            if desktop_ok || microphone_ok {
                save_modules(paths, &ids);
                if !desktop_ok {
                    missing.push("desktop");
                }
                if !microphone_ok {
                    missing.push("microphone");
                }
                return Audio {
                    device: Some("reel_screenrec.monitor".into()),
                    missing,
                };
            }

            cleanup_audio(paths);
            missing.push("desktop");
            missing.push("microphone");
            Audio {
                device: None,
                missing,
            }
        }
        (Some(desktop_source), None) => Audio {
            device: Some(desktop_source),
            missing,
        },
        (None, Some(microphone_source)) => Audio {
            device: Some(microphone_source),
            missing,
        },
        (None, None) => Audio {
            device: None,
            missing,
        },
    }
}

/// The capture arguments for a wf-recorder take. Geometry is passed in the
/// compositor's logical space, which is where the picker and Hyprland both
/// work.
pub struct Capture<'a> {
    pub target: &'a str,
    pub fps: i64,
    pub codec: &'a str,
    pub quality: &'a str,
    pub file: &'a str,
    pub audio: Option<&'a str>,
}

pub fn capture_args(capture: &Capture<'_>) -> Result<Vec<String>, String> {
    let mut args: Vec<String> = Vec::new();

    if let Some(name) = capture.target.strip_prefix("monitor:") {
        args.extend(["-o".into(), name.to_string()]);
    } else if let Some(body) = capture.target.strip_prefix("region:") {
        let (width, height, x, y) = target::parse_geometry(body)
            .ok_or_else(|| "That capture area could not be read.".to_string())?;
        args.extend(["-g".into(), format!("{x},{y} {width}x{height}")]);
    } else {
        return Err("wf-recorder cannot capture this target. Choose a monitor or an area.".into());
    }

    // -D captures every frame, not only damaged ones: wf-recorder reads a stop
    // signal only when a frame arrives, so a still screen would otherwise keep
    // recording until it is killed. -r pins the frame rate to the take's.
    args.extend([
        "-D".into(),
        "-r".into(),
        capture.fps.to_string(),
        "-R".into(),
        "48000".into(),
        "-C".into(),
        "aac".into(),
        "-y".into(),
    ]);

    let codec = match capture.codec {
        "hevc" => "libx265",
        "av1" => "libsvtav1",
        _ => "libx264",
    };
    args.extend(["-c".into(), codec.to_string()]);
    if codec != "libsvtav1" {
        // CPU fallback must stay usable: the slower presets turn a 4K take
        // into a slideshow on the machine that already lost hardware encoding.
        args.extend(["-p".into(), "preset=veryfast".into()]);
    }
    args.extend(["-p".into(), format!("crf={}", crf(capture.quality))]);

    if let Some(device) = capture.audio {
        args.push(format!("--audio={device}"));
    }

    args.extend(["-f".into(), capture.file.to_string()]);
    Ok(args)
}

/// Map Reel's quality presets onto libx264's CRF scale. The values keep the
/// same relative spacing as the GPU presets; the fallback trades speed for a
/// file the user can still share.
fn crf(quality: &str) -> u8 {
    match quality {
        "very_low" => 34,
        "low" => 30,
        "medium" => 26,
        "high" => 22,
        "ultra" => 16,
        _ => 19,
    }
}

/// Audio sources available to wf-recorder, in the same `id|label` shape as
/// `gpu-screen-recorder --list-audio-devices` so the panel can reuse its
/// picker.
pub fn audio_devices() -> Vec<(String, String, String)> {
    let mut devices = vec![
        (
            "default_output".into(),
            "Default output".into(),
            "output".into(),
        ),
        (
            "default_input".into(),
            "Default input".into(),
            "input".into(),
        ),
    ];
    let Some(sources) = util::run_stdout_within(
        "pactl",
        &["list", "short", "sources"],
        Duration::from_secs(2),
    ) else {
        return devices;
    };
    for line in sources.lines() {
        let mut fields = line.split('\t');
        let _index = fields.next();
        let Some(name) = fields.next().map(str::trim).filter(|name| !name.is_empty()) else {
            continue;
        };
        if name == "reel_screenrec.monitor" {
            continue;
        }
        let kind = if name.ends_with(".monitor") {
            "output"
        } else {
            "input"
        };
        devices.push((name.to_string(), name.to_string(), kind.to_string()));
    }
    devices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_default_desktop_audio_to_the_default_sink_monitor() {
        assert_eq!(
            resolve_desktop("default_output", "alsa_output.pci.HiFi__Speaker__sink"),
            Some("alsa_output.pci.HiFi__Speaker__sink.monitor".into())
        );
    }

    #[test]
    fn keeps_a_monitor_source_and_converts_a_bare_sink() {
        assert_eq!(
            resolve_desktop("alsa_output.sink.monitor", "ignored"),
            Some("alsa_output.sink.monitor".into())
        );
        assert_eq!(
            resolve_desktop("alsa_output.sink", "ignored"),
            Some("alsa_output.sink.monitor".into())
        );
    }

    #[test]
    fn refuses_application_capture_which_is_not_one_source() {
        assert_eq!(resolve_desktop("app:firefox|audio", "sink"), None);
        assert_eq!(resolve_desktop("default_output", ""), None);
    }

    #[test]
    fn maps_the_default_microphone_and_ignores_the_pactl_placeholder() {
        assert_eq!(
            resolve_microphone("default_input", "alsa_input.mic"),
            Some("alsa_input.mic".into())
        );
        assert_eq!(
            resolve_microphone("default_input", "@DEFAULT_SOURCE@"),
            None
        );
    }

    #[test]
    fn monitor_takes_pass_the_output_and_region_takes_the_geometry() {
        let monitor = capture_args(&Capture {
            target: "monitor:DP-1",
            fps: 60,
            codec: "h264",
            quality: "high",
            file: "/tmp/take.capture.mp4",
            audio: None,
        })
        .unwrap();
        assert_eq!(monitor[0], "-o");
        assert_eq!(monitor[1], "DP-1");
        assert!(monitor.windows(2).any(|pair| pair == ["-r", "60"]));
        assert!(monitor.contains(&"libx264".into()));
        assert!(!monitor.iter().any(|arg| arg.starts_with("--audio=")));

        let region = capture_args(&Capture {
            target: "region:1280x720+-100+-40",
            fps: 30,
            codec: "auto",
            quality: "very_high",
            file: "/tmp/take.capture.mkv",
            audio: Some("alsa_output.sink.monitor"),
        })
        .unwrap();
        assert_eq!(region[0], "-g");
        assert_eq!(region[1], "-100,-40 1280x720");
        assert!(region.contains(&"--audio=alsa_output.sink.monitor".into()));
    }

    #[test]
    fn portal_capture_is_not_available_on_the_cpu_backend() {
        let error = capture_args(&Capture {
            target: "portal",
            fps: 60,
            codec: "auto",
            quality: "high",
            file: "/tmp/take.mp4",
            audio: None,
        })
        .unwrap_err();
        assert!(error.contains("monitor or an area"));
    }

    #[test]
    fn crf_gets_lower_as_quality_rises() {
        assert!(crf("very_low") > crf("low"));
        assert!(crf("low") > crf("medium"));
        assert!(crf("medium") > crf("high"));
        assert!(crf("high") > crf("very_high"));
        assert!(crf("very_high") > crf("ultra"));
    }
}
