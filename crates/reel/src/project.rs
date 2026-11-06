//! The `project.json` v1 contract.
//!
//! A save writes this file next to the take when the take has telemetry.
//! `reel render` reads it and never re-derives the look from live settings, so
//! a project can be edited by hand and rendered again without opening the
//! panel. Unknown fields are kept when a project is loaded and written back.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub const PROJECT_VERSION: u32 = 1;

/// Clicks closer than this in time become one zoom block.
pub const CLUSTER_GAP_SECS: f64 = 2.5;
/// Clicks closer than this to the running centroid become one zoom block.
pub const CLUSTER_RADIUS_PX: f64 = 350.0;
/// Time spent easing into a zoom before the first click of a block.
pub const EASE_IN_SECS: f64 = 0.5;
/// Zoom is held for this long after the last click of a block.
pub const HOLD_AFTER_LAST_SECS: f64 = 0.9;
/// Time spent easing back to 1x after the hold.
pub const EASE_OUT_SECS: f64 = 0.5;
/// The zoom depths the contract allows.
pub const DEPTHS: [f64; 3] = [1.5, 2.0, 2.5];
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub version: u32,
    /// File name of the saved take, relative to the project unless absolute.
    pub take: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "trimStart")]
    pub trim_start: f64,
    pub source: Source,
    pub capture: Capture,
    /// Telemetry file name, relative to the project, or empty.
    pub telemetry: String,
    pub camera: Camera,
    pub cursor: Cursor,
    pub ripples: Ripples,
    pub canvas: Canvas,
    pub webcam: Webcam,
    pub exports: Vec<Export>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Project {
    fn default() -> Self {
        Project {
            version: PROJECT_VERSION,
            take: String::new(),
            created_at: String::new(),
            trim_start: 0.0,
            source: Source::default(),
            capture: Capture::default(),
            telemetry: String::new(),
            camera: Camera::default(),
            cursor: Cursor::default(),
            ripples: Ripples::default(),
            canvas: Canvas::default(),
            webcam: Webcam::default(),
            exports: Vec::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Source {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: f64,
    #[serde(rename = "frameCount")]
    pub frame_count: u64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Source {
    fn default() -> Self {
        Source {
            width: 0,
            height: 0,
            fps: 60.0,
            duration: 0.0,
            frame_count: 0,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Capture {
    pub kind: String,
    pub target: String,
    pub monitor: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Capture {
    fn default() -> Self {
        Capture {
            kind: String::new(),
            target: String::new(),
            monitor: String::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Camera {
    /// `off`, `subtle`, `cinematic`, or `close`.
    pub style: String,
    pub depth: f64,
    pub easing: String,
    pub keyframes: Vec<Keyframe>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            style: "off".into(),
            depth: 2.0,
            easing: "smoothstep".into(),
            keyframes: Vec::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Keyframe {
    /// Saved-take time in seconds.
    pub t: f64,
    pub zoom: f64,
    /// Viewport centre in source capture pixels.
    pub x: f64,
    pub y: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Keyframe {
    fn default() -> Self {
        Keyframe {
            t: 0.0,
            zoom: 1.0,
            x: 0.0,
            y: 0.0,
            extra: Map::new(),
        }
    }
}

impl Keyframe {
    pub fn new(t: f64, zoom: f64, x: f64, y: f64) -> Keyframe {
        Keyframe {
            t,
            zoom,
            x,
            y,
            extra: Map::new(),
        }
    }

    /// Keep the viewport inside a `width` by `height` source.
    pub fn clamp_center(&mut self, width: f64, height: f64) {
        if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
            return;
        }
        if !self.zoom.is_finite() || self.zoom < 1.0 {
            self.zoom = 1.0;
        }
        if !self.x.is_finite() {
            self.x = width / 2.0;
        }
        if !self.y.is_finite() {
            self.y = height / 2.0;
        }
        let half_w = width / (2.0 * self.zoom);
        let half_h = height / (2.0 * self.zoom);
        self.x = if width >= half_w * 2.0 {
            self.x.clamp(half_w, width - half_w)
        } else {
            width / 2.0
        };
        self.y = if height >= half_h * 2.0 {
            self.y.clamp(half_h, height - half_h)
        } else {
            height / 2.0
        };
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cursor {
    pub enabled: bool,
    pub size: String,
    #[serde(rename = "hideIdle")]
    pub hide_idle: bool,
    pub smooth: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Cursor {
    fn default() -> Self {
        Cursor {
            enabled: false,
            size: "medium".into(),
            hide_idle: true,
            smooth: true,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ripples {
    pub enabled: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Ripples {
    fn default() -> Self {
        Ripples {
            enabled: false,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Canvas {
    pub background: String,
    pub padding: String,
    pub radius: u32,
    pub shadow: String,
    pub image: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Canvas {
    fn default() -> Self {
        Canvas {
            background: "theme".into(),
            padding: "medium".into(),
            radius: 24,
            shadow: "medium".into(),
            image: String::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Webcam {
    pub enabled: bool,
    pub file: String,
    pub size: String,
    pub corner: String,
    pub shape: String,
    pub mirror: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Webcam {
    fn default() -> Self {
        Webcam {
            enabled: false,
            file: String::new(),
            size: "medium".into(),
            corner: "bottom_right".into(),
            shape: "circle".into(),
            mirror: false,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Export {
    pub aspect: String,
    pub width: u32,
    pub height: u32,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Export {
    fn default() -> Self {
        Export {
            aspect: "16:9".into(),
            width: 1920,
            height: 1080,
            extra: Map::new(),
        }
    }
}

/// One click in telemetry time. The telemetry module owns the wire format;
/// auto zoom only needs the time and the capture-local position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Click {
    pub t: f64,
    pub x: f64,
    pub y: f64,
}

/// The parts of a telemetry file the save pass needs: the clicks that drive the
/// camera and the `start` facts that fill the project's capture block.
#[derive(Debug, Clone, PartialEq)]
pub struct Telemetry {
    pub clicks: Vec<Click>,
    pub capture: Capture,
    /// Encoded video dimensions from the `start` record. `0` when absent.
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// End of the recording in telemetry time. `0` when no `stop` record.
    pub stop_t: f64,
}

/// Read a telemetry file into the clicks and capture facts the camera needs.
///
/// Malformed lines are skipped rather than failing the save: a truncated log
/// still has usable clicks, and a take must never be lost to a bad telemetry
/// line. A file with no `start` record still yields its clicks.
pub fn read_telemetry(path: &Path) -> Result<Telemetry> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("cannot read the telemetry at {}", path.display()))?;
    let mut telemetry = Telemetry {
        clicks: Vec::new(),
        capture: Capture::default(),
        width: 0,
        height: 0,
        fps: 0.0,
        stop_t: 0.0,
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match record.get("type").and_then(Value::as_str) {
            Some("start") => {
                telemetry.width = number(&record, "capture", "width");
                telemetry.height = number(&record, "capture", "height");
                telemetry.fps = record.get("fps").and_then(Value::as_f64).unwrap_or(0.0);
                if let Some(capture) = record.get("capture").and_then(Value::as_object) {
                    telemetry.capture = Capture {
                        kind: string(capture, "kind"),
                        target: string(capture, "target"),
                        monitor: string(capture, "monitor"),
                        extra: Map::new(),
                    };
                }
            }
            Some("button") => {
                if record.get("state").and_then(Value::as_str) != Some("down") {
                    continue;
                }
                let Some(t) = record.get("t").and_then(Value::as_f64) else {
                    continue;
                };
                let Some(x) = record.get("x").and_then(Value::as_f64) else {
                    continue;
                };
                let Some(y) = record.get("y").and_then(Value::as_f64) else {
                    continue;
                };
                telemetry.clicks.push(Click { t, x, y });
            }
            Some("stop") => {
                telemetry.stop_t = record.get("t").and_then(Value::as_f64).unwrap_or(0.0);
            }
            _ => {}
        }
    }
    Ok(telemetry)
}

fn number(record: &Value, object: &str, key: &str) -> u32 {
    record
        .get(object)
        .and_then(|value| value.get(key))
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| value.round() as u32)
        .unwrap_or(0)
}

fn string(record: &Map<String, Value>, key: &str) -> String {
    record
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// The camera for a project, built from clicks.
///
/// The save pass feeds this from the take's telemetry; `reel render` then reads
/// the resulting keyframes from `project.json`. Auto zoom is one on/off choice,
/// so `enabled` is the only mode there is; `depth` is the magnification.
pub fn build_camera(
    clicks: &[Click],
    enabled: bool,
    depth: f64,
    width: f64,
    height: f64,
    trim_start: f64,
) -> Camera {
    let style = if enabled { "auto" } else { "off" };
    let mut camera = Camera {
        style: style.into(),
        depth: valid_depth(depth),
        easing: "smoothstep".into(),
        keyframes: Vec::new(),
        extra: Map::new(),
    };

    if !enabled {
        return camera;
    }

    // Telemetry time is raw take time; keyframes use saved-take time.
    let trim = trim_start.max(0.0);
    let mut clicks: Vec<Click> = clicks
        .iter()
        .filter(|click| {
            click.t.is_finite()
                && click.x.is_finite()
                && click.y.is_finite()
                && click.x >= 0.0
                && click.x < width
                && click.y >= 0.0
                && click.y < height
        })
        .map(|click| Click {
            t: (click.t - trim).max(0.0),
            x: click.x,
            y: click.y,
        })
        .collect();
    clicks.sort_by(|a, b| a.t.total_cmp(&b.t));

    if clicks.is_empty() {
        return camera;
    }

    // One block per click cluster. A block is active from its ease-in until
    // its ease-out finishes.
    struct Block {
        start: f64,
        enter: f64,
        hold: f64,
        end: f64,
        cx: f64,
        cy: f64,
    }

    let blocks: Vec<Block> = cluster_clicks(&clicks)
        .iter()
        .map(|cluster| {
            let first = cluster[0];
            let last = *cluster.last().unwrap();
            let count = cluster.len() as f64;
            let enter = first.t.max(0.0);
            let hold = (last.t + HOLD_AFTER_LAST_SECS).max(enter);
            Block {
                start: (enter - EASE_IN_SECS).max(0.0),
                enter,
                hold,
                end: hold + EASE_OUT_SECS,
                cx: cluster.iter().map(|click| click.x).sum::<f64>() / count,
                cy: cluster.iter().map(|click| click.y).sum::<f64>() / count,
            }
        })
        .collect();

    // Blocks whose eased windows overlap would cut back to 1x and snap in
    // again a millisecond later. Stay zoomed and pan between their centres
    // instead, then leave once when the last of them is done.
    let (centre_x, centre_y) = (width / 2.0, height / 2.0);
    let mut frames: Vec<Keyframe> = Vec::new();
    let mut index = 0;
    while index < blocks.len() {
        let first = &blocks[index];
        push_frame(
            &mut frames,
            Keyframe::new(first.start, 1.0, centre_x, centre_y),
        );
        let mut enter = first.enter.max(first.start + 0.001);
        push_frame(
            &mut frames,
            Keyframe::new(enter, camera.depth, first.cx, first.cy),
        );

        let mut last = index;
        while last + 1 < blocks.len() && blocks[last + 1].start <= blocks[last].end {
            last += 1;
            let next = &blocks[last];
            enter = next.enter.max(enter + 0.001);
            push_frame(
                &mut frames,
                Keyframe::new(enter, camera.depth, next.cx, next.cy),
            );
        }

        let final_block = &blocks[last];
        let hold = final_block.hold.max(enter + 0.001);
        push_frame(
            &mut frames,
            Keyframe::new(hold, camera.depth, final_block.cx, final_block.cy),
        );
        push_frame(
            &mut frames,
            Keyframe::new(hold + EASE_OUT_SECS, 1.0, centre_x, centre_y),
        );
        index = last + 1;
    }

    camera.keyframes = frames;
    camera.normalize(width, height);
    camera
}

fn push_frame(frames: &mut Vec<Keyframe>, frame: Keyframe) {
    if let Some(last) = frames.last() {
        if (last.t - frame.t).abs() < 1e-9 {
            return;
        }
    }
    frames.push(frame);
}

/// Greedy time-ordered clustering: a click joins the open cluster when it is
/// within `CLUSTER_GAP_SECS` of the previous click and within
/// `CLUSTER_RADIUS_PX` of the running centroid.
pub fn cluster_clicks(clicks: &[Click]) -> Vec<Vec<Click>> {
    let mut sorted: Vec<Click> = clicks
        .iter()
        .copied()
        .filter(|click| click.t.is_finite() && click.x.is_finite() && click.y.is_finite())
        .collect();
    sorted.sort_by(|a, b| a.t.total_cmp(&b.t));

    let mut clusters: Vec<Vec<Click>> = Vec::new();
    let mut centroid = (0.0, 0.0);
    for click in sorted {
        let joins = clusters.last().and_then(|cluster| {
            let last = cluster.last()?;
            let close_in_time = click.t - last.t <= CLUSTER_GAP_SECS + 1e-9;
            let dx = click.x - centroid.0;
            let dy = click.y - centroid.1;
            let close_in_space = (dx * dx + dy * dy).sqrt() <= CLUSTER_RADIUS_PX + 1e-9;
            Some(close_in_time && close_in_space)
        });
        if joins == Some(true) {
            let cluster = clusters.last_mut().expect("checked above");
            cluster.push(click);
            let count = cluster.len() as f64;
            centroid.0 += (click.x - centroid.0) / count;
            centroid.1 += (click.y - centroid.1) / count;
        } else {
            clusters.push(vec![click]);
            centroid = (click.x, click.y);
        }
    }
    clusters
}

impl Camera {
    /// True when at least one keyframe actually magnifies the frame.
    pub fn needs_zoom(&self) -> bool {
        self.style != "off" && self.keyframes.iter().any(|frame| frame.zoom > 1.0 + 1e-9)
    }

    /// Sort keyframes, enforce zoom >= 1, and clamp every centre to the source.
    pub fn normalize(&mut self, width: f64, height: f64) {
        self.style = valid_style(&self.style).to_string();
        self.depth = valid_depth(self.depth);
        self.easing = "smoothstep".into();
        let mut frames: Vec<Keyframe> = self
            .keyframes
            .drain(..)
            .filter(|frame| frame.t.is_finite() && frame.zoom.is_finite())
            .collect();
        frames.sort_by(|a, b| a.t.total_cmp(&b.t));
        let mut folded: Vec<Keyframe> = Vec::with_capacity(frames.len());
        for mut frame in frames {
            frame.t = frame.t.max(0.0);
            frame.zoom = frame.zoom.max(1.0);
            frame.clamp_center(width, height);
            if let Some(last) = folded.last_mut() {
                if (last.t - frame.t).abs() < 1e-9 {
                    // The last keyframe at a timestamp wins; hand edits often
                    // append a correction at the same time.
                    *last = frame;
                    continue;
                }
            }
            folded.push(frame);
        }
        self.keyframes = folded;
    }

    /// The eased viewport at saved-take time `t`. Interpolation is smoothstep,
    /// so velocity is zero at every keyframe. The centre is clamped again after
    /// interpolation because a lower zoom between keyframes widens the
    /// viewport.
    pub fn viewport_at(&self, t: f64, width: f64, height: f64) -> (f64, f64, f64) {
        if self.keyframes.is_empty() {
            return (1.0, width / 2.0, height / 2.0);
        }
        if t <= self.keyframes[0].t {
            let frame = &self.keyframes[0];
            let mut copy = frame.clone();
            copy.clamp_center(width, height);
            return (copy.zoom, copy.x, copy.y);
        }
        for pair in self.keyframes.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if t <= b.t {
                let span = b.t - a.t;
                let u = if span > 0.0 {
                    smoothstep((t - a.t) / span)
                } else {
                    1.0
                };
                let zoom = a.zoom + (b.zoom - a.zoom) * u;
                let x = a.x + (b.x - a.x) * u;
                let y = a.y + (b.y - a.y) * u;
                let mut frame = Keyframe::new(t, zoom, x, y);
                frame.clamp_center(width, height);
                return (frame.zoom, frame.x, frame.y);
            }
        }
        let frame = self.keyframes.last().expect("not empty");
        let mut copy = frame.clone();
        copy.clamp_center(width, height);
        (copy.zoom, copy.x, copy.y)
    }
}

pub fn smoothstep(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// A camera style from the contract, or `off` for anything else. Projects
/// written before the modes were folded away carry `subtle`, `cinematic`, or
/// `close`; all of them mean the single automatic camera that is on.
pub fn valid_style(style: &str) -> &'static str {
    match style {
        "auto" | "subtle" | "cinematic" | "close" => "auto",
        _ => "off",
    }
}

pub fn valid_depth(depth: f64) -> f64 {
    DEPTHS
        .iter()
        .copied()
        .find(|allowed| (allowed - depth).abs() < 1e-9)
        .unwrap_or(2.0)
}

/// `<take-stem>.project.json`, next to the take.
pub fn project_path(take: &Path) -> PathBuf {
    let parent = take.parent().unwrap_or_else(|| Path::new(""));
    let stem = take
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default();
    parent.join(format!("{stem}.project.json"))
}

/// `<take-stem>.rendered.<ext>`, next to the take.
pub fn rendered_path(take: &Path) -> PathBuf {
    let parent = take.parent().unwrap_or_else(|| Path::new(""));
    let stem = take
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = take
        .extension()
        .map(|ext| ext.to_string_lossy().to_string())
        .unwrap_or_default();
    if ext.is_empty() {
        parent.join(format!("{stem}.rendered"))
    } else {
        parent.join(format!("{stem}.rendered.{ext}"))
    }
}

impl Project {
    pub fn load(path: &Path) -> Result<Project> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("cannot read the project at {}", path.display()))?;
        let mut project: Project = serde_json::from_str(&text)
            .with_context(|| format!("cannot parse the project at {}", path.display()))?;
        if project.version != PROJECT_VERSION {
            bail!(
                "unsupported project version {} at {} (expected {})",
                project.version,
                path.display(),
                PROJECT_VERSION
            );
        }
        project.normalize();
        Ok(project)
    }

    pub fn save(path: &Path, project: &Project) -> Result<()> {
        let body = serde_json::to_string_pretty(project)?;
        crate::paths::write_atomic(path, &body)
    }

    pub fn normalize(&mut self) {
        if !self.trim_start.is_finite() || self.trim_start < 0.0 {
            self.trim_start = 0.0;
        }
        self.camera
            .normalize(self.source.width as f64, self.source.height as f64);
    }

    /// The saved take this project points at.
    pub fn take_path(&self, project_path: &Path) -> PathBuf {
        let take = Path::new(&self.take);
        if take.is_absolute() {
            take.to_path_buf()
        } else {
            project_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(take)
        }
    }

    /// A project with the v1 defaults filled in. The save pass adds the capture
    /// block, the telemetry name, and the camera.
    pub fn for_take(take: &Path, width: u32, height: u32, fps: f64, duration: f64) -> Project {
        Project {
            take: take
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default(),
            created_at: iso8601_now(),
            source: Source {
                width,
                height,
                fps,
                duration,
                frame_count: (duration * fps).round().max(0.0) as u64,
                extra: Map::new(),
            },
            ..Project::default()
        }
    }
}

fn iso8601_now() -> String {
    // SAFETY: gmtime_r writes into a tm owned by this call and both pointers
    // are valid for its whole duration.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::gmtime_r(&now, &mut tm);
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(t: f64, x: f64, y: f64) -> Click {
        Click { t, x, y }
    }

    fn telemetry_fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/telemetry-basic.jsonl")
    }

    #[test]
    fn telemetry_reader_keeps_downs_and_the_capture_block() {
        let telemetry = read_telemetry(&telemetry_fixture()).unwrap();
        assert_eq!(telemetry.width, 2560);
        assert_eq!(telemetry.height, 1440);
        assert_eq!(telemetry.fps, 60.0);
        assert_eq!(telemetry.capture.kind, "monitor");
        assert_eq!(telemetry.capture.target, "monitor:DP-1");
        assert_eq!(telemetry.capture.monitor, "DP-1");
        assert_eq!(telemetry.stop_t, 10.0);
        // One `down` in the fixture: the `up` state is not a click, and the
        // `move` and `sync` records carry no click.
        assert_eq!(telemetry.clicks.len(), 1);
        assert_eq!(telemetry.clicks[0].t, 1.5);
        assert_eq!(telemetry.clicks[0].x, 100.5);
        assert_eq!(telemetry.clicks[0].y, 200.25);
    }

    #[test]
    fn telemetry_reader_skips_broken_lines() {
        let dir = std::env::temp_dir().join(format!("reel-telemetry-{}", std::process::id()));
        let path = dir.join("take.telemetry.jsonl");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &path,
            concat!(
                "{\"v\":1,\"type\":\"start\",\"t\":0.0,\"capture\":{\"kind\":\"window\",\"target\":\"window:1\",\"monitor\":\"DP-2\",\"width\":1920,\"height\":1080},\"fps\":30}\n",
                "not json\n",
                "\n",
                "{\"v\":1,\"type\":\"button\",\"t\":3.0,\"state\":\"down\",\"x\":10.0}\n",
                "{\"v\":1,\"type\":\"button\",\"t\":4.0,\"state\":\"down\",\"x\":10.0,\"y\":20.0}\n"
            ),
        )
        .unwrap();

        let telemetry = read_telemetry(&path).unwrap();
        assert_eq!(telemetry.capture.kind, "window");
        assert_eq!(telemetry.width, 1920);
        assert_eq!(telemetry.fps, 30.0);
        // The record without `y` is dropped; the valid one survives.
        assert_eq!(telemetry.clicks, vec![click(4.0, 10.0, 20.0)]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn telemetry_reader_fails_on_a_missing_file() {
        let missing = std::env::temp_dir().join("reel-telemetry-does-not-exist.jsonl");
        assert!(read_telemetry(&missing).is_err());
    }

    #[test]
    fn project_roundtrip_preserves_unknown_fields() {
        let text = r#"{
            "version": 1,
            "take": "reel-1.mp4",
            "createdAt": "2026-11-03T12:00:00Z",
            "source": { "width": 2560, "height": 1440, "fps": 60, "duration": 12.345, "frameCount": 741 },
            "capture": { "kind": "monitor", "target": "monitor:DP-1", "monitor": "DP-1" },
            "telemetry": "reel-1.telemetry.jsonl",
            "camera": {
                "style": "cinematic", "depth": 2.0, "easing": "smoothstep",
                "futureCameraOption": true,
                "keyframes": [{ "t": 0.0, "zoom": 1.0, "x": 1280.0, "y": 720.0, "extraKf": 7 }]
            },
            "cursor": { "enabled": false, "size": "medium", "hideIdle": true, "smooth": true },
            "ripples": { "enabled": false },
            "canvas": { "background": "theme", "padding": "medium", "radius": 24, "shadow": "medium", "image": "" },
            "webcam": { "enabled": false, "file": "", "size": "medium", "corner": "bottom_right", "shape": "circle", "mirror": false },
            "exports": [{ "aspect": "16:9", "width": 1920, "height": 1080 }],
            "futureTopLevel": { "keep": "me" }
        }"#;
        let mut project: Project = serde_json::from_str(text).unwrap();
        project.normalize();
        let written = serde_json::to_value(&project).unwrap();

        assert_eq!(written["futureTopLevel"]["keep"], "me");
        assert_eq!(written["camera"]["futureCameraOption"], true);
        assert_eq!(written["camera"]["keyframes"][0]["extraKf"], 7);
        assert_eq!(written["take"], "reel-1.mp4");
        assert_eq!(written["source"]["frameCount"], 741);
    }

    #[test]
    fn missing_fields_take_the_v1_defaults() {
        let mut project: Project = serde_json::from_str(r#"{"version": 1}"#).unwrap();
        project.normalize();
        assert_eq!(project.take, "");
        assert_eq!(project.trim_start, 0.0);
        assert_eq!(project.camera.style, "off");
        assert_eq!(project.camera.depth, 2.0);
        assert_eq!(project.camera.easing, "smoothstep");
        assert_eq!(project.canvas.background, "theme");
        assert_eq!(project.canvas.radius, 24);
        assert_eq!(project.webcam.corner, "bottom_right");
        assert_eq!(project.source.fps, 60.0);
        assert_eq!(project.cursor.size, "medium");
        assert!(project.cursor.hide_idle);
    }

    #[test]
    fn loading_a_fixture_sorts_and_clamps_the_camera() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project-basic.json");
        let project = Project::load(&path).unwrap();

        assert_eq!(project.version, 1);
        assert_eq!(project.take, "reel-2026-11-03_12-00-00.mp4");
        assert_eq!(project.trim_start, 0.0);
        assert_eq!(project.source.width, 2560);
        assert_eq!(project.source.height, 1440);
        assert_eq!(project.camera.depth, 2.0);
        assert!(project.camera.needs_zoom());
        assert!(project
            .camera
            .keyframes
            .windows(2)
            .all(|pair| pair[0].t <= pair[1].t));
        for frame in &project.camera.keyframes {
            assert!(frame.zoom >= 1.0);
            assert!(frame.x >= 0.0 && frame.x <= 2560.0);
            assert!(frame.y >= 0.0 && frame.y <= 1440.0);
        }
    }

    #[test]
    fn a_newer_project_version_is_rejected() {
        let root = std::env::temp_dir().join(format!(
            "reel-project-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("take.project.json");
        fs::write(&path, r#"{"version": 2}"#).unwrap();
        let err = Project::load(&path).unwrap_err().to_string();
        assert!(err.contains("unsupported project version 2"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn keyframe_centres_are_clamped_to_the_source() {
        let mut camera = Camera {
            style: "cinematic".into(),
            depth: 2.0,
            keyframes: vec![
                Keyframe::new(0.0, 2.0, 0.0, 0.0),
                Keyframe::new(1.0, 2.0, 9999.0, 9999.0),
            ],
            ..Camera::default()
        };
        camera.normalize(2560.0, 1440.0);

        assert_eq!(camera.keyframes[0].x, 640.0);
        assert_eq!(camera.keyframes[0].y, 360.0);
        assert_eq!(camera.keyframes[1].x, 1920.0);
        assert_eq!(camera.keyframes[1].y, 1080.0);
    }

    #[test]
    fn viewport_never_leaves_the_source_between_keyframes() {
        // Low zoom in the middle widens the viewport, so a centre that was
        // safe at both ends must be clamped again after interpolation.
        let camera = Camera {
            style: "cinematic".into(),
            depth: 2.0,
            keyframes: vec![
                Keyframe::new(0.0, 2.0, 200.0, 200.0),
                Keyframe::new(1.0, 1.0, 200.0, 200.0),
                Keyframe::new(2.0, 2.0, 200.0, 200.0),
            ],
            ..Camera::default()
        };
        for step in 0..=200 {
            let t = step as f64 / 100.0;
            let (zoom, x, y) = camera.viewport_at(t, 1280.0, 720.0);
            let half_w = 1280.0 / (2.0 * zoom);
            let half_h = 720.0 / (2.0 * zoom);
            assert!(zoom >= 1.0);
            assert!(x >= half_w - 1e-9 && x <= 1280.0 - half_w + 1e-9);
            assert!(y >= half_h - 1e-9 && y <= 720.0 - half_h + 1e-9);
        }
    }

    #[test]
    fn smoothstep_is_flat_at_both_ends() {
        assert_eq!(smoothstep(0.0), 0.0);
        assert_eq!(smoothstep(1.0), 1.0);
        assert!((smoothstep(0.5) - 0.5).abs() < 1e-12);
        assert!((smoothstep(0.01) - 0.000298).abs() < 1e-6);
        assert!((smoothstep(0.99) - 0.999702).abs() < 1e-6);
    }

    #[test]
    fn clicks_merge_at_two_and_a_half_seconds_and_350_px() {
        let clusters = cluster_clicks(&[
            click(0.0, 100.0, 100.0),
            click(2.4, 240.0, 100.0),
            click(4.9, 260.0, 100.0),
            click(7.4, 700.0, 100.0),
        ]);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].len(), 3);
        assert_eq!(clusters[1].len(), 1);
    }

    #[test]
    fn clicks_far_apart_in_space_start_a_new_cluster() {
        let clusters = cluster_clicks(&[
            click(0.0, 100.0, 100.0),
            click(1.0, 100.0, 100.0),
            click(2.0, 600.0, 100.0),
        ]);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].len(), 2);
        assert_eq!(clusters[1].len(), 1);
    }

    #[test]
    fn a_click_produces_a_zoom_block_centred_on_that_area() {
        let width = 2560.0;
        let height = 1440.0;
        let camera = build_camera(&[click(1.0, 1800.0, 500.0)], true, 2.0, width, height, 0.0);

        assert_eq!(camera.style, "auto");
        assert!(camera.needs_zoom());
        let peak = camera
            .keyframes
            .iter()
            .find(|frame| frame.zoom == 2.0)
            .expect("a zoomed keyframe");
        assert_eq!(peak.x, 1800.0);
        assert_eq!(peak.y, 500.0);
        assert_eq!(camera.keyframes.first().unwrap().zoom, 1.0);
        assert_eq!(camera.keyframes.last().unwrap().zoom, 1.0);
    }

    #[test]
    fn the_camera_uses_the_cluster_centroid() {
        let camera = build_camera(
            &[click(1.0, 1000.0, 400.0), click(1.2, 1200.0, 600.0)],
            true,
            2.5,
            2560.0,
            1440.0,
            0.0,
        );
        let peak = camera
            .keyframes
            .iter()
            .find(|frame| frame.zoom == 2.5)
            .unwrap();
        assert_eq!(peak.x, 1100.0);
        assert_eq!(peak.y, 500.0);
    }

    #[test]
    fn overlapping_blocks_pan_instead_of_cutting_back_to_one() {
        // Two clicks too far apart to cluster, but close enough in time that
        // their eased windows overlap. The camera must stay zoomed and pan.
        let camera = build_camera(
            &[click(1.0, 1800.0, 500.0), click(1.3, 300.0, 1200.0)],
            true,
            2.0,
            2560.0,
            1440.0,
            0.0,
        );

        let zooms: Vec<f64> = camera.keyframes.iter().map(|frame| frame.zoom).collect();
        assert_eq!(zooms, vec![1.0, 2.0, 2.0, 2.0, 1.0]);
        assert!(camera
            .keyframes
            .windows(2)
            .all(|pair| pair[0].t < pair[1].t));
        // The pan goes from the first centre to the clamped second one.
        assert_eq!(camera.keyframes[1].x, 1800.0);
        assert_eq!(camera.keyframes[2].x, 640.0);
    }

    #[test]
    fn separate_blocks_return_to_one_between_clicks() {
        let camera = build_camera(
            &[click(1.0, 1280.0, 720.0), click(6.0, 1280.0, 720.0)],
            true,
            2.0,
            2560.0,
            1440.0,
            0.0,
        );
        let zooms: Vec<f64> = camera.keyframes.iter().map(|frame| frame.zoom).collect();
        assert_eq!(zooms, vec![1.0, 2.0, 2.0, 1.0, 1.0, 2.0, 2.0, 1.0]);
    }

    #[test]
    fn trim_start_moves_clicks_into_saved_take_time() {
        let camera = build_camera(&[click(1.0, 1280.0, 720.0)], true, 2.0, 2560.0, 1440.0, 3.0);
        // The click happened before the saved part of the take, so it is
        // clamped to zero rather than producing a negative keyframe.
        assert!(camera.keyframes.iter().all(|frame| frame.t >= 0.0));
        assert_eq!(camera.keyframes[0].t, 0.0);
    }

    #[test]
    fn clicks_outside_the_capture_are_ignored() {
        let camera = build_camera(
            &[
                click(1.0, -10.0, 100.0),
                click(1.1, 3000.0, 100.0),
                click(1.2, 100.0, -1.0),
            ],
            true,
            2.0,
            2560.0,
            1440.0,
            0.0,
        );
        assert!(camera.keyframes.is_empty());
        assert!(!camera.needs_zoom());
    }

    #[test]
    fn auto_zoom_off_keeps_the_project_still() {
        let camera = build_camera(
            &[click(1.0, 1280.0, 720.0)],
            false,
            2.0,
            2560.0,
            1440.0,
            0.0,
        );
        assert!(!camera.needs_zoom());
        assert!(camera.keyframes.is_empty());
        assert_eq!(camera.style, "off");
    }

    #[test]
    fn legacy_styles_read_as_the_single_auto_mode() {
        assert_eq!(valid_style("auto"), "auto");
        assert_eq!(valid_style("subtle"), "auto");
        assert_eq!(valid_style("cinematic"), "auto");
        assert_eq!(valid_style("close"), "auto");
        assert_eq!(valid_style("off"), "off");
        assert_eq!(valid_style("dramatic"), "off");
    }

    #[test]
    fn depth_and_times_are_sanitized() {
        assert_eq!(valid_depth(1.5), 1.5);
        assert_eq!(valid_depth(2.0), 2.0);
        assert_eq!(valid_depth(2.5), 2.5);
        assert_eq!(valid_depth(9.0), 2.0);
        assert_eq!(valid_depth(f64::NAN), 2.0);

        let mut project = Project {
            trim_start: -4.0,
            camera: Camera {
                depth: 9.0,
                easing: "linear".into(),
                keyframes: vec![
                    Keyframe::new(2.0, 0.5, 1280.0, 720.0),
                    Keyframe::new(1.0, 2.0, 1280.0, 720.0),
                    Keyframe::new(1.0, 3.0, 1280.0, 720.0),
                ],
                ..Camera::default()
            },
            source: Source {
                width: 2560,
                height: 1440,
                ..Source::default()
            },
            ..Project::default()
        };
        project.normalize();
        assert_eq!(project.trim_start, 0.0);
        assert_eq!(project.camera.depth, 2.0);
        assert_eq!(project.camera.easing, "smoothstep");
        assert_eq!(project.camera.keyframes.len(), 2);
        assert_eq!(project.camera.keyframes[0].t, 1.0);
        assert_eq!(project.camera.keyframes[0].zoom, 3.0);
        assert_eq!(project.camera.keyframes[1].t, 2.0);
        assert_eq!(project.camera.keyframes[1].zoom, 1.0);
    }

    #[test]
    fn take_and_output_paths_follow_the_contract() {
        let take = Path::new("/videos/Reel/reel-2026-11-03_12-00-00.mp4");
        assert_eq!(
            project_path(take),
            Path::new("/videos/Reel/reel-2026-11-03_12-00-00.project.json")
        );
        assert_eq!(
            rendered_path(take),
            Path::new("/videos/Reel/reel-2026-11-03_12-00-00.rendered.mp4")
        );

        let mut project = Project {
            take: "reel-1.mp4".into(),
            ..Project::default()
        };
        assert_eq!(
            project.take_path(Path::new("/videos/Reel/reel-1.project.json")),
            Path::new("/videos/Reel/reel-1.mp4")
        );
        project.take = "/elsewhere/reel-2.mp4".into();
        assert_eq!(
            project.take_path(Path::new("/videos/Reel/reel-1.project.json")),
            Path::new("/elsewhere/reel-2.mp4")
        );
    }
}
