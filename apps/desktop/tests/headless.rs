//! The headless run, as a test: `cargo test -p panoply-desktop --features snapshot`.
//!
//! It starts the real binary in its `--selfcheck` mode, which opens the bar and the app window at
//! x = -20000 (never ordered front, never key), drives them, renders PNGs and writes a report.
//! Nothing is ever shown on screen. Without the `snapshot` feature there is nothing to run.
#![cfg(all(feature = "snapshot", target_os = "macos"))]

use std::process::Command;

#[test]
fn the_headless_run_meets_the_functional_bars() {
    let out = std::env::temp_dir().join(format!("panoply-headless-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let status = Command::new(env!("CARGO_BIN_EXE_panoply-desktop"))
        .args(["--selfcheck", "--out"])
        .arg(&out)
        .output()
        .expect("run the self check");
    assert!(
        status.status.success(),
        "self check failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("selfcheck-report.json")).expect("report"))
            .expect("report is json");

    // A job ran on a worker, through the registry, and the card has real numbers.
    let flow = &report["flow"];
    assert!(
        flow["result_headline"]
            .as_str()
            .is_some_and(|h| h.contains("smaller")),
        "{flow}"
    );
    assert_eq!(flow["undo_trashed_outputs"], true);
    assert_eq!(flow["then_chain"]["panel_closed"], true, "{flow}");
    assert_eq!(
        flow["then_chain"]["files_on_bar"].as_array().map(Vec::len),
        Some(1),
        "{flow}"
    );
    assert_eq!(
        flow["grapheme_patch_backspace_removes_whole_cluster"], true,
        "{flow}"
    );
    assert!(
        flow["instant_transform_png_to_jpg"]["headline"].is_string(),
        "pattern A runs on Return"
    );
    assert!(
        flow["failure"]["next_step"].is_string(),
        "failures name a next step"
    );

    // A tool that needs a download asks first, names what it is, writes nothing while it asks,
    // and Not now tells the job no with nothing recorded.
    let consent = &flow["consent"];
    assert!(
        consent["asked_before_starting"]["size"].is_string(),
        "{consent}"
    );
    assert!(
        consent["asked_before_starting"]["source"].is_string(),
        "{consent}"
    );
    assert!(
        consent["asked_before_starting"]["license"].is_string(),
        "{consent}"
    );
    assert_eq!(consent["nothing_written_while_asking"], true, "{consent}");
    assert_eq!(consent["not_now_tells_the_job_no"], true, "{consent}");
    assert_eq!(consent["not_now_recorded_nothing"], true, "{consent}");

    // Budgets that do not depend on a release build.
    let b = &report["budgets"];
    assert!(b["panel_open_max_ms"].as_f64().unwrap() < 50.0, "{b}");
    assert!(b["idle_cpu_percent"].as_f64().unwrap() < 1.0, "{b}");

    // Every actionable element has a role and a label.
    for state in report["accessibility"].as_array().unwrap() {
        assert_eq!(state["pass"], true, "{state}");
    }

    // Every screen rendered to a PNG.
    for shot in report["screens"].as_array().unwrap() {
        let s = shot.as_str().unwrap();
        assert!(
            !s.starts_with("render failed") && !s.starts_with("save failed") && s != "no window",
            "{s}"
        );
    }
    let _ = std::fs::remove_dir_all(&out);
}
