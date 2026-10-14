//! `panoply models`: what Panoply may download, what is on this computer, and the consent
//! behind it. Also the prompt that asks before a download starts.
//!
//! Nothing is downloaded without a yes per item. The yes is remembered in `consent.json` in the
//! data folder; `remove` deletes the files and forgets the yes.

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;
use std::sync::Arc;

use clap::Subcommand;
use panoply_core::assets::{self, Answer, Asker, AssetInfo, format_size};
use panoply_host::assets as host_assets;

/// What to do with the downloadable items.
#[derive(Subcommand)]
pub enum ModelsCmd {
    /// List every model and engine: size, license, whether it is on this computer
    List,
    /// Download items now (ask first, or pass --yes): panoply models fetch isnet pdfium
    Fetch {
        /// Item ids. With none, every item this platform has.
        ids: Vec<String>,
    },
    /// Say yes to a download ahead of time, without downloading: panoply models allow isnet
    Allow {
        /// Item ids, or "all"
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Delete an item from this computer and forget the yes: panoply models remove isnet
    Remove {
        /// Item ids, or "all"
        #[arg(required = true)]
        ids: Vec<String>,
    },
}

/// Asks on the terminal before a download.
struct PromptAsker;

impl Asker for PromptAsker {
    fn ask(&self, info: &AssetInfo) -> Answer {
        prompt(&mut std::io::stdin().lock(), &mut std::io::stderr(), info)
    }
}

/// Write the question to `out`, read one line from `input`. Only `y` or `yes` (any case) is a
/// yes: an empty line, anything else, or a closed input is a no.
fn prompt(input: &mut dyn BufRead, out: &mut dyn Write, info: &AssetInfo) -> Answer {
    let _ = writeln!(out);
    let _ = writeln!(out, "Panoply needs to download the {}.", info.name);
    let _ = writeln!(out, "  What for:  {}", info.purpose);
    let _ = writeln!(out, "  Size:      {}, once", info.size_label());
    let _ = writeln!(out, "  From:      {}", info.source);
    let _ = writeln!(out, "  License:   {}", info.license);
    let _ = writeln!(
        out,
        "It stays on this computer, and none of your files are sent anywhere."
    );
    let _ = write!(out, "Download it? [y/N] ");
    let _ = out.flush();
    let mut line = String::new();
    if input.read_line(&mut line).is_err() {
        return Answer::No;
    }
    if matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Answer::Yes
    } else {
        Answer::No
    }
}

/// Set up consent for this process: `--yes` says yes to everything (and records it per item);
/// on a terminal without it a prompt asks; with neither, a download that was not allowed
/// before stops with a plain error that says how to allow it.
pub fn install(yes: bool) {
    assets::set_assume_yes(yes);
    if !yes && std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        assets::set_asker(Some(Arc::new(PromptAsker)));
    }
}

/// Print the network line a module's manifest asks to be shown before it runs.
pub fn show_network_notice(registry: &panoply_host::Registry, module_id: &str) {
    if let Some(m) = registry.get(module_id) {
        let caps = &m.manifest().capabilities;
        if caps.network {
            eprintln!(
                "Uses the network: {}",
                caps.network_reason.as_deref().unwrap_or("no reason given")
            );
        }
    }
}

fn pick(ids: &[String]) -> Result<Vec<assets::Entry>, String> {
    if ids.is_empty() || ids.iter().any(|i| i == "all") {
        return Ok(host_assets::catalog());
    }
    ids.iter()
        .map(|id| {
            host_assets::find(id).ok_or_else(|| {
                let all: Vec<String> = host_assets::catalog()
                    .iter()
                    .map(|e| e.info.id.clone())
                    .collect();
                format!(
                    "\"{id}\" is not something Panoply downloads. Try one of: {}.",
                    all.join(", ")
                )
            })
        })
        .collect()
}

fn list() -> ExitCode {
    println!(
        "Models and engines are downloaded once, only after you allow each one, and kept in {}.",
        panoply_vision::cache::engines_dir().display()
    );
    println!(
        "Your yes is remembered in {}.\n",
        assets::consent_file().display()
    );
    println!("{:<22} {:>7}  {:<28} state", "id", "size", "license");
    for e in host_assets::catalog() {
        let st = e.status();
        let state = match (st.installed, st.allowed) {
            (true, _) => format!("on this computer, {}", format_size(st.bytes)),
            (false, true) => "allowed, not downloaded yet".to_string(),
            (false, false) => "not downloaded".to_string(),
        };
        println!(
            "{:<22} {:>7}  {:<28} {}",
            e.info.id,
            e.info.size_label(),
            e.info.license,
            state
        );
        println!("{:<22} {}; {}", "", e.info.name, e.info.purpose);
    }
    println!(
        "\nOn macOS the system's own engines are used first, so most of these are only fetched when the system has none."
    );
    ExitCode::SUCCESS
}

/// Run `panoply models`.
pub fn run(cmd: Option<ModelsCmd>) -> ExitCode {
    match cmd.unwrap_or(ModelsCmd::List) {
        ModelsCmd::List => list(),
        ModelsCmd::Fetch { ids } => {
            let wanted = match pick(&ids) {
                Ok(v) => v,
                Err(m) => {
                    eprintln!("{m}");
                    return ExitCode::FAILURE;
                }
            };
            let mut failed = false;
            for e in wanted {
                let mut notice = |s: &str| eprintln!("{s}");
                match host_assets::fetch(&e.info.id, &mut notice) {
                    Ok(p) => println!("{}: ready at {}", e.info.id, p.display()),
                    Err(m) => {
                        eprintln!("{}: {m}", e.info.id);
                        failed = true;
                    }
                }
            }
            if failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        ModelsCmd::Allow { ids } => {
            let wanted = match pick(&ids) {
                Ok(v) => v,
                Err(m) => {
                    eprintln!("{m}");
                    return ExitCode::FAILURE;
                }
            };
            for e in wanted {
                match assets::allow(&e.info.id) {
                    Ok(()) => println!(
                        "{}: allowed ({}, from {}, {} license)",
                        e.info.id,
                        e.info.size_label(),
                        e.info.source,
                        e.info.license
                    ),
                    Err(err) => {
                        eprintln!("Could not save your choice: {err}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            ExitCode::SUCCESS
        }
        ModelsCmd::Remove { ids } => {
            let wanted = match pick(&ids) {
                Ok(v) => v,
                Err(m) => {
                    eprintln!("{m}");
                    return ExitCode::FAILURE;
                }
            };
            for e in wanted {
                match e.remove() {
                    Ok(0) => println!("{}: nothing to remove", e.info.id),
                    Ok(n) => println!("{}: removed, {} freed", e.info.id, format_size(n)),
                    Err(err) => {
                        eprintln!("{}: could not remove it ({err})", e.info.id);
                        return ExitCode::FAILURE;
                    }
                }
            }
            ExitCode::SUCCESS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use panoply_core::assets::AssetKind;

    fn info() -> AssetInfo {
        AssetInfo {
            id: "isnet".into(),
            name: "IS-Net background model".into(),
            kind: AssetKind::Model,
            size: 44_229_662,
            source: "huggingface.co".into(),
            license: "MIT".into(),
            purpose: "removes backgrounds".into(),
        }
    }

    fn ask(answer: &str) -> (Answer, String) {
        let mut out = Vec::new();
        let a = prompt(&mut answer.as_bytes(), &mut out, &info());
        (a, String::from_utf8(out).unwrap())
    }

    #[test]
    fn the_prompt_names_the_item_its_size_source_and_license() {
        let (_, shown) = ask("n\n");
        for want in [
            "IS-Net background model",
            "44 MB",
            "huggingface.co",
            "MIT",
            "removes backgrounds",
            "none of your files are sent",
            "[y/N]",
        ] {
            assert!(shown.contains(want), "{want} in {shown}");
        }
    }

    #[test]
    fn only_a_clear_yes_downloads() {
        for yes in ["y\n", "Y\n", "yes\n", " YES \n", "y"] {
            assert_eq!(ask(yes).0, Answer::Yes, "{yes:?}");
        }
        for no in ["\n", "n\n", "no\n", "maybe\n", "yep\n", "", "ys\n"] {
            assert_eq!(ask(no).0, Answer::No, "{no:?} is a no (the default)");
        }
    }

    #[test]
    fn a_closed_input_is_a_no() {
        struct Broken;
        impl std::io::Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("closed"))
            }
        }
        let mut input = std::io::BufReader::new(Broken);
        let mut out = Vec::new();
        assert_eq!(prompt(&mut input, &mut out, &info()), Answer::No);
    }
}
