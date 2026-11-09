//! Where the picture changed while a zoom block was playing.
//!
//! Auto zoom aims every block at the pointer. That is right for a click on the
//! thing the user is looking at. It is wrong for a tap that only focuses a
//! window, because a touchpad tap clicks wherever the pointer was parked. The
//! take then zooms into an empty patch while the user types somewhere else.
//!
//! The save pass samples the capture around each block. When the click landed
//! on a quiet area and one compact part of the screen changed, the block
//! follows the change: the text being typed, the caret blinking next to it, the
//! panel that opened. A click that changes the picture where it landed keeps
//! its place, so ordinary clicking is unaffected.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::paths::{self, Paths};

/// Analysis grid over the sampled frame.
pub const GRID_X: usize = 16;
pub const GRID_Y: usize = 9;
/// Sampling size. Small enough that a whole window is a few hundred kilobytes,
/// large enough that a caret is more than one pixel.
const SAMPLE_WIDTH: usize = 160;
const SAMPLE_HEIGHT: usize = 90;
const SAMPLE_FPS: f64 = 8.0;
/// The longest span sampled for one block. Typing keeps going and a caret
/// blinks, so a long block does not need every frame.
const MAX_WINDOW_SECS: f64 = 8.0;
/// Cells within this share of the busiest cell belong to the change.
const CLUSTER_SHARE: f64 = 0.35;
/// A click area with less than this share of the busiest cell is quiet.
const QUIET_SHARE: f64 = 0.25;
/// Change below this mean grey level is compression noise.
const MIN_CELL_CHANGE: f64 = 1.5;
/// A change covering more than this share of the grid is the whole screen
/// moving, not one thing to zoom at.
const MAX_CLUSTER_SHARE: f64 = 0.5;
/// One cell changing on its own is a clock tick or a notification dot, not the
/// place the user is working.
const MIN_CLUSTER_CELLS: usize = 2;
/// A retarget must move the camera at least this far, as a share of the width,
/// so a one-cell nudge never fights the click.
const MIN_MOVE_SHARE: f64 = 0.05;
/// A drag must start within this long after the click.
const DRAG_START_SECS: f64 = 0.75;
/// A drag must last at least this long.
const DRAG_MIN_SECS: f64 = 0.30;
/// A drag must move at least this share of the width.
const DRAG_MIN_SHARE: f64 = 0.04;
/// A drag ends after this long without movement.
const DRAG_STOP_SECS: f64 = 0.30;
/// How long after a click to keep looking for a drag.
const DRAG_MAX_SECS: f64 = 6.0;
/// Spacing of the camera points generated along a drag.
const DRAG_STEP_SECS: f64 = 0.12;

/// Mean absolute change per pixel, cell by cell, over a sampled window.
#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    cells: Vec<f64>,
}

impl Activity {
    /// Accumulate the change between consecutive grey frames into the grid.
    /// Frames must be `SAMPLE_WIDTH * SAMPLE_HEIGHT` bytes.
    pub fn from_frames(frames: &[Vec<u8>]) -> Option<Self> {
        let pixels = SAMPLE_WIDTH * SAMPLE_HEIGHT;
        if frames.len() < 2 || frames.iter().any(|frame| frame.len() != pixels) {
            return None;
        }

        let mut total = vec![0.0f64; GRID_X * GRID_Y];
        let mut counts = vec![0u64; GRID_X * GRID_Y];
        for pair in frames.windows(2) {
            let (before, after) = (&pair[0], &pair[1]);
            for gy in 0..GRID_Y {
                let y0 = gy * SAMPLE_HEIGHT / GRID_Y;
                let y1 = (gy + 1) * SAMPLE_HEIGHT / GRID_Y;
                for gx in 0..GRID_X {
                    let x0 = gx * SAMPLE_WIDTH / GRID_X;
                    let x1 = (gx + 1) * SAMPLE_WIDTH / GRID_X;
                    let cell = gy * GRID_X + gx;
                    let mut sum = 0u64;
                    for y in y0..y1 {
                        let row = y * SAMPLE_WIDTH;
                        for x in x0..x1 {
                            sum += u64::from(before[row + x].abs_diff(after[row + x]));
                        }
                    }
                    total[cell] += sum as f64;
                    counts[cell] += ((y1 - y0) * (x1 - x0)) as u64;
                }
            }
        }

        let cells = total
            .iter()
            .zip(&counts)
            .map(|(sum, count)| {
                if *count == 0 {
                    0.0
                } else {
                    sum / *count as f64
                }
            })
            .collect();
        Some(Self { cells })
    }

    /// The camera centre to use instead of `(cx, cy)`, or `None` to keep the
    /// click. Only a quiet click area and one compact patch of change count.
    pub fn retarget(&self, cx: f64, cy: f64, width: f64, height: f64) -> Option<(f64, f64)> {
        if !cx.is_finite() || !cy.is_finite() || width <= 0.0 || height <= 0.0 {
            return None;
        }
        let busiest = self.cells.iter().copied().fold(0.0, f64::max);
        if busiest < MIN_CELL_CHANGE {
            return None;
        }

        // Where the click landed, plus its neighbours.
        let click_x = ((cx / width) * GRID_X as f64)
            .floor()
            .clamp(0.0, (GRID_X - 1) as f64) as usize;
        let click_y = ((cy / height) * GRID_Y as f64)
            .floor()
            .clamp(0.0, (GRID_Y - 1) as f64) as usize;
        let mut near_click = 0.0f64;
        for gy in click_y.saturating_sub(1)..=(click_y + 1).min(GRID_Y - 1) {
            for gx in click_x.saturating_sub(1)..=(click_x + 1).min(GRID_X - 1) {
                near_click = near_click.max(self.cells[gy * GRID_X + gx]);
            }
        }
        if near_click >= QUIET_SHARE * busiest {
            return None;
        }

        let mut weight = 0.0;
        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        let mut cells = 0usize;
        for (index, score) in self.cells.iter().copied().enumerate() {
            if score < CLUSTER_SHARE * busiest {
                continue;
            }
            cells += 1;
            weight += score;
            let gx = index % GRID_X;
            let gy = index / GRID_X;
            sum_x += score * (gx as f64 + 0.5) / GRID_X as f64 * width;
            sum_y += score * (gy as f64 + 0.5) / GRID_Y as f64 * height;
        }
        if cells < MIN_CLUSTER_CELLS
            || weight <= 0.0
            || cells as f64 > MAX_CLUSTER_SHARE * (GRID_X * GRID_Y) as f64
        {
            return None;
        }

        let (x, y) = (sum_x / weight, sum_y / weight);
        let moved = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
        if moved < MIN_MOVE_SHARE * width {
            return None;
        }
        Some((x, y))
    }
}

/// Camera points along a pointer drag that starts just after the click.
///
/// A text selection is a drag: the pointer moves continuously from the click
/// to the other end. Screen Studio follows that movement; snapping between
/// clicks leaves the selection off-screen. Returns `(saved-take time, x, y)`
/// points, starting at the last still sample before the pointer moved.
pub fn drag_targets(
    samples: &[(f64, f64, f64)],
    enter: f64,
    hold: f64,
    width: f64,
) -> Vec<(f64, f64, f64)> {
    if width <= 0.0 || samples.len() < 2 {
        return Vec::new();
    }

    let mut points: Vec<(f64, f64, f64)> = Vec::new();
    let mut start: Option<(f64, f64, f64)> = None;
    let mut previous: Option<(f64, f64, f64)> = None;
    let mut last_move = enter;
    let start_limit = hold.max(enter + DRAG_START_SECS);

    for &sample in samples {
        if sample.0 < enter - 0.15 || sample.0 > enter + DRAG_MAX_SECS {
            continue;
        }
        let Some(prev) = previous else {
            previous = Some(sample);
            continue;
        };
        let step = distance(prev, sample);
        if start.is_none() {
            // The still sample before the first real movement is the drag's
            // origin, so the camera starts from the selection's first end.
            if sample.0 <= start_limit && step >= 2.0 {
                start = Some(prev);
                points.push(prev);
                points.push(sample);
                last_move = sample.0;
            }
            previous = Some(sample);
            continue;
        }
        points.push(sample);
        if step >= 2.0 {
            last_move = sample.0;
        }
        previous = Some(sample);
        if sample.0 - last_move > DRAG_STOP_SECS {
            break;
        }
    }

    let (Some(start), Some(&end)) = (start, points.last()) else {
        return Vec::new();
    };
    if last_move - start.0 < DRAG_MIN_SECS || distance(start, end) < DRAG_MIN_SHARE * width {
        return Vec::new();
    }

    // A camera point every DRAG_STEP_SECS keeps the pan continuous. Always
    // keep the final position so the block ends where the drag did.
    let mut out = vec![start];
    let mut next = start.0 + DRAG_STEP_SECS;
    for &point in points.iter().skip(1) {
        if point.0 >= next {
            out.push(point);
            next = point.0 + DRAG_STEP_SECS;
        }
    }
    if out.last().map(|point| point.0) != Some(end.0) {
        out.push(end);
    }
    smooth_positions(&mut out);
    out
}

fn distance(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
    ((a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt()
}

/// Three-point smoothing, keeping the endpoints exact.
fn smooth_positions(points: &mut [(f64, f64, f64)]) {
    if points.len() < 3 {
        return;
    }
    let original = points.to_vec();
    for index in 1..points.len() - 1 {
        let before = original[index - 1];
        let here = original[index];
        let after = original[index + 1];
        points[index].1 = (before.1 + 2.0 * here.1 + after.1) / 4.0;
        points[index].2 = (before.2 + 2.0 * here.2 + after.2) / 4.0;
    }
}

/// Samples a take around a block and answers where the camera should point.
pub struct Refiner {
    video: PathBuf,
    log: PathBuf,
    offset: f64,
    samples: Vec<(f64, f64, f64)>,
}

impl Refiner {
    /// `trim_start` converts saved-take time back to raw capture time.
    pub fn new(paths: &Paths, video: &Path, trim_start: f64) -> Self {
        Self {
            video: video.to_path_buf(),
            log: paths.log.clone(),
            offset: trim_start.max(0.0),
            samples: Vec::new(),
        }
    }

    /// Pointer samples in raw telemetry time, used to follow a drag.
    pub fn with_samples(mut self, samples: Vec<(f64, f64, f64)>) -> Self {
        self.samples = samples;
        self
    }

    /// The camera track for a block that eases in at `enter` and holds until
    /// `hold`, given the click centroid. The first point replaces the click
    /// when the click area was quiet; a pointer drag after the click is
    /// followed continuously.
    pub fn targets(
        &self,
        enter: f64,
        hold: f64,
        cx: f64,
        cy: f64,
        width: f64,
        height: f64,
    ) -> Option<Vec<(f64, f64, f64)>> {
        let mut points = Vec::new();
        let start = (enter + self.offset).max(0.0);
        let span = (hold + self.offset - start).clamp(0.0, MAX_WINDOW_SECS);
        if let Some(frames) = self.sample(start, span) {
            if let Some((x, y)) = Activity::from_frames(&frames)
                .and_then(|activity| activity.retarget(cx, cy, width, height))
            {
                points.push((enter, x, y));
                paths::log_to(
                    &self.log,
                    &format!(
                        "finalize: zoom target moved from ({cx:.0},{cy:.0}) to ({x:.0},{y:.0}) at {enter:.2}s; the click area was quiet"
                    ),
                );
            }
        }
        if points.is_empty() {
            points.push((enter, cx, cy));
        }

        if !self.samples.is_empty() {
            let saved: Vec<(f64, f64, f64)> = self
                .samples
                .iter()
                .map(|&(t, x, y)| (t - self.offset, x, y))
                .collect();
            let drag = drag_targets(&saved, enter, hold, width);
            if !drag.is_empty() {
                let count = drag.len();
                points.extend(drag.into_iter().filter(|point| point.0 > enter + 0.01));
                paths::log_to(
                    &self.log,
                    &format!("finalize: zoom follows a drag of {count} points at {enter:.2}s"),
                );
            }
        }
        Some(points)
    }

    fn sample(&self, start: f64, duration: f64) -> Option<Vec<Vec<u8>>> {
        if duration < 0.1 || !self.video.is_file() {
            return None;
        }
        let filter =
            format!("fps={SAMPLE_FPS},scale={SAMPLE_WIDTH}:{SAMPLE_HEIGHT}:flags=area,format=gray");
        let output = Command::new("ffmpeg")
            .args(["-v", "error", "-nostdin"])
            .args(["-ss", &format!("{start:.3}")])
            .arg("-i")
            .arg(&self.video)
            .args(["-t", &format!("{duration:.3}")])
            .args(["-vf", &filter])
            .args(["-f", "rawvideo", "-"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let size = SAMPLE_WIDTH * SAMPLE_HEIGHT;
        let frames: Vec<Vec<u8>> = output
            .stdout
            .chunks_exact(size)
            .map(|chunk| chunk.to_vec())
            .collect();
        if frames.len() < 2 {
            return None;
        }
        Some(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grey frame with an optional white rectangle, in sample pixels.
    fn frame(rect: Option<(usize, usize, usize, usize)>, value: u8) -> Vec<u8> {
        let mut frame = vec![0u8; SAMPLE_WIDTH * SAMPLE_HEIGHT];
        if let Some((x0, y0, x1, y1)) = rect {
            for y in y0..y1 {
                for x in x0..x1 {
                    frame[y * SAMPLE_WIDTH + x] = value;
                }
            }
        }
        frame
    }

    /// Frames where the rectangle blinks, which is what a caret looks like.
    fn blinking(rect: (usize, usize, usize, usize), count: usize) -> Vec<Vec<u8>> {
        (0..count)
            .map(|index| {
                if index % 2 == 0 {
                    frame(Some(rect), 0)
                } else {
                    frame(Some(rect), 255)
                }
            })
            .collect()
    }

    fn full_frame(value: u8) -> Vec<u8> {
        vec![value; SAMPLE_WIDTH * SAMPLE_HEIGHT]
    }

    /// The 2880x1620 source this sample grid stands for.
    const WIDTH: f64 = 2880.0;
    const HEIGHT: f64 = 1620.0;

    #[test]
    fn a_quiet_click_follows_the_blinking_region() {
        // A caret-like block at the top right, in sample pixels.
        let rect = (136, 4, 148, 13);
        let activity = Activity::from_frames(&blinking(rect, 6)).unwrap();

        // A click parked in the empty middle of the screen.
        let target = activity
            .retarget(2100.0, 820.0, WIDTH, HEIGHT)
            .expect("the caret is worth following");

        let (x, y) = target;
        assert!((x - 2556.0).abs() < 180.0, "x was {x}");
        assert!((y - 153.0).abs() < 180.0, "y was {y}");
    }

    #[test]
    fn a_click_on_the_change_keeps_its_place() {
        let activity = Activity::from_frames(&blinking((136, 4, 148, 13), 6)).unwrap();

        assert_eq!(activity.retarget(2556.0, 153.0, WIDTH, HEIGHT), None);
    }

    #[test]
    fn a_still_picture_keeps_the_click() {
        let frames = vec![frame(None, 0); 6];
        let activity = Activity::from_frames(&frames).unwrap();

        assert_eq!(activity.retarget(2100.0, 820.0, WIDTH, HEIGHT), None);
    }

    #[test]
    fn a_change_across_the_whole_frame_keeps_the_click() {
        let frames: Vec<Vec<u8>> = (0..6)
            .map(|index| {
                if index % 2 == 0 {
                    full_frame(0)
                } else {
                    full_frame(200)
                }
            })
            .collect();
        let activity = Activity::from_frames(&frames).unwrap();

        assert_eq!(activity.retarget(2100.0, 820.0, WIDTH, HEIGHT), None);
    }

    #[test]
    fn compression_noise_keeps_the_click() {
        let frames: Vec<Vec<u8>> = (0..6)
            .map(|index| {
                if index % 2 == 0 {
                    full_frame(40)
                } else {
                    full_frame(41)
                }
            })
            .collect();
        let activity = Activity::from_frames(&frames).unwrap();

        assert_eq!(activity.retarget(2100.0, 820.0, WIDTH, HEIGHT), None);
    }

    #[test]
    fn a_change_right_next_to_the_click_keeps_its_place() {
        // Deselecting a word changes pixels a few dozen px from the click; the
        // camera should not chase that.
        let activity = Activity::from_frames(&blinking((136, 4, 148, 13), 6)).unwrap();

        assert_eq!(activity.retarget(2490.0, 153.0, WIDTH, HEIGHT), None);
    }

    #[test]
    fn a_single_cell_change_is_a_tick_not_the_work() {
        // One cell is what a clock digit or a notification dot looks like.
        let activity = Activity::from_frames(&blinking((2, 2, 8, 7), 6)).unwrap();

        assert_eq!(activity.retarget(2100.0, 820.0, WIDTH, HEIGHT), None);
    }

    #[test]
    fn frames_of_the_wrong_size_are_ignored() {
        let frames = vec![vec![0u8; 10], vec![0u8; 10]];
        assert!(Activity::from_frames(&frames).is_none());
    }

    #[test]
    fn a_drag_is_followed_between_clicks() {
        let mut samples = vec![(1.0, 720.0, 720.0)];
        for step in 1..=20 {
            let t = 1.0 + step as f64 * 0.1;
            samples.push((t, 720.0 + step as f64 * 20.0, 720.0 + step as f64 * 10.0));
        }

        let points = drag_targets(&samples, 1.0, 1.9, WIDTH);

        assert!(points.len() >= 5, "points were {points:?}");
        assert!(points.windows(2).all(|pair| pair[0].0 <= pair[1].0));
        let (_, x, y) = *points.last().unwrap();
        assert!(x > 1000.0, "last x was {x}");
        assert!(y > 900.0, "last y was {y}");
    }

    #[test]
    fn a_single_jump_is_not_a_drag() {
        let samples = vec![
            (1.0, 720.0, 720.0),
            (1.05, 2000.0, 1200.0),
            (1.6, 2000.0, 1200.0),
        ];

        assert!(drag_targets(&samples, 1.0, 1.9, WIDTH).is_empty());
    }

    #[test]
    fn a_move_after_the_drag_window_is_not_a_drag() {
        let mut samples = Vec::new();
        for step in 0..=10 {
            samples.push((1.0 + step as f64 * 0.1, 720.0, 720.0));
        }
        for step in 1..=10 {
            samples.push((2.0 + step as f64 * 0.1, 720.0 + step as f64 * 30.0, 720.0));
        }

        assert!(drag_targets(&samples, 1.0, 1.9, WIDTH).is_empty());
    }
}
