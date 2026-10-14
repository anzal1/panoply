//! Panoply desktop: the command bar and the app window.
//!
//! The logic (catalog, search, forms, jobs, history) has no UI types and is tested on its own.
//! The GPUI views in `ui` draw it, and nothing in them is specific to any module.

pub mod catalog;
pub mod downloads;
pub mod estimate;
pub mod form;
pub mod fuzzy;
pub mod history;
pub mod intent;
pub mod jobs;
pub mod paths;
pub mod profile;
pub mod search;
pub mod units;

mod icons_gen;
mod platform;
#[cfg(feature = "snapshot")]
mod selfcheck;
mod ui;

/// Run the app. With no arguments this is the normal app; the dev flags need `--features snapshot`.
pub fn run_from_args(args: Vec<String>) {
    #[cfg(feature = "snapshot")]
    {
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
