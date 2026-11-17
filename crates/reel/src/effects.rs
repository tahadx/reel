//! Click effects burned in by the save pass.
//!
//! Reel draws them as ASS vector shapes and lets libass composite them over
//! the source frames before the camera pass. The shapes stay in source pixels,
//! so a zoom moves the effect with the picture underneath it. Nothing here is
//! live: the save pass owns the effect, and `reel render` replays it from the
//! project's `ripples` block.

use crate::project::Ripples;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The effect styles the panel and project contract accept.
pub const STYLES: [&str; 2] = ["ripple", "halo"];

/// The fallback ink for a theme without a readable accent.
const FALLBACK_ACCENT: &str = "#ffffff";
/// Cubic circle control-point distance for a unit circle.
const KAPPA: f64 = 0.552_284_749_830_793_6;

/// Keep a style inside the contract. Anything unknown (including the `off`
/// switch) reads as `off`, so a hand-edited project can never invent a style.
pub fn valid_style(style: &str) -> &'static str {
    let style = style.trim().to_ascii_lowercase();
    STYLES
        .iter()
        .copied()
        .find(|known| *known == style)
        .unwrap_or("off")
}

/// Normalize a project color to lowercase `#rrggbb`. Empty and junk are `None`
/// so the renderer can fall back to the live theme.
pub fn valid_color(color: &str) -> Option<String> {
    let raw = color.trim();
    let raw = raw.strip_prefix('#').unwrap_or(raw);
    if raw.len() != 6 || !raw.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", raw.to_ascii_lowercase()))
}

/// The active Omarchy accent, read from the theme's `colors.toml`.
///
/// The panel owns the live theme, but the save pass runs outside the shell. It
/// reads the same file Omarchy writes for the current theme rather than a
/// private copy, so a theme switch is picked up by the next take.
pub fn theme_accent() -> String {
    let mut candidates = Vec::new();
    if let Some(state) = state_root() {
        candidates.push(state.join("omarchy/current/theme/colors.toml"));
    }
    if let Some(home) = home_dir() {
        candidates.push(home.join(".config/omarchy/current/theme/colors.toml"));
    }
    for path in candidates {
        if let Some(color) = accent_in(&path) {
            return color;
        }
    }
    FALLBACK_ACCENT.into()
}

fn state_root() -> Option<PathBuf> {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        if !state.is_empty() {
            return Some(PathBuf::from(state));
        }
    }
    home_dir().map(|home| home.join(".local/state"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

fn accent_in(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "accent" {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if let Some(color) = valid_color(value) {
            return Some(color);
        }
    }
    None
}

/// The ffmpeg filter that renders `script` over the input frames.
pub fn ass_filter(script: &Path) -> String {
    format!("ass=filename='{}'", escape_filter_value(script))
}

fn escape_filter_value(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
}

/// Write the effect script. The caller owns the runtime path and removes it
/// after the render.
pub fn write_script(path: &Path, script: &str) -> io::Result<()> {
    fs::write(path, script)
}

/// Build the ASS script for a project's click effects. No clicks or a disabled
/// effect yields `None`, so the caller can skip the filter entirely.
pub fn subtitle_script(
    ripples: &Ripples,
    width: u32,
    height: u32,
    duration: f64,
) -> Option<String> {
    if !ripples.needs_render() || width == 0 || height == 0 {
        return None;
    }
    let style = valid_style(&ripples.style);
    if style == "off" {
        return None;
    }
    let accent = valid_color(&ripples.color).unwrap_or_else(theme_accent);
    // The shapes are the theme accent, with a lighter accent for the hot core.
    // White is not the effect's ink: the dark hairline is what carries contrast
    // on light content.
    let palette = Palette {
        ring: lighten(&accent, 0.10),
        hot: lighten(&accent, 0.28),
        accent,
    };
    // The effect's natural size follows the frame. At 1080p the base radius is
    // about 39 px, so the ring peaks near 58 px and reads at normal viewing
    // distance without covering the click target.
    let unit = (f64::from(width.min(height)) * 0.036).clamp(22.0, 66.0);
    let last = if duration.is_finite() && duration > 0.0 {
        duration + 1.0
    } else {
        ripples
            .clicks
            .last()
            .map(|click| click.t + 1.5)
            .unwrap_or(1.0)
    };

    let mut events = String::new();
    for click in &ripples.clicks {
        if !(click.t.is_finite() && click.x.is_finite() && click.y.is_finite()) {
            continue;
        }
        if click.t > last || click.x < 0.0 || click.y < 0.0 {
            continue;
        }
        if click.x >= f64::from(width) || click.y >= f64::from(height) {
            continue;
        }
        match style {
            "halo" => halo(&mut events, click.t, click.x, click.y, unit, &palette),
            _ => ripple(&mut events, click.t, click.x, click.y, unit, &palette),
        }
    }
    if events.is_empty() {
        return None;
    }

    let mut script = String::with_capacity(events.len() + 512);
    script.push_str("[Script Info]\n");
    script.push_str("; Generated by Reel: click effects\n");
    script.push_str("ScriptType: v4.00+\n");
    script.push_str(&format!("PlayResX: {width}\nPlayResY: {height}\n"));
    script.push_str("WrapStyle: 2\nScaledBorderAndShadow: yes\n");
    script.push_str("YCbCr Matrix: TV.601\n\n");
    script.push_str("[V4+ Styles]\n");
    script.push_str(
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, \
         OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, \
         ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, \
         MarginL, MarginR, MarginV, Encoding\n",
    );
    script.push_str(
        "Style: Effect,DejaVu Sans,20,&H00FFFFFF,&H00FFFFFF,&H00000000,\
         &H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n",
    );
    script.push_str("[Events]\n");
    script.push_str(
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, \
         Effect, Text\n",
    );
    script.push_str(&events);
    Some(script)
}

/// The accent shades one effect uses. `accent` is the theme color, `ring` is
/// the slightly lifted ring, and `hot` is the lit core.
struct Palette {
    accent: String,
    ring: String,
    hot: String,
}

/// The signature effect: an accent glow, a crisp accented ring, a thin
/// trailing accent echo, and a lighter accent core that dies first.
fn ripple(out: &mut String, t: f64, x: f64, y: f64, unit: f64, palette: &Palette) {
    let start = t.max(0.0);
    let glow = format!(
        "{{\\an7\\pos({x:.1},{y:.1})\\p1\\1c{}\\1a&H78&\\bord0\\blur{:.1}\
         \\fscx{:.1}\\fscy{:.1}\\fad(40,540)\
         \\t(0,220,\\fscx{:.1}\\fscy{:.1})\
         \\t(220,760,\\fscx{:.1}\\fscy{:.1})}}",
        ass_color(&palette.accent),
        unit * 0.16,
        unit * 0.34,
        unit * 0.34,
        unit * 0.92,
        unit * 0.92,
        unit * 1.62,
        unit * 1.62,
    );
    event(out, 0, start, start + 0.78, &glow, &ring_shape(100.0, 56.0));

    let echo = format!(
        "{{\\an7\\pos({x:.1},{y:.1})\\p1\\1c{}\\1a&H38&\\3c&H000000&\\3a&H86&\
         \\bord1.1\\blur0.4\\fscx{:.1}\\fscy{:.1}\\fad(20,220)\
         \\t(0,170,\\fscx{:.1}\\fscy{:.1})\
         \\t(170,500,\\fscx{:.1}\\fscy{:.1})}}",
        ass_color(&palette.accent),
        unit * 0.60,
        unit * 0.60,
        unit * 0.98,
        unit * 0.98,
        unit * 1.24,
        unit * 1.24,
    );
    event(
        out,
        1,
        start + 0.07,
        start + 0.56,
        &echo,
        &ring_shape(100.0, 84.0),
    );

    let ring_tags = format!(
        "{{\\an7\\pos({x:.1},{y:.1})\\p1\\1c{}\\1a&H0C&\\3c&H000000&\\3a&H58&\
         \\bord1.4\\blur0.3\\fscx{:.1}\\fscy{:.1}\\fad(14,300)\
         \\t(0,230,\\fscx{:.1}\\fscy{:.1})\
         \\t(230,620,\\fscx{:.1}\\fscy{:.1})}}",
        ass_color(&palette.ring),
        unit * 0.44,
        unit * 0.44,
        unit * 1.04,
        unit * 1.04,
        unit * 1.48,
        unit * 1.48,
    );
    event(
        out,
        2,
        start,
        start + 0.64,
        &ring_tags,
        &ring_shape(100.0, 82.0),
    );

    let core = format!(
        "{{\\an7\\pos({x:.1},{y:.1})\\p1\\1c{}\\1a&H00&\\3c&H000000&\\3a&H70&\
         \\bord0.9\\fscx{:.1}\\fscy{:.1}\\fad(12,150)}}",
        ass_color(&palette.hot),
        unit * 0.14,
        unit * 0.14,
    );
    event(out, 3, start, start + 0.24, &core, &circle(100.0));
}

/// A softer effect: the click blooms like a lens flare, with no hard outline.
fn halo(out: &mut String, t: f64, x: f64, y: f64, unit: f64, palette: &Palette) {
    let start = t.max(0.0);
    let bloom = format!(
        "{{\\an7\\pos({x:.1},{y:.1})\\p1\\1c{}\\1a&H86&\\bord0\\blur{:.1}\
         \\fscx{:.1}\\fscy{:.1}\\fad(70,600)\
         \\t(0,300,\\fscx{:.1}\\fscy{:.1})\
         \\t(300,880,\\fscx{:.1}\\fscy{:.1})}}",
        ass_color(&palette.accent),
        unit * 0.24,
        unit * 0.52,
        unit * 0.52,
        unit * 1.24,
        unit * 1.24,
        unit * 1.86,
        unit * 1.86,
    );
    event(out, 0, start, start + 0.94, &bloom, &circle(100.0));

    let inner = format!(
        "{{\\an7\\pos({x:.1},{y:.1})\\p1\\1c{}\\1a&H48&\\bord0\\blur{:.1}\
         \\fscx{:.1}\\fscy{:.1}\\fad(40,430)\
         \\t(0,240,\\fscx{:.1}\\fscy{:.1})\
         \\t(240,680,\\fscx{:.1}\\fscy{:.1})}}",
        ass_color(&palette.accent),
        unit * 0.09,
        unit * 0.18,
        unit * 0.18,
        unit * 0.72,
        unit * 0.72,
        unit * 1.08,
        unit * 1.08,
    );
    event(out, 1, start, start + 0.72, &inner, &circle(100.0));

    let core = format!(
        "{{\\an7\\pos({x:.1},{y:.1})\\p1\\1c{}\\1a&H00&\\3c&H000000&\\3a&H78&\
         \\bord1.0\\fscx{:.1}\\fscy{:.1}\\fad(14,220)\
         \\t(0,150,\\fscx{:.1}\\fscy{:.1})}}",
        ass_color(&palette.hot),
        unit * 0.10,
        unit * 0.10,
        unit * 0.16,
        unit * 0.16,
    );
    event(out, 2, start, start + 0.34, &core, &circle(100.0));
}

fn event(out: &mut String, layer: u8, start: f64, end: f64, tags: &str, drawing: &str) {
    if !(start.is_finite() && end.is_finite()) || end <= start {
        return;
    }
    out.push_str(&format!(
        "Dialogue: {layer},{},{},Effect,,0,0,0,,{tags}{drawing}\n",
        ass_time(start.max(0.0)),
        ass_time(end.max(start + 0.01)),
    ));
}

fn ass_time(seconds: f64) -> String {
    let centis = (seconds * 100.0).round().max(0.0) as u64;
    let hours = centis / 360_000;
    let minutes = (centis / 6_000) % 60;
    let seconds = (centis / 100) % 60;
    let centis = centis % 100;
    format!("{hours}:{minutes:02}:{seconds:02}.{centis:02}")
}

fn ass_color(hex: &str) -> String {
    let raw = hex.trim_start_matches('#');
    let red = u8::from_str_radix(&raw[0..2], 16).unwrap_or(255);
    let green = u8::from_str_radix(&raw[2..4], 16).unwrap_or(255);
    let blue = u8::from_str_radix(&raw[4..6], 16).unwrap_or(255);
    format!("&H{blue:02X}{green:02X}{red:02X}&")
}

/// Mix an accent toward white. The effect stays the theme's color, but a
/// lightened accent gives the core a lit highlight instead of a white dot.
fn lighten(hex: &str, amount: f64) -> String {
    let raw = hex.trim_start_matches('#');
    let amount = amount.clamp(0.0, 1.0);
    let channel = |index: usize| -> u8 {
        let value = u8::from_str_radix(&raw[index..index + 2], 16).unwrap_or(255);
        (f64::from(value) + (255.0 - f64::from(value)) * amount).round() as u8
    };
    format!("#{:02x}{:02x}{:02x}", channel(0), channel(2), channel(4))
}

/// One filled circle of `radius` on the 100-unit drawing canvas.
fn circle(radius: f64) -> String {
    let k = radius * KAPPA;
    format!(
        "m 0 -{r} b {k} -{r} {r} -{k} {r} 0 b {r} {k} {k} {r} 0 {r} \
         b -{k} {r} -{r} {k} -{r} 0 b -{r} -{k} -{k} -{r} 0 -{r}",
        r = number(radius),
        k = number(k),
    )
}

/// An annulus. The inner contour is wound the other way so both the even-odd
/// and nonzero fill rules cut the middle out.
fn ring_shape(outer: f64, inner: f64) -> String {
    let k = inner * KAPPA;
    format!(
        "{} m 0 -{r} b -{k} -{r} -{r} -{k} -{r} 0 b -{r} {k} -{k} {r} 0 {r} \
         b {k} {r} {r} {k} {r} 0 b {r} -{k} {k} -{r} 0 -{r}",
        circle(outer),
        r = number(inner),
        k = number(k),
    )
}

fn number(value: f64) -> String {
    let mut text = format!("{value:.3}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    if text == "-0" {
        text = "0".into();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::RippleClick;

    fn ripples(style: &str) -> Ripples {
        Ripples {
            enabled: true,
            style: style.into(),
            color: "#8a9a7b".into(),
            clicks: vec![
                RippleClick::new(0.5, 100.0, 200.0),
                RippleClick::new(2.0, 300.0, 400.0),
            ],
            ..Ripples::default()
        }
    }

    #[test]
    fn an_unknown_style_is_off_and_the_known_ones_survive() {
        assert_eq!(valid_style("RIPPLE"), "ripple");
        assert_eq!(valid_style(" halo "), "halo");
        assert_eq!(valid_style("pulse"), "off");
        assert_eq!(valid_style("off"), "off");
        assert_eq!(valid_style("sparkle"), "off");
    }

    #[test]
    fn colors_are_normalized_or_rejected() {
        assert_eq!(valid_color("#8A9A7B"), Some("#8a9a7b".into()));
        assert_eq!(valid_color("8a9a7b"), Some("#8a9a7b".into()));
        assert_eq!(valid_color("#fff"), None);
        assert_eq!(valid_color(""), None);
        assert_eq!(valid_color("#zzzzzz"), None);
    }

    #[test]
    fn a_script_has_one_effect_per_click_with_source_position() {
        let script = subtitle_script(&ripples("ripple"), 1920, 1080, 4.0).expect("a script");
        assert!(script.contains("PlayResX: 1920"));
        assert!(script.contains("PlayResY: 1080"));
        assert_eq!(script.matches("Dialogue:").count(), 8);
        assert!(script.contains("\\pos(100.0,200.0)"));
        assert!(script.contains("\\pos(300.0,400.0)"));
        assert!(script.contains("0:00:00.50"));
        assert!(script.contains("0:00:02.00"));
        // The shapes are the theme accent, never a white fill.
        assert!(script.contains("&H7B9A8A&"));
        assert!(!script.contains("&HFFFFFF&"));
    }

    #[test]
    fn the_two_styles_draw_differently() {
        let ripple = subtitle_script(&ripples("ripple"), 2560, 1440, 6.0).unwrap();
        let halo = subtitle_script(&ripples("halo"), 2560, 1440, 6.0).unwrap();
        assert!(ripple.contains("Dialogue:"));
        assert!(halo.contains("Dialogue:"));
        assert_ne!(ripple, halo);
    }

    #[test]
    fn pulse_is_gone_from_the_contract() {
        assert_eq!(valid_style("pulse"), "off");
        let mut list = ripples("pulse");
        list.style = "pulse".into();
        assert!(subtitle_script(&list, 1920, 1080, 4.0).is_none());
    }

    #[test]
    fn disabled_or_clickless_effects_skip_the_render() {
        let mut off = ripples("ripple");
        off.enabled = false;
        assert!(subtitle_script(&off, 1920, 1080, 4.0).is_none());

        let mut empty = ripples("ripple");
        empty.clicks.clear();
        assert!(subtitle_script(&empty, 1920, 1080, 4.0).is_none());

        let mut unknown = ripples("sparkle");
        unknown.style = "sparkle".into();
        assert!(subtitle_script(&unknown, 1920, 1080, 4.0).is_none());
    }

    #[test]
    fn clicks_outside_the_capture_are_dropped() {
        let mut list = ripples("ripple");
        list.clicks = vec![
            RippleClick::new(0.5, -1.0, 10.0),
            RippleClick::new(0.6, 10.0, -1.0),
            RippleClick::new(0.7, 1920.0, 10.0),
            RippleClick::new(0.8, 10.0, 1080.0),
            RippleClick::new(0.9, 10.0, 10.0),
        ];
        let script = subtitle_script(&list, 1920, 1080, 4.0).unwrap();
        assert_eq!(script.matches("Dialogue:").count(), 4);
    }

    #[test]
    fn the_theme_accent_parser_reads_a_quoted_toml_value() {
        let root = std::env::temp_dir().join(format!(
            "reel-effects-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let theme = root.join("omarchy/current/theme");
        fs::create_dir_all(&theme).unwrap();
        let colors = theme.join("colors.toml");
        fs::write(
            &colors,
            "mode = \"dark\"\n# accent = \"#000000\"\naccent = \"#8A9A7B\"\n",
        )
        .unwrap();
        assert_eq!(accent_in(&colors).as_deref(), Some("#8a9a7b"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn ffmpeg_accepts_the_effect_script() {
        if !crate::util::have("ffmpeg") {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "reel-effects-ffmpeg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&root).unwrap();
        let ass = root.join("effect.ass");
        let mut list = ripples("ripple");
        list.clicks = vec![RippleClick::new(0.1, 320.0, 180.0)];
        let script = subtitle_script(&list, 640, 360, 1.0).unwrap();
        write_script(&ass, &script).unwrap();
        let output = std::process::Command::new("ffmpeg")
            .args([
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=640x360:d=0.6:r=30",
                "-vf",
                &format!("{},trim=start=0.3", ass_filter(&ass)),
                "-frames:v",
                "1",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "gray",
                "-",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "ffmpeg rejected the effect script: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout.len(), 640 * 360);

        // The ring is drawn on the click point. A 14 px anchor error (the
        // drawing-alignment trap this test exists for) would move this centroid.
        let mut count = 0u64;
        let mut sum_x = 0u64;
        let mut sum_y = 0u64;
        for (index, value) in output.stdout.iter().enumerate() {
            if *value > 24 {
                count += 1;
                sum_x += (index % 640) as u64;
                sum_y += (index / 640) as u64;
            }
        }
        assert!(count > 100, "the effect drew only {count} pixels");
        let center_x = sum_x as f64 / count as f64;
        let center_y = sum_y as f64 / count as f64;
        assert!(
            (center_x - 320.0).abs() < 4.0 && (center_y - 180.0).abs() < 4.0,
            "the effect is not on the click: ({center_x:.1}, {center_y:.1})"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn ass_times_round_to_centiseconds() {
        assert_eq!(ass_time(0.0), "0:00:00.00");
        assert_eq!(ass_time(1.239), "0:00:01.24");
        assert_eq!(ass_time(3661.5), "1:01:01.50");
    }

    #[test]
    fn an_effect_script_escapes_the_runtime_path() {
        let filter = ass_filter(Path::new("/run/user/1/reel/render-1.ass"));
        assert_eq!(filter, "ass=filename='/run/user/1/reel/render-1.ass'");
    }
}
