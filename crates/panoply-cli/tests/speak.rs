//! End to end checks of `panoply audio speak` and `panoply audio voices`. The binary runs as a
//! separate process, so the system voice's main thread is the process's own. The audio is
//! written to a file and measured, never played. macOS and Windows only: Linux has no engine.
#![cfg(any(target_os = "macos", windows))]

use std::process::Command;

fn panoply(args: &[&str]) -> (String, String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_panoply"))
        .args(args)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.success(),
    )
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("panoply-cli-speak-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn speak_writes_a_plausible_non_silent_wav_from_text() {
    let dir = scratch("text");
    let (out, err, ok) = panoply(&[
        "audio",
        "speak",
        "--text",
        "Panoply reads text aloud. This sentence is only measured, never played.",
        "--out",
        dir.to_str().unwrap(),
    ]);
    assert!(ok, "{err}");
    let wav = dir.join("speech.wav");
    let (secs, rms) = panoply_tts::wav_stats(&wav).unwrap();
    println!("cli: {secs:.2} s, rms {rms:.4}\n{out}{err}");
    assert!((0.5..=20.0).contains(&secs), "duration {secs}");
    assert!(rms > 0.005, "rms {rms} is silence");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn speak_reads_a_markdown_file_next_to_it_and_voices_lists_the_os_voices() {
    let dir = scratch("md");
    let md = dir.join("note.md");
    std::fs::write(
        &md,
        "# Note\n\nHello from a **Markdown** file. Nothing is played.\n",
    )
    .unwrap();
    let (_, err, ok) = panoply(&["audio", "speak", md.to_str().unwrap()]);
    assert!(ok, "{err}");
    let (secs, rms) = panoply_tts::wav_stats(&dir.join("note (spoken).wav")).unwrap();
    assert!((0.5..=20.0).contains(&secs) && rms > 0.005, "{secs} {rms}");
    let (voices, err, ok) = panoply(&["audio", "voices"]);
    assert!(ok, "{err}");
    assert!(voices.lines().count() >= 1, "{voices}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The name and identifier of a listed voice that really renders sound here, and keeps doing so.
/// A machine may list voices it cannot speak with (not downloaded, or not usable for synthesis).
/// On a GitHub macOS runner the super-compact voices also come and go between processes while
/// voice data loads: one process lists and speaks with one, the next cannot find it. So the more
/// stable families are tried first, super-compact last, and a voice is taken only after it speaks
/// in two separate processes in a row.
fn usable_voice() -> (String, String) {
    let (voices, err, ok) = panoply(&["audio", "voices"]);
    assert!(ok, "{err}");
    // One folder per call: two tests of a process may probe at once.
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let probe = scratch(&format!("probe{n}"));
    let speaks = |id: &str| {
        let (_, _, ok) = panoply(&[
            "audio",
            "speak",
            "--text",
            "Hi.",
            "--voice",
            id,
            "--out",
            probe.to_str().unwrap(),
        ]);
        let spoke = ok
            && panoply_tts::wav_stats(&probe.join("speech.wav")).is_ok_and(|(_, rms)| rms > 0.005);
        let _ = std::fs::remove_file(probe.join("speech.wav"));
        spoke
    };
    let rank = |id: &str| match id {
        _ if id.contains(".super-compact.") => 2,
        _ if id.contains(".voice.compact.") || id.contains(".speech.synthesis.voice.") => 0,
        _ => 1,
    };
    let mut listed: Vec<(&str, &str)> = voices
        .lines()
        .filter_map(|line| {
            let cols: Vec<&str> = line.split('\t').collect();
            (cols.len() > 3).then(|| (cols[0], cols[3]))
        })
        .collect();
    // A stable sort keeps the system's order inside each family.
    listed.sort_by_key(|(_, id)| rank(id));
    let mut tried = Vec::new();
    for (name, id) in listed.into_iter().take(12) {
        if speaks(id) && speaks(id) {
            println!("voice for the test: {name} ({id})");
            let _ = std::fs::remove_dir_all(&probe);
            return (name.to_string(), id.to_string());
        }
        tried.push(id.to_string());
    }
    panic!("none of the listed voices rendered sound twice in a row: {tried:?}");
}

#[test]
fn a_named_voice_is_used_a_missing_one_is_refused_and_speed_changes_the_length() {
    let (_, id) = usable_voice();
    let dir = scratch("voice");
    let d = dir.to_str().unwrap();
    let (_, err, ok) = panoply(&[
        "audio", "speak", "--text", "Hello.", "--voice", &id, "--out", d,
    ]);
    assert!(ok, "{err}");
    assert!(panoply_tts::wav_stats(&dir.join("speech.wav")).unwrap().1 > 0.005);
    let (_, err, ok) = panoply(&[
        "audio",
        "speak",
        "--text",
        "Hello.",
        "--voice",
        "no-such-voice-xyz",
        "--out",
        d,
    ]);
    assert!(!ok && err.contains("no-such-voice-xyz"), "{err}");

    let text =
        "The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs.";
    let mut secs = Vec::new();
    for (k, speed) in ["0.7", "1.0", "1.6"].iter().enumerate() {
        let sub = dir.join(format!("s{k}"));
        std::fs::create_dir_all(&sub).unwrap();
        let (_, err, ok) = panoply(&[
            "audio",
            "speak",
            "--text",
            text,
            "--speed",
            speed,
            "--out",
            sub.to_str().unwrap(),
        ]);
        assert!(ok, "{err}");
        secs.push(panoply_tts::wav_stats(&sub.join("speech.wav")).unwrap().0);
    }
    println!(
        "0.7x {:.2} s, 1.0x {:.2} s, 1.6x {:.2} s",
        secs[0], secs[1], secs[2]
    );
    assert!(secs[0] > secs[1] && secs[1] > secs[2], "{secs:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn long_text_is_read_in_parts_with_progress() {
    let dir = scratch("long");
    let text = "This is one more sentence of a long document. ".repeat(40);
    let (_, err, ok) = panoply(&[
        "audio",
        "speak",
        "--text",
        &text,
        "--out",
        dir.to_str().unwrap(),
    ]);
    assert!(ok, "{err}");
    assert!(
        err.contains("Spoke part 3 of 4") || err.contains("of 4"),
        "{err}"
    );
    let (secs, rms) = panoply_tts::wav_stats(&dir.join("speech.wav")).unwrap();
    assert!(secs > 30.0 && rms > 0.005, "{secs} {rms}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A voice given by its plain name (any case), as `panoply audio voices` prints it.
#[cfg(target_os = "macos")]
#[test]
fn a_voice_given_by_name_works() {
    let (name, _) = usable_voice();
    let name = name.to_lowercase();
    let dir = scratch("byname");
    let (_, err, ok) = panoply(&[
        "audio",
        "speak",
        "--text",
        "Hello.",
        "--voice",
        &name,
        "--out",
        dir.to_str().unwrap(),
    ]);
    assert!(ok, "{err}");
    assert!(panoply_tts::wav_stats(&dir.join("speech.wav")).unwrap().1 > 0.005);
    let _ = std::fs::remove_dir_all(&dir);
}
