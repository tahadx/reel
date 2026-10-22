use crate::util;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct Monitor {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub x: i64,
    #[serde(default)]
    pub y: i64,
    #[serde(default)]
    pub width: i64,
    #[serde(default)]
    pub height: i64,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub transform: i64,
    #[serde(default)]
    pub focused: bool,
}

fn one() -> f64 {
    1.0
}

#[derive(Deserialize, Debug, Clone, Default)]
struct Client {
    #[serde(default)]
    at: Vec<i64>,
    #[serde(default)]
    size: Vec<i64>,
    #[serde(default = "yes")]
    mapped: bool,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    class: String,
    #[serde(default)]
    title: String,
}

fn yes() -> bool {
    true
}

/// The target Reel will capture, plus the words the recording state uses to
/// describe it honestly.
#[derive(Debug, Clone)]
pub struct Choice {
    /// `monitor:NAME`, `region:WxH+X+Y`, or `portal`.
    pub target: String,
    /// Human summary, such as `Firefox · Inbox` or `DP-1 · 2560 × 1440`.
    pub label: String,
    /// A limitation the user must see before recording, or an empty string.
    pub warning: String,
    /// `monitor`, `window`, `region`, or `portal`.
    pub kind: String,
    pub monitor: String,
}

pub fn monitors() -> Vec<Monitor> {
    let Some(text) = util::run_stdout("hyprctl", &["monitors", "-j"]) else {
        util::debug("target: hyprctl monitors failed");
        return Vec::new();
    };
    match serde_json::from_str::<Vec<Monitor>>(&text) {
        Ok(monitors) => monitors,
        Err(err) => {
            util::debug(&format!("target: cannot parse monitors: {err}"));
            Vec::new()
        }
    }
}

fn clients() -> Vec<Client> {
    let Some(text) = util::run_stdout("hyprctl", &["clients", "-j"]) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<Client>>(&text).unwrap_or_default()
}

/// Accept the shapes the Omarchy picker prints and the forms the CLI accepts.
pub fn normalize(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(name) = raw.strip_prefix("monitor:") {
        if !name.is_empty() {
            return Some(format!("monitor:{name}"));
        }
        return None;
    }
    if raw == "portal" {
        return Some("portal".into());
    }
    if let Some(body) = raw.strip_prefix("region:") {
        return parse_geometry(body)
            .filter(|(w, h, _, _)| *w >= 16 && *h >= 16)
            .map(|(w, h, x, y)| format!("region:{w}x{h}+{x}+{y}"));
    }
    // `x,y WxH` — the Omarchy picker's shape.
    if let Some((pos, size)) = raw.split_once(char::is_whitespace) {
        let (x, y) = pos.split_once(',')?;
        let geometry = parse_size(size)?;
        let x = x.trim().parse::<i64>().ok()?;
        let y = y.trim().parse::<i64>().ok()?;
        if geometry.0 < 16 || geometry.1 < 16 {
            return None;
        }
        return Some(format!("region:{}x{}+{x}+{y}", geometry.0, geometry.1));
    }
    parse_geometry(raw)
        .filter(|(w, h, _, _)| *w >= 16 && *h >= 16)
        .map(|(w, h, x, y)| format!("region:{w}x{h}+{x}+{y}"))
}

fn parse_size(raw: &str) -> Option<(u64, u64)> {
    let (w, h) = raw.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// `WxH+X+Y`, with either offset allowed to be negative.
pub fn parse_geometry(raw: &str) -> Option<(u64, u64, i64, i64)> {
    let (w, rest) = raw.split_once('x')?;
    let width = w.parse::<u64>().ok()?;
    let plus = rest.find('+')?;
    let height = rest[..plus].parse::<u64>().ok()?;
    let mut tail = &rest[plus + 1..];
    let second = tail.find('+')?;
    let x = tail[..second].parse::<i64>().ok()?;
    tail = &tail[second + 1..];
    let y = tail.parse::<i64>().ok()?;
    Some((width, height, x, y))
}

pub fn focused_monitor() -> Option<String> {
    monitors()
        .into_iter()
        .find(|monitor| monitor.focused)
        .map(|monitor| monitor.name)
        .filter(|name| !name.is_empty())
}

fn monitor_pixels(monitor: &Monitor) -> (i64, i64) {
    if monitor.transform % 2 != 0 {
        (monitor.height, monitor.width)
    } else {
        (monitor.width, monitor.height)
    }
}

fn monitor_size(monitor: &Monitor) -> (i64, i64) {
    let scale = if monitor.scale > 0.0 {
        monitor.scale
    } else {
        1.0
    };
    let (width, height) = monitor_pixels(monitor);
    (
        (width as f64 / scale).floor() as i64,
        (height as f64 / scale).floor() as i64,
    )
}

fn physical_size(x: i64, y: i64, w: i64, h: i64) -> (i64, i64) {
    let cx = x + w / 2;
    let cy = y + h / 2;
    let scale = monitors()
        .into_iter()
        .find(|monitor| {
            let (width, height) = monitor_pixels(monitor);
            cx >= monitor.x && cx < monitor.x + width && cy >= monitor.y && cy < monitor.y + height
        })
        .map(|monitor| {
            if monitor.scale > 0.0 {
                monitor.scale
            } else {
                1.0
            }
        })
        .unwrap_or(1.0);
    // Match the compositor's boundary rounding: round the physical start and
    // end of the crop, then subtract. This is the size gpu-screen-recorder
    // produces for a logical picker rectangle.
    let left = (x as f64 * scale).round() as i64;
    let right = ((x + w) as f64 * scale).round() as i64;
    let top = (y as f64 * scale).round() as i64;
    let bottom = ((y + h) as f64 * scale).round() as i64;
    ((right - left).max(0), (bottom - top).max(0))
}

fn monitor_choice(name: &str) -> Result<Choice, String> {
    let monitor = monitors()
        .into_iter()
        .find(|monitor| monitor.name == name)
        .ok_or_else(|| {
            if name.is_empty() {
                "Reel could not read the monitor layout from Hyprland.".to_string()
            } else {
                format!("{name} is not connected. Choose another monitor.")
            }
        })?;
    let (width, height) = monitor_pixels(&monitor);
    if width <= 0 || height <= 0 {
        return Err(format!("{name} has no usable resolution."));
    }
    Ok(Choice {
        target: format!("monitor:{name}"),
        label: if monitor.focused {
            format!("{name} · {width} × {height} · focused")
        } else {
            format!("{name} · {width} × {height}")
        },
        warning: String::new(),
        kind: "monitor".into(),
        monitor: name.to_string(),
    })
}

fn excluded(class: &str, title: &str) -> bool {
    let class = class.to_ascii_lowercase();
    let title = title.to_ascii_lowercase();
    const CLASSES: &[&str] = &[
        "reel",
        "io.github.tahadx.reel",
        "rofi",
        "wofi",
        "fuzzel",
        "walker",
        "hyprlauncher",
        "omarchy-menu",
        "omarchy-launcher",
    ];
    CLASSES.iter().any(|needle| class.contains(needle))
        || title == "reel"
        || title.starts_with("reel ·")
        || title.starts_with("reel -")
}

fn window_label(class: &str, title: &str) -> String {
    let app = if class.is_empty() {
        "Window".to_string()
    } else {
        let mut chars = class.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => "Window".to_string(),
        }
    };
    if title.is_empty() {
        app
    } else {
        format!("{app} · {title}")
    }
}

fn centre_monitor(x: i64, y: i64, w: i64, h: i64) -> String {
    let cx = x + w / 2;
    let cy = y + h / 2;
    monitors()
        .into_iter()
        .find(|monitor| {
            let (width, height) = monitor_pixels(monitor);
            cx >= monitor.x && cx < monitor.x + width && cy >= monitor.y && cy < monitor.y + height
        })
        .map(|monitor| monitor.name)
        .unwrap_or_default()
}

fn client_for_rect(x: i64, y: i64, w: i64, h: i64) -> Option<Client> {
    clients().into_iter().find(|client| {
        client.mapped
            && !client.hidden
            && !excluded(&client.class, &client.title)
            && client.at.len() >= 2
            && client.size.len() >= 2
            && client.at[0] == x
            && client.at[1] == y
            && client.size[0] == w
            && client.size[1] == h
    })
}

fn choice_from_rect(x: i64, y: i64, w: i64, h: i64) -> Choice {
    let target = format!("region:{w}x{h}+{x}+{y}");
    let monitor = centre_monitor(x, y, w, h);
    let (pixel_width, pixel_height) = physical_size(x, y, w, h);
    match client_for_rect(x, y, w, h) {
        Some(client) => Choice {
            target,
            label: format!(
                "{} · {pixel_width} × {pixel_height}",
                window_label(&client.class, &client.title)
            ),
            warning: "Records the visible window bounds. Overlapping windows and window movement can appear in the recording.".into(),
            kind: "window".into(),
            monitor,
        },
        None => Choice {
            target,
            label: format!("Selected area · {pixel_width} × {pixel_height}"),
            warning: "Records the pixels inside the selected area. Overlapping windows can appear."
                .into(),
            kind: "region".into(),
            monitor,
        },
    }
}

/// Run the shared Omarchy picker. A cancelled pick is `Ok(None)`; a broken
/// picker is an error.
fn pick_with() -> Result<Option<String>, String> {
    if !util::have("omarchy-capture-region") {
        return Err("omarchy-capture-region is missing.".into());
    }
    let Some(result) = util::run("omarchy-capture-region", &["smart", "--match-monitor"]) else {
        return Err("The screen picker could not be started.".into());
    };
    // A non-zero exit with no output is a cancelled pick: stay quiet.
    if !result.success || result.stdout.is_empty() {
        return Ok(None);
    }
    match normalize(&result.stdout) {
        Some(target) => Ok(Some(target)),
        None => Err(
            "The picker returned a selection Reel could not read, or one that is too small.".into(),
        ),
    }
}

/// Open Omarchy's picker. A click snaps to the window or monitor under the
/// pointer; a drag selects an area.
pub fn pick() -> Result<Option<Choice>, String> {
    let Some(raw) = pick_with()? else {
        return Ok(None);
    };
    if let Some(name) = raw.strip_prefix("monitor:") {
        return monitor_choice(name).map(Some);
    }
    let (w, h, x, y) = parse_geometry(raw.strip_prefix("region:").unwrap_or(&raw))
        .ok_or_else(|| "The picker returned a selection Reel could not read.".to_string())?;
    if w < 16 || h < 16 {
        return Err("That selection is too small to record.".into());
    }
    Ok(Some(choice_from_rect(x, y, w as i64, h as i64)))
}

/// `(x, y, width, height)` in compositor coordinates.
pub fn geometry(target: &str) -> Option<(i64, i64, i64, i64)> {
    if let Some(body) = target.strip_prefix("region:") {
        let (w, h, x, y) = parse_geometry(body)?;
        return Some((x, y, w as i64, h as i64));
    }
    if let Some(name) = target.strip_prefix("monitor:") {
        let monitor = monitors().into_iter().find(|m| m.name == name)?;
        let (width, height) = monitor_size(&monitor);
        return Some((monitor.x, monitor.y, width, height));
    }
    None
}

/// Catch a monitor unplugged between the picker and the countdown, and a
/// region that no longer intersects any connected output.
pub fn validate_target(target: &str) -> Result<(), String> {
    if let Some(name) = target.strip_prefix("monitor:") {
        if monitors().iter().any(|monitor| monitor.name == name) {
            return Ok(());
        }
        return Err(format!(
            "{name} is no longer connected. Choose another monitor."
        ));
    }
    if target == "portal" {
        return Ok(());
    }
    let (w, h, x, y) = parse_geometry(target.strip_prefix("region:").unwrap_or(target))
        .ok_or_else(|| "The saved target is malformed. Choose it again.".to_string())?;
    if w < 16 || h < 16 {
        return Err("That target is too small to record. Choose it again.".into());
    }
    let cx = x + w as i64 / 2;
    let cy = y + h as i64 / 2;
    let connected = monitors().into_iter().any(|monitor| {
        let (width, height) = monitor_pixels(&monitor);
        cx >= monitor.x && cx < monitor.x + width && cy >= monitor.y && cy < monitor.y + height
    });
    if connected {
        Ok(())
    } else {
        Err(
            "The selected area is no longer on a connected monitor. Choose the target again."
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_negative_geometry() {
        assert_eq!(
            parse_geometry("1280x720+-100+-40"),
            Some((1280, 720, -100, -40))
        );
    }

    #[test]
    fn rejects_tiny_regions() {
        assert_eq!(normalize("1,2 4x4"), None);
        assert_eq!(normalize("region:4x4+1+2"), None);
    }

    #[test]
    fn keeps_monitor_targets() {
        assert_eq!(normalize("monitor:DP-1"), Some("monitor:DP-1".into()));
    }

    #[test]
    fn excludes_reel_and_launchers() {
        assert!(excluded("io.github.tahadx.reel", "Reel"));
        assert!(excluded("fuzzel", "launcher"));
        assert!(!excluded("firefox", "Inbox"));
    }
}
