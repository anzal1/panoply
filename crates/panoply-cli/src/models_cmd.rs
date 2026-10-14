//! `panoply models`: what Panoply may download, what is on this computer, and the consent
//! behind it. Also the prompt that asks before a download starts.
//!
//! Nothing is downloaded without a yes per item. The yes is remembered in `consent.json` in the
//! data folder; `remove` deletes the files and forgets the yes.

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;
use std::sync::Arc;

use clap::Subcommand;
use panoply_core::assets::{self, Answer, AssetInfo, Asker, format_size};
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
        let mut err = std::io::stderr();
        let _ = writeln!(err);
        let _ = writeln!(err, "Panoply needs to download the {}.", info.name);
        let _ = writeln!(err, "  What for:  {}", info.purpose);
        let _ = writeln!(err, "  Size:      {}, once", info.size_label());
        let _ = writeln!(err, "  From:      {}", info.source);
        let _ = writeln!(err, "  License:   {}", info.license);
        let _ = writeln!(
            err,
            "It stays on this computer, and none of your files are sent anywhere."
        );
        let _ = write!(err, "Download it? [y/N] ");
        let _ = err.flush();
        let mut line = String::new();
        if std::io::stdin().lock().read_line(&mut line).is_err() {
            return Answer::No;
        }
        if matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            Answer::Yes
        } else {
            Answer::No
        }
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
    println!("Your yes is remembered in {}.\n", assets::consent_file().display());
    println!(
        "{:<22} {:>7}  {:<26} {}",
        "id", "size", "license", "state"
    );
    for e in host_assets::catalog() {
        let st = e.status();
        let state = match (st.installed, st.allowed) {
            (true, _) => format!("on this computer, {}", format_size(st.bytes)),
            (false, true) => "allowed, not downloaded yet".to_string(),
            (false, false) => "not downloaded".to_string(),
        };
        println!(
            "{:<22} {:>7}  {:<26} {}",
            e.info.id,
            e.info.size_label(),
            e.info.license,
            state
        );
        println!("{:<22} {}; {}", "", e.info.name, e.info.purpose);
    }
    println!("\nOn macOS the system's own engines are used first, so most of these are only fetched when the system has none.");
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
