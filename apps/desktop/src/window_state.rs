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

/// Fit a saved frame to the displays that exist now. The size stays between `min` and the
/// largest display; the position is moved so that the top edge of the window (where the title
/// and the traffic lights are) is on a display and at least 120 points of it are reachable.
/// With no displays the frame is returned unchanged.
pub fn fit(saved: Saved, min: (f32, f32), displays: &[Display]) -> Saved {
    let Some(&first) = displays.first() else {
        return saved;
    };
    let biggest = displays
        .iter()
        .fold((first.2, first.3), |m, d| (m.0.max(d.2), m.1.max(d.3)));
    let w = saved.w.clamp(min.0, biggest.0.max(min.0));
    let h = saved.h.clamp(min.1, biggest.1.max(min.1));
    // The display holding most of the window's top edge, else the first one.
    let cx = saved.x + w / 2.0;
    let home = displays
        .iter()
        .copied()
        .find(|d| cx >= d.0 && cx < d.0 + d.2 && saved.y >= d.1 - 1.0 && saved.y < d.1 + d.3)
        .unwrap_or(first);
    let reach = 120.0f32.min(w);
    let x = saved.x.clamp(home.0 - w + reach, home.0 + home.2 - reach);
    let y = saved.y.clamp(home.1, (home.1 + home.3 - 60.0).max(home.1));
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
        assert_eq!((huge.w, huge.h), (1512.0, 982.0));
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
