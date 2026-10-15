//! The app window's remembered size and position, kept in the data folder as a tiny JSON file.
//!
//! Saving is cheap and debounced by the caller; loading happens once, before the window opens.
//! A saved position that no longer fits the displays (a monitor was unplugged) is moved back
//! onto one, and a saved size is held between the window's minimum and the display.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A window's frame in screen points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
    /// Whether the window was maximized (the frame is then the size to restore to).
    #[serde(default)]
    pub maximized: bool,
}

/// A display's frame in screen points: x, y, width, height.
pub type Display = (f32, f32, f32, f32);

fn file(name: &str) -> PathBuf {
    crate::paths::data_dir().join(format!("window-{name}.json"))
}

/// The saved frame of the window called `name`, if there is one.
pub fn load(name: &str) -> Option<Saved> {
    let text = std::fs::read_to_string(file(name)).ok()?;
    let s: Saved = serde_json::from_str(&text).ok()?;
    [s.x, s.y, s.w, s.h]
        .iter()
        .all(|v| v.is_finite())
        .then_some(s)
}

/// Remember the frame of the window called `name`. Failures are ignored: it is a convenience.
pub fn save(name: &str, s: &Saved) {
    let path = file(name);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(s) {
        let _ = std::fs::write(path, text);
    }
}

/// Room kept around a window that was saved or opened on a display, in points: the frame the
/// system draws outside the client area (about 15 by 7 points on Windows) and a margin so the
/// close button and the bottom edge stay on screen.
const EDGE: f32 = 32.0;

/// What the system frame adds to the right of and below the client area, in points, so a window
/// moved against the right or bottom edge keeps its outer frame on the display too.
const FRAME: (f32, f32) = (16.0, 8.0);

/// Fit a saved frame to the displays that exist now, all in points (the unit GPUI reports display
/// bounds in, so a 1920 by 1080 screen at 150% scaling is 1280 by 720). The frame is moved onto
/// the display that holds most of its top edge (else the first one) and shrunk to fit it, never
/// below `min`, so the whole window, close button included, is reachable. That covers a monitor
/// that was unplugged and a display scale that was raised since the frame was saved.
/// With no displays the frame is returned unchanged.
pub fn fit(saved: Saved, min: (f32, f32), displays: &[Display]) -> Saved {
    let Some(&first) = displays.first() else {
        return saved;
    };
    // The display holding most of the window's top edge, else the first one.
    let cx = saved.x + saved.w / 2.0;
    let home = displays
        .iter()
        .copied()
        .find(|d| cx >= d.0 && cx < d.0 + d.2 && saved.y >= d.1 - 1.0 && saved.y < d.1 + d.3)
        .unwrap_or(first);
    let w = saved.w.min(home.2 - EDGE).max(min.0);
    let h = saved.h.min(home.3 - EDGE).max(min.1);
    let x = saved
        .x
        .clamp(home.0, (home.0 + home.2 - w - FRAME.0).max(home.0));
    let y = saved
        .y
        .clamp(home.1, (home.1 + home.3 - h - FRAME.1).max(home.1));
    Saved {
        x,
        y,
        w,
        h,
        maximized: saved.maximized,
    }
}

/// The frame for a first launch, when nothing is saved: the default size at the default spot,
/// shrunk to leave room for a taskbar and some desktop on a small display (a 1024 by 768 screen,
/// or a 1920 by 1080 one at 150% scaling, which is 1280 by 720 points), never below `min`.
/// The whole window ends up on the display, so the caption buttons are reachable.
pub fn first_frame(
    origin: (f32, f32),
    size: (f32, f32),
    min: (f32, f32),
    displays: &[Display],
) -> Saved {
    let Some(&d) = displays.first() else {
        return Saved {
            x: origin.0,
            y: origin.1,
            w: size.0,
            h: size.1,
            maximized: false,
        };
    };
    let w = size.0.min(d.2 - 80.0).max(min.0);
    let h = size.1.min(d.3 - 140.0).max(min.1);
    let x = origin.0.min(d.0 + d.2 - w).max(d.0);
    let y = origin.1.min(d.1 + d.3 - h - 48.0).max(d.1);
    Saved {
        x,
        y,
        w,
        h,
        maximized: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D1: Display = (0.0, 0.0, 1512.0, 982.0);
    const D2: Display = (1512.0, 0.0, 1920.0, 1080.0);

    fn s(x: f32, y: f32, w: f32, h: f32) -> Saved {
        Saved {
            x,
            y,
            w,
            h,
            maximized: false,
        }
    }

    #[test]
    fn first_launch_keeps_the_default_on_a_roomy_display() {
        let f = first_frame((180.0, 90.0), (1160.0, 720.0), (860.0, 560.0), &[D1]);
        assert_eq!(f, s(180.0, 90.0, 1160.0, 720.0));
    }

    #[test]
    fn first_launch_fits_a_small_display() {
        let small: Display = (0.0, 0.0, 1024.0, 768.0);
        let f = first_frame((180.0, 90.0), (1160.0, 720.0), (860.0, 560.0), &[small]);
        assert!(f.x >= 0.0 && f.x + f.w <= 1024.0, "{f:?}");
        assert!(f.y >= 0.0 && f.y + f.h + 48.0 <= 768.0, "{f:?}");
        // 1280 by 720 points (1080p at 150%): it can not shrink below the minimum height.
        let hd: Display = (0.0, 0.0, 1280.0, 720.0);
        let g = first_frame((180.0, 90.0), (1160.0, 720.0), (860.0, 560.0), &[hd]);
        assert!(g.h >= 560.0 && g.y + g.h <= 720.0, "{g:?}");
    }

    #[test]
    fn a_frame_that_fits_is_kept() {
        let f = s(180.0, 90.0, 1160.0, 720.0);
        assert_eq!(fit(f, (860.0, 560.0), &[D1]), f);
    }

    #[test]
    fn a_window_on_an_unplugged_display_comes_back() {
        let f = fit(s(2000.0, 100.0, 1160.0, 720.0), (860.0, 560.0), &[D1]);
        assert!(f.x + f.w >= 120.0 && f.x <= D1.0 + D1.2 - 120.0);
        assert!(f.x >= D1.0 - f.w + 120.0 && f.x + 120.0 <= D1.2);
        assert!(f.y >= 0.0 && f.y < D1.3);
    }

    #[test]
    fn a_second_display_is_honoured_when_it_exists() {
        let f = s(1700.0, 80.0, 1000.0, 700.0);
        assert_eq!(fit(f, (860.0, 560.0), &[D1, D2]), f);
    }

    #[test]
    fn size_is_held_between_the_minimum_and_the_display() {
        let tiny = fit(s(10.0, 10.0, 100.0, 100.0), (860.0, 560.0), &[D1]);
        assert_eq!((tiny.w, tiny.h), (860.0, 560.0));
        let huge = fit(s(10.0, 10.0, 9000.0, 9000.0), (860.0, 560.0), &[D1]);
        assert_eq!((huge.w, huge.h), (1512.0 - EDGE, 982.0 - EDGE));
    }

    /// A 1920 by 1080 screen at 150% scaling is 1280 by 720 points. The frame saved at 100% (1160 by
    /// 720 at 180,90, which is 1740 by 1080 physical pixels at 150%) used to come back as it was and
    /// ran off the right and bottom edges, with the close button out of sight.
    #[test]
    fn a_frame_saved_at_100_percent_fits_the_same_screen_at_150_percent() {
        let scale = 1.5;
        let physical = (1920.0f32, 1080.0f32);
        // The work area: the 40 pixel taskbar is not part of it.
        let points: Display = (0.0, 0.0, physical.0 / scale, (physical.1 - 40.0) / scale);
        let f = fit(s(180.0, 90.0, 1160.0, 720.0), (860.0, 560.0), &[points]);
        assert!(f.x >= 0.0 && f.y >= 0.0, "{f:?}");
        assert!(f.x + f.w <= points.2 && f.y + f.h <= points.3, "{f:?}");
        // In physical pixels, with the system frame (15 by 7 points) on top of it, above the taskbar.
        let (right, bottom) = ((f.x + f.w + 15.0) * scale, (f.y + f.h + 7.0) * scale);
        assert!(
            right <= physical.0 && bottom <= physical.1 - 40.0,
            "{right} x {bottom}"
        );
        // The same frame at 100% (1920 by 1080 points) is left alone.
        let big: Display = (0.0, 0.0, 1920.0, 1080.0);
        let g = fit(s(180.0, 90.0, 1160.0, 720.0), (860.0, 560.0), &[big]);
        assert_eq!(g, s(180.0, 90.0, 1160.0, 720.0));
    }

    /// The first launch on that screen: the default frame shrinks to the points, not the pixels.
    #[test]
    fn first_launch_at_150_percent_on_1080p_fits_in_points_and_pixels() {
        let scale = 1.5;
        let points: Display = (0.0, 0.0, 1920.0 / scale, 1040.0 / scale);
        let f = first_frame((180.0, 90.0), (1160.0, 720.0), (860.0, 560.0), &[points]);
        let (right, bottom) = ((f.x + f.w + 15.0) * scale, (f.y + f.h + 7.0) * scale);
        assert!(
            right <= 1920.0 && bottom <= 1040.0,
            "{f:?} -> {right} x {bottom}"
        );
    }

    #[test]
    fn a_window_partly_off_the_right_edge_is_pulled_back_whole() {
        let f = fit(s(1000.0, 300.0, 1160.0, 600.0), (860.0, 560.0), &[D1]);
        assert_eq!((f.x, f.y), (1512.0 - 1160.0 - FRAME.0, 300.0));
    }

    #[test]
    fn the_title_bar_never_hides_above_the_display() {
        let f = fit(s(100.0, -500.0, 1000.0, 700.0), (860.0, 560.0), &[D1]);
        assert_eq!(f.y, 0.0);
    }

    #[test]
    fn with_no_displays_nothing_changes() {
        let f = s(5.0, 5.0, 900.0, 600.0);
        assert_eq!(fit(f, (860.0, 560.0), &[]), f);
    }
}
