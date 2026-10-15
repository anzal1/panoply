//! End to end checks of the `panoply` binary.

use std::process::Command;

fn panoply(args: &[&str]) -> (String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_panoply"))
        .args(args)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.success(),
    )
}

#[test]
fn route_prints_the_chain() {
    assert_eq!(
        panoply(&["route", "docx", "jpg"]),
        ("docx -> pdf -> jpg\n".into(), true)
    );
    assert_eq!(panoply(&["route", "heic", "jpg"]).0, "heic -> jpg\n");
}

#[test]
fn route_flags_model_steps_and_missing_routes() {
    let (out, ok) = panoply(&["route", "mp3", "srt"]);
    assert!(ok && out.starts_with("mp3 -> srt"));
    let (out, ok) = panoply(&["route", "stl", "mp3"]);
    assert!(!ok && out.contains("No route"));
}

#[test]
fn formats_filters_by_family() {
    let (out, ok) = panoply(&["formats", "--family", "audio"]);
    assert!(ok);
    assert_eq!(out.lines().count(), 23);
    assert!(out.lines().all(|l| l.contains("Audio")));
}

#[test]
fn hash_matches_sha256sum_of_abc() {
    let path = std::env::temp_dir().join(format!("panoply-cli-{}.txt", std::process::id()));
    std::fs::write(&path, "abc").unwrap();
    let (out, ok) = panoply(&["hash", path.to_str().unwrap()]);
    std::fs::remove_file(&path).unwrap();
    assert!(ok);
    assert!(out.starts_with("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  "));
}

#[test]
fn modules_lists_builtin_and_examples() {
    let (out, ok) = panoply(&["modules"]);
    assert!(ok);
    assert!(out.contains("files.hash") && out.contains("built-in"));
    assert!(out.contains("pdf.compress") && out.contains("image.resize_to_kb"));
}

#[test]
fn version_prints() {
    let (out, ok) = panoply(&["version"]);
    assert!(ok && out.starts_with("panoply "));
}

// ---- `panoply run`: community modules built for wasm32-wasip2 -------------------------------

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// `cargo`, at low priority where the platform has `nice`. Windows has no `nice`; the copy that
/// Git for Windows puts on PATH eats the backslashes in the arguments it forwards.
fn low_priority_cargo() -> Command {
    #[cfg(unix)]
    {
        let mut c = Command::new("nice");
        c.args(["-n", "15"]).arg(env!("CARGO"));
        c
    }
    #[cfg(not(unix))]
    {
        Command::new(env!("CARGO"))
    }
}

/// Build an example module project once per test run, at low priority on three jobs.
fn build_module(name: &str) -> PathBuf {
    static BUILD: Mutex<()> = Mutex::new(());
    let _one_at_a_time = BUILD.lock().unwrap_or_else(|p| p.into_inner());
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../modules/examples-wasm")
        .join(name);
    let status = low_priority_cargo()
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-wasip2",
            "-j3",
            "--quiet",
        ])
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        // A target folder set for the host build (CI sets one) would take the module out of the
        // project folder the tests look in.
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .unwrap();
    assert!(
        status.success(),
        "build failed (rustup target add wasm32-wasip2?)"
    );
    dir.join("module.toml")
}

fn panoply_full(args: &[&str]) -> (String, String, bool) {
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

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("panoply-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn run_reports_from_a_pattern_i_module() {
    let module = build_module("word-count");
    let d = scratch("wc");
    let f = d.join("poem.txt");
    std::fs::write(&f, "roses are red\nviolets are blue\n").unwrap();
    let (out, _, ok) = panoply_full(&["run", module.to_str().unwrap(), f.to_str().unwrap()]);
    assert!(ok);
    assert_eq!(out, "poem.txt: 2 lines, 6 words, 31 characters, 31 bytes\n");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn run_writes_next_to_the_original_or_into_out_and_never_overwrites() {
    let module = build_module("slugify-lines");
    let module = module.to_str().unwrap();
    let d = scratch("slug");
    let f = d.join("t.txt");
    std::fs::write(&f, "Hello There\n").unwrap();
    let f = f.to_str().unwrap();
    let (out, _, ok) = panoply_full(&["run", module, f, "--opt", "separator=_"]);
    assert!(ok && out.contains("t (slugified).txt"), "{out}");
    assert_eq!(
        std::fs::read_to_string(d.join("t (slugified).txt")).unwrap(),
        "hello_there"
    );
    let (out, _, _) = panoply_full(&["run", module, f]);
    assert!(out.contains("t (slugified 2).txt"), "{out}");
    let to = d.join("results");
    std::fs::create_dir(&to).unwrap();
    let (out, _, ok) = panoply_full(&["run", module, f, "--out", to.to_str().unwrap()]);
    assert!(ok && out.contains("results"), "{out}");
    assert!(to.join("t (slugified).txt").is_file());
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn run_explains_mistakes_in_plain_words_and_fails() {
    let module = build_module("slugify-lines");
    let module = module.to_str().unwrap();
    let d = scratch("mistakes");
    let f = d.join("t.txt");
    std::fs::write(&f, "x").unwrap();
    let f = f.to_str().unwrap();
    let (_, err, ok) = panoply_full(&["run", module, f, "--opt", "colour=red"]);
    assert!(
        !ok && err.contains("not an option") && err.contains("separator"),
        "{err}"
    );
    let (_, err, ok) = panoply_full(&["run", module]);
    assert!(!ok && err.contains("at least one file"), "{err}");
    let (_, err, ok) = panoply_full(&["run", d.join("missing/module.toml").to_str().unwrap(), f]);
    assert!(!ok && err.contains("Could not read"), "{err}");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn modules_lists_community_modules() {
    let (out, ok) = panoply(&["modules"]);
    assert!(ok);
    assert!(out.contains("text.slugify_lines") && out.contains("text.word_count"));
    assert!(out.contains("community"));
}

// ---- the community plugins: real inputs through the real sandbox ----------------------------

/// Copy a real input from a module's `tests/fixtures` into a scratch folder and return its path.
fn fixture(module: &str, name: &str, to: &Path) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../modules/examples-wasm")
        .join(module)
        .join("tests/fixtures")
        .join(name);
    let dest = to.join(name);
    std::fs::copy(&from, &dest).unwrap_or_else(|e| panic!("{}: {e}", from.display()));
    dest
}

#[test]
fn data_format_tidies_converts_and_checks() {
    let module = build_module("data-format");
    let module = module.to_str().unwrap();
    let d = scratch("dataformat");
    let pkg = fixture("data-format", "package.json", &d);
    let (out, err, ok) = panoply_full(&["run", module, pkg.to_str().unwrap(), "--opt", "indent=4"]);
    assert!(ok && out.contains("package (formatted).json"), "{out}{err}");
    let tidy = std::fs::read_to_string(d.join("package (formatted).json")).unwrap();
    assert!(
        tidy.starts_with("{\n    \"name\": \"left-pad\",\n    \"version\""),
        "{tidy}"
    );

    let compose = fixture("data-format", "docker-compose.yml", &d);
    let (out, err, ok) =
        panoply_full(&["run", module, compose.to_str().unwrap(), "--opt", "to=toml"]);
    assert!(
        ok && out.contains("docker-compose (toml).toml"),
        "{out}{err}"
    );
    let toml = std::fs::read_to_string(d.join("docker-compose (toml).toml")).unwrap();
    assert!(
        toml.contains("[services.web]") && toml.contains("image = \"nginx:1.27\""),
        "{toml}"
    );

    let cargo = fixture("data-format", "Cargo.toml", &d);
    let (out, err, ok) = panoply_full(&[
        "run",
        module,
        cargo.to_str().unwrap(),
        "--opt",
        "to=json",
        "--opt",
        "sort_keys=true",
    ]);
    assert!(ok, "{out}{err}");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.join("Cargo (json).json")).unwrap())
            .unwrap();
    assert_eq!(json["bin"].as_array().unwrap().len(), 2);
    assert_eq!(json["package"]["released"], "2026-10-16");

    let (out, _, ok) = panoply_full(&[
        "run",
        module,
        cargo.to_str().unwrap(),
        "--opt",
        "check_only=true",
    ]);
    assert!(ok && out == "Cargo.toml: valid TOML\n", "{out}");

    let broken = fixture("data-format", "broken.json", &d);
    let (_, err, ok) = panoply_full(&["run", module, broken.to_str().unwrap()]);
    assert!(
        !ok && err.contains("broken.json is not valid JSON. Line 3, column 14"),
        "{err}"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn markdown_to_html_makes_a_printable_page() {
    let module = build_module("markdown-to-html");
    let module = module.to_str().unwrap();
    let d = scratch("markdown");
    let notes = fixture("markdown-to-html", "notes.markdown", &d);
    let (out, err, ok) = panoply_full(&[
        "run",
        module,
        notes.to_str().unwrap(),
        "--opt",
        "toc=true",
        "--opt",
        "paper=Letter",
    ]);
    assert!(ok && out.contains("notes (html).html"), "{out}{err}");
    let html = std::fs::read_to_string(d.join("notes (html).html")).unwrap();
    assert!(html.contains("<title>Trip notes: Goa, December</title>"));
    assert!(html.contains("size: Letter"));
    assert!(html.contains("<a href=\"#budget\">Budget</a>"));
    assert!(html.contains("Cost (INR)</th>"));
    assert!(html.contains("<pre><code class=\"language-rust\">"));
    assert!(html.contains("type=\"checkbox\""));
    // No script can run on the page, whatever the Markdown held.
    assert!(html.contains("default-src 'none'"));
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn qr_code_makes_real_pngs_and_svgs() {
    let module = build_module("qr-code");
    let module = module.to_str().unwrap();
    let d = scratch("qr");
    let links = fixture("qr-code", "links.txt", &d);
    let (out, err, ok) = panoply_full(&["run", module, links.to_str().unwrap()]);
    assert!(ok, "{out}{err}");
    for n in 1..=3 {
        let bytes = std::fs::read(d.join(format!("links (qr {n}).png"))).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }
    let contact = fixture("qr-code", "contact.vcf", &d);
    let (out, err, ok) = panoply_full(&[
        "run",
        module,
        contact.to_str().unwrap(),
        "--opt",
        "format=svg",
        "--opt",
        "lines=whole_file",
    ]);
    assert!(ok && out.contains("contact (qr).svg"), "{out}{err}");
    let svg = std::fs::read_to_string(d.join("contact (qr).svg")).unwrap();
    assert!(svg.starts_with("<svg ") && svg.contains("<path"));
    // Colours a phone cannot read are refused before anything is written.
    let (_, err, ok) = panoply_full(&[
        "run",
        module,
        contact.to_str().unwrap(),
        "--opt",
        "dark=#999999",
        "--opt",
        "light=#aaaaaa",
    ]);
    assert!(!ok && err.contains("too close in colour"), "{err}");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn modules_lists_the_community_plugins() {
    let (out, ok) = panoply(&["modules"]);
    assert!(ok);
    for id in ["data.format", "doc.markdown_to_html", "image.qr_code"] {
        assert!(out.contains(id), "{id} missing from:\n{out}");
    }
}

/// The folders and ids in `modules/shipped.txt`.
fn shipped() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/shipped.txt"),
    )
    .unwrap();
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut w = l.split_whitespace();
            (w.next().unwrap().to_string(), w.next().unwrap().to_string())
        })
        .collect()
}

#[test]
fn the_shipped_list_is_the_three_plugins_and_no_example() {
    let list = shipped();
    let ids: Vec<&str> = list.iter().map(|(_, id)| id.as_str()).collect();
    assert_eq!(
        ids,
        ["doc.markdown_to_html", "data.format", "image.qr_code"]
    );
    for (folder, id) in &list {
        let manifest = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../modules/examples-wasm")
                .join(folder)
                .join("module.toml"),
        )
        .unwrap();
        assert!(
            manifest.contains(&format!("id = \"{id}\"")),
            "{folder} is not {id}"
        );
    }
}

#[test]
fn a_shipped_module_folder_is_found_by_the_cli_from_anywhere() {
    // The release layout: a folder of module folders, found through the search path, listed from a
    // folder that is not a source checkout.
    let module = build_module("qr-code");
    let src = module.parent().unwrap();
    let d = scratch("shipped");
    let dest = d.join("modules/qr-code");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::copy(src.join("module.toml"), dest.join("module.toml")).unwrap();
    std::fs::copy(
        src.join("target/wasm32-wasip2/release/module.wasm"),
        dest.join("module.wasm"),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_panoply"))
        .arg("modules")
        .env("PANOPLY_MODULES", d.join("modules"))
        .env("PANOPLY_DATA_DIR", d.join("data"))
        .current_dir(&d)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    assert!(
        text.contains("image.qr_code") && text.contains("QR code maker"),
        "{text}"
    );
    assert!(
        !text.contains("text.word_count"),
        "an example showed up from nowhere:\n{text}"
    );
    std::fs::remove_dir_all(&d).unwrap();
}
