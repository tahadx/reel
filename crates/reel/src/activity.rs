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

/// Samples a take around a block and answers where the camera should point.
pub struct Refiner {
    video: PathBuf,
    log: PathBuf,
    offset: f64,
}

impl Refiner {
    /// `trim_start` converts saved-take time back to raw capture time.
    pub fn new(paths: &Paths, video: &Path, trim_start: f64) -> Self {
        Self {
            video: video.to_path_buf(),
            log: paths.log.clone(),
            offset: trim_start.max(0.0),
        }
    }

    /// The centre for a block that eases in at `enter` and holds until `hold`,
    /// given the click centroid. `None` keeps the click.
    pub fn target(
        &self,
        enter: f64,
        hold: f64,
        cx: f64,
        cy: f64,
        width: f64,
        height: f64,
    ) -> Option<(f64, f64)> {
        let start = (enter + self.offset).max(0.0);
        let window = (hold + self.offset - start).clamp(0.0, MAX_WINDOW_SECS);
        let frames = self.sample(start, window)?;
        let activity = Activity::from_frames(&frames)?;
        let target = activity.retarget(cx, cy, width, height)?;
        paths::log_to(
            &self.log,
            &format!(
                "finalize: zoom target moved from ({cx:.0},{cy:.0}) to ({:.0},{:.0}) at {enter:.2}s; the click area was quiet",
                target.0, target.1
            ),
        );
        Some(target)
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
}
