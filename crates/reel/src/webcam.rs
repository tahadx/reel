use crate::paths::{self, Paths};
use crate::target;
use crate::util;
use serde::Deserialize;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[derive(Deserialize)]
struct Client {
    #[serde(default)]
    title: String,
    #[serde(default)]
    address: String,
}

fn window_address() -> Option<String> {
    let text = util::run_stdout("hyprctl", &["clients", "-j"])?;
    let clients: Vec<Client> = serde_json::from_str(&text).ok()?;
    clients
        .into_iter()
        .find(|client| client.title == "WebcamOverlay")
        .map(|client| client.address)
        .filter(|address| !address.is_empty())
}

fn dispatch(lua: &str, legacy: [&str; 2]) {
    if !util::run_ok("hyprctl", &["dispatch", lua]) {
        let _ = util::run_ok("hyprctl", &["dispatch", legacy[0], legacy[1]]);
    }
}

pub fn cleanup(paths: &Paths) {
    if let Some(pid) = paths::read_pid(&paths.webcam_pid) {
        paths::kill(pid, libc::SIGTERM);
        // Poll briefly instead of always paying a fixed 100 ms on stop.
        for _ in 0..20 {
            if !paths::alive(pid) {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        if paths::alive(pid) {
            paths::kill(pid, libc::SIGKILL);
        }
    }
    paths::remove_quiet(&paths.webcam_pid);
}

/// Place the 8:9 camera in the bottom-right corner of the recorded rectangle.
/// Mirrors omarchy-capture-webcam-resize so manual resizes agree.
fn resize(target: &str, size: &str) {
    let Some(address) = window_address() else {
        return;
    };
    let Some((x, y, width, height)) = target::geometry(target) else {
        return;
    };
    let margin = 40;
    if width <= 2 * margin || height <= 2 * margin {
        return;
    }

    let mut scale_height = height;
    let available_width = width - 2 * margin;
    if scale_height * 3 / 10 > available_width {
        scale_height = available_width * 10 / 3;
    }

    let cam_height = match size {
        "small" => (scale_height * 9 + 25) / 50,
        "large" => (scale_height * 27 + 40) / 80,
        _ => (scale_height + 2) / 4,
    };
    let cam_width = (cam_height * 8 + 4) / 9;
    if cam_width <= 0 || cam_height <= 0 {
        return;
    }

    let mut cam_x = x + width - cam_width - margin;
    let mut cam_y = y + height - cam_height - margin;
    if cam_x < x + margin {
        cam_x = x + margin;
    }
    if cam_y < y + margin {
        cam_y = y + margin;
    }

    let window = format!("address:{address}");
    dispatch(
        &format!(
            "hl.dsp.window.resize({{ window = \"{window}\", x = {cam_width}, y = {cam_height} }})"
        ),
        [
            "resizewindowpixel",
            &format!("exact {cam_width} {cam_height},{window}"),
        ],
    );
    dispatch(
        &format!("hl.dsp.window.move({{ window = \"{window}\", x = {cam_x}, y = {cam_y} }})"),
        [
            "movewindowpixel",
            &format!("exact {cam_x} {cam_y},{window}"),
        ],
    );
}

fn device(explicit: &str) -> Option<String> {
    if !explicit.is_empty() {
        return Some(explicit.to_string());
    }
    if util::have("omarchy-capture-webcam-list") {
        if let Some(list) = util::run_stdout("omarchy-capture-webcam-list", &[]) {
            if let Some(first) = list.lines().next() {
                if let Some(id) = first.split_whitespace().next() {
                    if !id.is_empty() {
                        return Some(id.to_string());
                    }
                }
            }
        }
    }
    let list = util::run_stdout("gpu-screen-recorder", &["--list-v4l2-devices"])?;
    list.lines()
        .next()
        .and_then(|line| line.split('|').next())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

pub fn start(paths: &Paths, target: &str, size: &str, explicit_device: &str) -> bool {
    if size == "off" || size.is_empty() {
        return true;
    }
    if !util::have("mpv") {
        util::notify(
            "Reel",
            Some("mpv is missing, so the webcam overlay cannot start."),
            "critical",
        );
        return false;
    }

    let Some(device) = device(explicit_device) else {
        util::notify("Reel", Some("No webcam was found."), "critical");
        return false;
    };

    cleanup(paths);

    let mut capture_options = "framerate=30".to_string();
    if let Some(formats) = util::run_stdout("v4l2-ctl", &["--list-formats-ext", "-d", &device]) {
        for resolution in ["640x360", "1280x720", "1920x1080"] {
            if formats.contains(resolution) {
                capture_options = format!("video_size={resolution},{capture_options}");
                break;
            }
        }
    }

    let demuxer = format!("--demuxer-lavf-o={capture_options}");
    let app_id = format!("--wayland-app-id=WebcamOverlay-{size}");
    let child = Command::new("mpv")
        .arg(format!("av://v4l2:{device}"))
        .arg("--profile=low-latency")
        .arg("--untimed")
        .arg("--no-cache")
        .arg(demuxer)
        .arg("--vf=lavfi=[crop=ih*8/9:ih]")
        .arg("--title=WebcamOverlay")
        .arg(app_id)
        .arg("--no-border")
        .arg("--no-audio")
        .arg("--no-osc")
        .arg("--osd-level=0")
        .arg("--really-quiet")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    let Ok(child) = child else {
        return false;
    };
    let _ = paths::write_pid(&paths.webcam_pid, child.id());

    let mut found = false;
    for _ in 0..60 {
        if window_address().is_some() {
            found = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    if !found {
        cleanup(paths);
        return false;
    }

    resize(target, size);
    thread::sleep(Duration::from_millis(350));
    resize(target, size);
    true
}
