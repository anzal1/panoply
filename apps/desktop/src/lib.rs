//! Panoply desktop: the command bar and the app window.
//!
//! The logic (catalog, search, forms, jobs, history) has no UI types and is tested on its own.
//! The GPUI views in `ui` draw it, and nothing in them is specific to any module.

pub mod catalog;
pub mod disk_review;
pub mod downloads;
pub mod estimate;
pub mod finder;
pub mod form;
pub mod fuzzy;
pub mod history;
pub mod intent;
pub mod jobs;
pub mod paths;
pub mod profile;
pub mod result_view;
pub mod search;
pub mod share;
pub mod units;
pub mod window_state;

mod icons_gen;
mod platform;
#[cfg(feature = "snapshot")]
mod selfcheck;
mod ui;

/// Run the app. With no arguments this is the normal app; the dev flags need `--features snapshot`.
pub fn run_from_args(args: Vec<String>) {
    #[cfg(feature = "snapshot")]
    {
        if args.iter().any(|a| a == "--selfcheck-find") {
            let out = args
                .iter()
                .position(|a| a == "--out")
                .and_then(|i| args.get(i + 1))
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("screens"));
            selfcheck::run_find(out);
            return;
        }
        if args.iter().any(|a| a == "--selfcheck-idle") {
            let hold = args
                .iter()
                .position(|a| a == "--hold")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(14);
            selfcheck::run_idle(hold);
            return;
        }
        if args.iter().any(|a| a == "--selfcheck-review") {
            let out = args
                .iter()
                .position(|a| a == "--out")
                .and_then(|i| args.get(i + 1))
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("review"));
            selfcheck::run_review(out);
            return;
        }
        if args.iter().any(|a| a == "--selfcheck-userpass") {
            let arg = |name: &str| {
                args.iter()
                    .position(|a| a == name)
                    .and_then(|i| args.get(i + 1))
                    .map(std::path::PathBuf::from)
            };
            selfcheck::run_userpass(
                arg("--out").unwrap_or_else(|| std::path::PathBuf::from("userpass")),
                arg("--manifest").unwrap_or_else(|| std::path::PathBuf::from("manifest.json")),
            );
            return;
        }
        if args.iter().any(|a| a == "--selfcheck-frames") {
            let out = args
                .iter()
                .position(|a| a == "--out")
                .and_then(|i| args.get(i + 1))
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("frames"));
            selfcheck::run_frames(out);
            return;
        }
        if args.iter().any(|a| a == "--selfcheck") {
            let out = args
                .iter()
                .position(|a| a == "--out")
                .and_then(|i| args.get(i + 1))
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("screens"));
            selfcheck::run(out);
            return;
        }
    }
    ui::live::run(args);
}
