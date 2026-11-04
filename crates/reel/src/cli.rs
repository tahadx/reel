use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "reel",
    version,
    about = "Reel — screen recorder for Omarchy.",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start a recording, selecting a target and beginning immediately.
    Start(StartArgs),
    /// Start a recording, or stop the one in progress.
    Toggle(StartArgs),
    /// Stop the take and save it, or discard it.
    Stop {
        #[arg(long)]
        discard: bool,
    },
    /// Pause the recorder.
    Pause,
    /// Resume a paused recorder.
    Resume,
    /// Pause a recording, or resume it when already paused.
    #[command(name = "toggle-pause")]
    TogglePause,
    /// Print the current state.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// List recent takes in the output folder.
    Recent {
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 8)]
        limit: usize,
    },
    /// Move a take to the trash.
    Trash { file: PathBuf },
    /// Read or write the settings file.
    Config(ConfigArgs),
    /// Capture a still of the next take's framing for the panel.
    Preview {
        #[arg(long)]
        json: bool,
        /// Resolve the target and note without taking a new still.
        #[arg(long = "note-only")]
        note_only: bool,
    },
    /// Render a take again from its project file.
    Render(RenderArgs),
    /// Print the recordings folder.
    Output,
    /// Check the tools Reel needs.
    Doctor,
    /// Update the installed plugin and re-run its installer.
    Update {
        /// Add or refresh the optional Hyprland bindings without prompting.
        #[arg(long)]
        binds: bool,
    },
    /// Zoom in on the pointer during recording.
    #[command(name = "zoom-in")]
    ZoomIn {
        #[arg(short, long)]
        step: Option<f64>,
        #[arg(value_name = "STEP")]
        pos_step: Option<f64>,
    },
    /// Zoom out during recording.
    #[command(name = "zoom-out")]
    ZoomOut {
        #[arg(short, long)]
        step: Option<f64>,
        #[arg(value_name = "STEP")]
        pos_step: Option<f64>,
    },
    /// Reset zoom to 1.0x (full frame) during recording.
    #[command(name = "zoom-reset")]
    ZoomReset,
    /// Toggle zoom between 1.0x and magnification level during recording.
    #[command(name = "zoom-toggle")]
    ZoomToggle {
        #[arg(short, long)]
        factor: Option<f64>,
        #[arg(value_name = "FACTOR")]
        pos_factor: Option<f64>,
    },
    /// Check current manual zoom level during recording.
    #[command(name = "zoom-status")]
    ZoomStatus {
        #[arg(long)]
        json: bool,
    },
    /// Pan the zoomed view during recording.
    #[command(name = "zoom-pan")]
    ZoomPan {
        /// left, right, up, or down.
        direction: Option<String>,
        #[arg(short, long)]
        step: Option<f64>,
        /// Use the larger step, for covering ground quickly.
        #[arg(long)]
        fast: bool,
        /// Keep panning until stopped (used by the hold binds).
        #[arg(long)]
        hold: bool,
        /// Stop a held pan.
        #[arg(long)]
        stop: bool,
    },
    /// Manual zoom controls.
    Zoom(ZoomActionArgs),
    /// Internal child process that owns the save pass.
    #[command(name = "__finalize", hide = true)]
    Finalize(FinalizeArgs),
}

#[derive(Args, Debug, Default, Clone)]
pub struct StartArgs {
    #[arg(long = "desktop-audio")]
    pub desktop_audio: Option<String>,
    #[arg(long = "microphone-audio", alias = "mic")]
    pub microphone_audio: Option<String>,
    #[arg(long = "noise-reduction", alias = "noise-cancellation")]
    pub noise_cancellation: Option<String>,
    #[arg(long)]
    pub cursor: Option<String>,
    #[arg(long)]
    pub webcam: Option<String>,
    #[arg(long = "webcam-device")]
    pub webcam_device: Option<String>,
    #[arg(long)]
    pub countdown: Option<String>,
    #[arg(long)]
    pub fps: Option<String>,
    /// Quality preset: very_low, low, medium, high, very_high, or ultra.
    #[arg(long)]
    pub quality: Option<String>,
    #[arg(long)]
    pub codec: Option<String>,
    #[arg(long)]
    pub resolution: Option<String>,
    #[arg(long)]
    pub container: Option<String>,
    #[arg(long = "output-dir")]
    pub output_dir: Option<String>,
    /// Prefix for the saved filename. Empty uses `reel-<date>`.
    #[arg(long = "filename-prefix")]
    pub filename_prefix: Option<String>,
    #[arg(long = "manual-zoom")]
    pub manual_zoom: Option<String>,
    #[arg(long = "zoom-max")]
    pub zoom_max: Option<f64>,
}

#[derive(Args, Debug)]
pub struct RenderArgs {
    /// Project file written next to the take.
    #[arg(long)]
    pub project: PathBuf,
    /// Output path. Defaults to `<take-stem>.rendered.<ext>`.
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: Option<ConfigAction>,
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Print the settings file path.
    Path,
    /// Print the merged settings.
    Show,
    /// Write the default settings if no file exists.
    Init,
    /// Set one key (the value is parsed as JSON when possible).
    Set { key: String, value: String },
    /// Merge a JSON object into the settings.
    Apply {
        #[arg(default_value = "")]
        patch: String,
    },
}

#[derive(Args, Debug)]
pub struct FinalizeArgs {
    #[arg(long)]
    pub file: PathBuf,
    /// Final path for the trimmed take. Empty trims in place.
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long = "trim-start", default_value_t = 0.0)]
    pub trim_start: f64,
    #[arg(long, default_value = "")]
    pub preview: String,
    #[arg(long = "started-at", default_value_t = 0)]
    pub started_at: i64,
    #[arg(long, default_value = "")]
    pub target: String,
    #[arg(long = "target-kind", default_value = "")]
    pub target_kind: String,
    #[arg(long = "target-label", default_value = "")]
    pub target_label: String,
    /// Apply microphone noise reduction during the save pass.
    #[arg(long = "noise-cancellation")]
    pub noise_cancellation: bool,
    /// Use a project contract for this save instead of the legacy trim.
    #[arg(long)]
    pub project: Option<PathBuf>,
    /// Pointer telemetry for this take. The save pass reads it to build the
    /// camera and writes `<take>.project.json`; without it the save is
    /// unchanged and no project is written.
    #[arg(long)]
    pub telemetry: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct ZoomActionArgs {
    #[command(subcommand)]
    pub action: Option<ZoomSubcommand>,
}

#[derive(Subcommand, Debug)]
pub enum ZoomSubcommand {
    /// Zoom in on pointer during recording.
    In {
        #[arg(short, long)]
        step: Option<f64>,
        #[arg(value_name = "STEP")]
        pos_step: Option<f64>,
    },
    /// Zoom out during recording.
    Out {
        #[arg(short, long)]
        step: Option<f64>,
        #[arg(value_name = "STEP")]
        pos_step: Option<f64>,
    },
    /// Reset zoom to 1.0x (full frame) during recording.
    Reset,
    /// Toggle zoom between 1.0x and target magnification.
    Toggle {
        #[arg(short, long)]
        factor: Option<f64>,
        #[arg(value_name = "FACTOR")]
        pos_factor: Option<f64>,
    },
    /// Query current zoom level during recording.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Pan the zoomed view during recording.
    Pan {
        /// left, right, up, or down.
        direction: Option<String>,
        #[arg(short, long)]
        step: Option<f64>,
        #[arg(long)]
        fast: bool,
        #[arg(long)]
        hold: bool,
        #[arg(long)]
        stop: bool,
    },
}
