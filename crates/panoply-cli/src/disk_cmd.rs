//! `panoply disk <path>`: what is using the space under a folder.
//!
//! A thin printer over `panoply_disk::scan`. It shows the biggest entries at the top, the biggest
//! files anywhere, what was counted once, and what could not be read. Sizes are allocated bytes, as
//! the disk map shows them, in decimal units like a file manager.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use panoply_core::assets::format_size;
use panoply_disk::{Kind, NodeId, ScanOptions, ScanReport, SkipReason, Tree};
use serde_json::{Value, json};

/// Options of `panoply disk`.
#[derive(Args)]
pub struct DiskArgs {
    /// Folder (or whole drive) to look at
    pub path: PathBuf,
    /// How many of the biggest entries to list in each folder
    #[arg(long, default_value_t = 15, value_name = "N")]
    pub top: usize,
    /// How many of the biggest files anywhere to list
    #[arg(long, default_value_t = 10, value_name = "N")]
    pub files: usize,
    /// How many folder levels to list (1 is the top level only)
    #[arg(long, default_value_t = 1, value_name = "N")]
    pub depth: usize,
    /// Go into other drives mounted below the folder (off by default)
    #[arg(long)]
    pub cross: bool,
    /// Print the result as JSON
    #[arg(long)]
    pub json: bool,
}

/// Run the command.
pub fn run(args: DiskArgs) -> ExitCode {
    let opts = ScanOptions {
        cross_volumes: args.cross,
        ..ScanOptions::default()
    };
    let scan = match panoply_disk::scan(&args.path, &opts) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Could not scan {}: {e}", args.path.display());
            return ExitCode::FAILURE;
        }
    };
    let depth = args.depth.max(1);
    if args.json {
        let v = to_json(&scan.tree, &scan.report, &args, depth);
        println!(
            "{}",
            serde_json::to_string_pretty(&v).unwrap_or_else(|_| "{}".into())
        );
    } else {
        print!("{}", render(&scan.tree, &scan.report, &args, depth));
    }
    ExitCode::SUCCESS
}

/// Bytes the way the rest of the CLI shows them, with GB and TB added for disks.
fn size(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes >= 1_000_000_000_000 {
        format!("{:.1} TB", b / 1e12)
    } else if bytes >= 1_000_000_000 {
        format!("{:.1} GB", b / 1e9)
    } else {
        format_size(bytes)
    }
}

fn seconds(report: &ScanReport) -> String {
    let s = report.elapsed.as_secs_f64();
    if s < 1.0 {
        format!("{} ms", report.elapsed.as_millis())
    } else {
        format!("{s:.1} s")
    }
}

/// 1234567 as "1,234,567".
fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{} {}", count(n), if n == 1 { one } else { many })
}

/// The name an entry is listed under. A folded run of small files has no name of its own.
fn label(tree: &Tree, id: NodeId) -> String {
    match tree.kind(id) {
        Kind::Small => plural(u64::from(tree.items(id)), "small file", "small files"),
        Kind::Dir => format!("{}/", tree.name(id)),
        _ => tree.name(id),
    }
}

fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Dir => "folder",
        Kind::File => "file",
        Kind::Symlink => "link",
        Kind::Other => "other",
        Kind::Small => "small files",
    }
}

fn reason_words(r: SkipReason) -> &'static str {
    match r {
        SkipReason::Permission => "no permission",
        SkipReason::FullDiskAccess => "macOS wants Full Disk Access",
        SkipReason::Gone => "vanished during the scan",
        SkipReason::Other => "could not be read",
    }
}

const REASONS: [SkipReason; 4] = [
    SkipReason::FullDiskAccess,
    SkipReason::Permission,
    SkipReason::Other,
    SkipReason::Gone,
];

/// The one line about what was counted once, or `None` when nothing needed it.
fn counted_once_line(r: &ScanReport) -> Option<String> {
    let mut parts = Vec::new();
    if r.hard_link_entries > 0 {
        parts.push(format!(
            "{} ({}) as hard links",
            plural(r.hard_link_entries, "extra name", "extra names"),
            size(r.hard_link_bytes_saved)
        ));
    }
    if r.clone_bytes_saved > 0 {
        parts.push(format!("{} of cloned file data", size(r.clone_bytes_saved)));
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("Counted once, not twice: {}.", parts.join(" and ")))
    }
}

fn render(tree: &Tree, report: &ScanReport, args: &DiskArgs, depth: usize) -> String {
    let mut out = String::new();
    let root = tree.root();
    out.push_str(&format!("{}\n", tree.path(root).display()));
    out.push_str(&format!(
        "{} on disk, {}, {}, scanned in {}\nBackend: {}\n",
        size(report.total_bytes),
        plural(report.files, "file", "files"),
        plural(report.dirs, "folder", "folders"),
        seconds(report),
        report.backend
    ));
    if let Some(line) = counted_once_line(report) {
        out.push_str(&format!("{line}\n"));
    }
    if report.cancelled || report.truncated {
        out.push_str("This list is partial: the scan stopped early.\n");
    }

    if args.top > 0 {
        out.push_str(&format!(
            "\nLargest {}\n",
            if depth > 1 {
                "entries, by folder"
            } else {
                "entries at the top level"
            }
        ));
        let before = out.len();
        list_level(tree, root, args.top, depth, 0, report.total_bytes, &mut out);
        if out.len() == before {
            out.push_str("  (empty)\n");
        }
    }

    if args.files > 0 {
        out.push_str("\nLargest files anywhere\n");
        let files = tree.largest(args.files, Kind::File);
        if files.is_empty() {
            out.push_str("  (no files)\n");
        }
        for id in files {
            out.push_str(&format!(
                "{:>9}  {}\n",
                size(tree.size(id)),
                tree.path(id).display()
            ));
        }
    }

    if report.other_volumes > 0 {
        out.push_str(&format!(
            "\n{} not entered (other drives). Use --cross to include them.\n",
            plural(report.other_volumes, "mount point was", "mount points were")
        ));
    }
    if !report.skipped.is_empty() {
        out.push_str(&unreadable_note(tree, report));
    }
    out
}

fn list_level(
    tree: &Tree,
    dir: NodeId,
    top: usize,
    depth: usize,
    level: usize,
    total: u64,
    out: &mut String,
) {
    for id in tree.children_by_size(dir).into_iter().take(top) {
        let pct = if total == 0 {
            0.0
        } else {
            tree.size(id) as f64 * 100.0 / total as f64
        };
        out.push_str(&format!(
            "{:>9}  {:>4.0}%  {}{}\n",
            size(tree.size(id)),
            pct,
            "  ".repeat(level),
            label(tree, id)
        ));
        if tree.kind(id) == Kind::Dir && level + 1 < depth {
            list_level(tree, id, top, depth, level + 1, total, out);
        }
    }
}

fn unreadable_note(tree: &Tree, report: &ScanReport) -> String {
    let mut out = format!(
        "\n{} could not be read, so their sizes are missing from the total.\n",
        plural(report.skipped.len() as u64, "folder", "folders")
    );
    for r in REASONS {
        let n = report.skipped_count(r) as u64;
        if n > 0 {
            out.push_str(&format!("  {}: {}\n", reason_words(r), count(n)));
        }
    }
    for s in report.skipped.iter().take(5) {
        out.push_str(&format!("  {}\n", tree.path(s.node).display()));
    }
    if report.skipped.len() > 5 {
        out.push_str(&format!("  and {} more\n", report.skipped.len() - 5));
    }
    if report.needs_full_disk_access() {
        out.push_str(
            "On macOS, Full Disk Access would let Panoply read them: System Settings, Privacy and \
             Security, Full Disk Access, switch on your terminal (or Panoply), then run this again.\n",
        );
    }
    out
}

fn entry_json(tree: &Tree, id: NodeId, depth: usize, top: usize, level: usize) -> Value {
    let mut v = json!({
        "name": match tree.kind(id) {
            Kind::Small => label(tree, id),
            _ => tree.name(id),
        },
        "path": tree.path(id).display().to_string(),
        "kind": kind_word(tree.kind(id)),
        "bytes": tree.size(id),
        "items": tree.items(id),
    });
    if tree.kind(id) == Kind::Dir && level + 1 < depth {
        v["children"] = children_json(tree, id, depth, top, level + 1);
    }
    v
}

fn children_json(tree: &Tree, dir: NodeId, depth: usize, top: usize, level: usize) -> Value {
    Value::Array(
        tree.children_by_size(dir)
            .into_iter()
            .take(top)
            .map(|id| entry_json(tree, id, depth, top, level))
            .collect(),
    )
}

fn to_json(tree: &Tree, report: &ScanReport, args: &DiskArgs, depth: usize) -> Value {
    let root = tree.root();
    let mut by_reason = serde_json::Map::new();
    for r in REASONS {
        let n = report.skipped_count(r);
        if n > 0 {
            by_reason.insert(reason_words(r).to_string(), json!(n));
        }
    }
    let files: Vec<Value> = tree
        .largest(args.files, Kind::File)
        .into_iter()
        .map(|id| json!({ "path": tree.path(id).display().to_string(), "bytes": tree.size(id) }))
        .collect();
    json!({
        "path": Path::new(&tree.path(root)).display().to_string(),
        "total_bytes": report.total_bytes,
        "files": report.files,
        "folders": report.dirs,
        "scan_ms": report.elapsed.as_millis() as u64,
        "backend": report.backend,
        "partial": report.cancelled || report.truncated,
        "counted_once": {
            "hard_link_entries": report.hard_link_entries,
            "hard_link_bytes_saved": report.hard_link_bytes_saved,
            "clone_bytes_saved": report.clone_bytes_saved,
        },
        "other_drives_not_entered": report.other_volumes,
        "unreadable": {
            "folders": report.skipped.len(),
            "by_reason": by_reason,
            "needs_full_disk_access": report.needs_full_disk_access(),
            "first": report.skipped.iter().take(20)
                .map(|s| tree.path(s.node).display().to_string())
                .collect::<Vec<_>>(),
        },
        "top": children_json(tree, root, depth, args.top, 0),
        "largest_files": files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_decimal_units_up_to_terabytes() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(233_000), "233 KB");
        assert_eq!(size(4_540_000), "4.5 MB");
        assert_eq!(size(4_200_000_000), "4.2 GB");
        assert_eq!(size(2_500_000_000_000), "2.5 TB");
    }

    #[test]
    fn counts_get_thousands_separators() {
        assert_eq!(count(7), "7");
        assert_eq!(count(1234), "1,234");
        assert_eq!(count(1_234_567), "1,234,567");
        assert_eq!(plural(1, "file", "files"), "1 file");
    }
}
