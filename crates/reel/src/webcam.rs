use crate::paths::{self, Paths};
use crate::target;
use crate::util;
use serde::Deserialize;
use std::fs;
use std::path::Path;
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

/// A camera Reel can record. `id` is what `webcamDevice` stores: the
/// `/dev/v4l/by-id` link when udev made one, because `/dev/videoN` numbers
/// move when cameras are unplugged and plugged back in. `path` is the
/// `/dev/videoN` node it resolves to today, so older configs that stored the
/// node still match. `label` is the human name, falling back to the id.
pub struct Camera {
    pub id: String,
    pub path: String,
    pub label: String,
}

const BY_ID: &str = "/dev/v4l/by-id";

pub fn devices() -> Vec<Camera> {
    discover()
        .into_iter()
        .map(|(path, label)| {
            let id = stable_id(Path::new(BY_ID), &path);
            // A nameless camera is labelled with its path; prefer the stable one.
            let label = if label == path { id.clone() } else { label };
            Camera { id, path, label }
        })
        .collect()
}

/// `(node, label)` pairs from the tools Omarchy ships, in order of preference.
fn discover() -> Vec<(String, String)> {
    if util::have("omarchy-capture-webcam-list") {
        if let Some(list) = util::run_stdout("omarchy-capture-webcam-list", &[]) {
            let found = parse_omarchy_list(&list);
            if !found.is_empty() {
                return found;
            }
        }
    }
    let Some(list) = util::run_stdout("gpu-screen-recorder", &["--list-v4l2-devices"]) else {
        return Vec::new();
    };
    parse_gsr_list(&list)
}

/// The first link in `dir` that resolves to `node`, or `node` itself.
fn stable_id(dir: &Path, node: &str) -> String {
    let Ok(target) = fs::canonicalize(node) else {
        return node.to_string();
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return node.to_string();
    };
    let mut links: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|link| fs::canonicalize(link).is_ok_and(|resolved| resolved == target))
        .collect();
    links.sort();
    links
        .first()
        .map(|link| link.to_string_lossy().into_owned())
        .unwrap_or_else(|| node.to_string())
}

/// `omarchy-capture-webcam-list` prints `"/dev/video0  Some Camera: (usb-...)"`.
fn parse_omarchy_list(list: &str) -> Vec<(String, String)> {
    list.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, char::is_whitespace);
            let id = parts.next()?.trim();
            if id.is_empty() {
                return None;
            }
            let label = parts.next().unwrap_or("").trim().trim_end_matches(':');
            let label = label.split(':').next().unwrap_or("").trim();
            Some((
                id.to_string(),
                if label.is_empty() {
                    id.to_string()
                } else {
                    label.to_string()
                },
            ))
        })
        .collect()
}

/// `gpu-screen-recorder --list-v4l2-devices` prints `"/dev/video0|Some Camera"`.
fn parse_gsr_list(list: &str) -> Vec<(String, String)> {
    list.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '|');
            let id = parts.next()?.trim();
            if id.is_empty() {
                return None;
            }
            let label = parts.next().unwrap_or("").trim();
            Some((
                id.to_string(),
                if label.is_empty() {
                    id.to_string()
                } else {
                    label.to_string()
                },
            ))
        })
        .collect()
}

/// The camera to open. A chosen camera that is not plugged in falls back to
/// the first one found, with a notice, so the take still gets a webcam.
fn device(explicit: &str) -> Option<String> {
    if !explicit.is_empty() && Path::new(explicit).exists() {
        return Some(explicit.to_string());
    }
    let first = devices().into_iter().next()?;
    if !explicit.is_empty() {
        util::notify(
            "Reel",
            Some(&format!(
                "The chosen webcam is not connected, so Reel is using {}.",
                first.label
            )),
            "normal",
        );
    }
    Some(first.id)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_omarchy_list_keeps_the_path_and_the_human_name() {
        let list = "/dev/video0  Laptop Webcam Module (2nd Gen): (usb-0000:c3:00.0-3):\n\
                    /dev/video2  Insta360 Link 2C Pro: Insta360  (usb-0000:c3:00.3-1):\n";
        assert_eq!(
            parse_omarchy_list(list),
            vec![
                (
                    "/dev/video0".to_string(),
                    "Laptop Webcam Module (2nd Gen)".to_string()
                ),
                (
                    "/dev/video2".to_string(),
                    "Insta360 Link 2C Pro".to_string()
                ),
            ]
        );
    }

    #[test]
    fn a_nameless_omarchy_entry_falls_back_to_its_path() {
        assert_eq!(
            parse_omarchy_list("/dev/video0\n"),
            vec![("/dev/video0".to_string(), "/dev/video0".to_string())]
        );
    }

    #[test]
    fn blank_lines_are_not_cameras() {
        assert!(parse_omarchy_list("\n   \n").is_empty());
        assert!(parse_gsr_list("\n").is_empty());
    }

    #[test]
    fn the_gsr_list_splits_on_the_pipe() {
        assert_eq!(
            parse_gsr_list("/dev/video0|Laptop Webcam\n/dev/video2|Insta360 Link 2C Pro\n"),
            vec![
                ("/dev/video0".to_string(), "Laptop Webcam".to_string()),
                (
                    "/dev/video2".to_string(),
                    "Insta360 Link 2C Pro".to_string()
                ),
            ]
        );
    }

    #[test]
    fn a_node_with_a_by_id_link_gets_the_stable_id() {
        let dir = std::env::temp_dir().join(format!("reel-by-id-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let node = dir.join("video2");
        fs::write(&node, "").unwrap();
        std::os::unix::fs::symlink(&node, dir.join("usb-Cam-video-index0")).unwrap();
        let other = dir.join("video4");
        fs::write(&other, "").unwrap();

        let node = node.to_string_lossy().into_owned();
        let other = other.to_string_lossy().into_owned();
        assert_eq!(
            stable_id(&dir, &node),
            dir.join("usb-Cam-video-index0").to_string_lossy()
        );
        assert_eq!(stable_id(&dir, &other), other);
        assert_eq!(stable_id(&dir.join("missing"), &node), node);
        fs::remove_dir_all(&dir).unwrap();
    }
}
