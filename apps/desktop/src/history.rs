//! The job history: one JSON line per finished job, newest last. Read back newest first.
//!
//! The file is only ever appended to, so a crash cannot corrupt older entries, and reading looks
//! at the last 256 KB only, so a long history never costs startup time or memory.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const TAIL_BYTES: u64 = 256 * 1024;

/// One finished job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Seconds since the epoch.
    pub at: u64,
    /// Module id.
    pub tool_id: String,
    /// Module name at the time.
    pub tool_name: String,
    /// Input files.
    pub inputs: Vec<PathBuf>,
    /// Options used.
    #[serde(default)]
    pub options: Value,
    /// Files written.
    #[serde(default)]
    pub outputs: Vec<PathBuf>,
    /// The result card's headline.
    #[serde(default)]
    pub headline: String,
}

/// The history file.
#[derive(Debug, Clone)]
pub struct History {
    path: PathBuf,
}

impl History {
    /// The history in the user's data folder.
    pub fn open_default() -> Self {
        Self::at(crate::paths::history_file())
    }

    /// A history in this file.
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// Add an entry.
    pub fn append(&self, entry: &Entry) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
        line.push('\n');
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        f.write_all(line.as_bytes())
    }

    /// The most recent `n` entries, newest first. Lines that do not parse are skipped.
    pub fn recent(&self, n: usize) -> Vec<Entry> {
        let Ok(mut f) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        let start = len.saturating_sub(TAIL_BYTES);
        if f.seek(SeekFrom::Start(start)).is_err() {
            return Vec::new();
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(&buf);
        let mut lines = text.lines();
        if start > 0 {
            // The first line of a tail read is probably cut in half.
            lines.next();
        }
        let mut entries: Vec<Entry> = lines
            .filter_map(|l| serde_json::from_str::<Entry>(l).ok())
            .collect();
        entries.reverse();
        entries.truncate(n);
        entries
    }

    /// Module ids from the most recent jobs, newest first, without repeats.
    pub fn recent_tool_ids(&self, n: usize) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for e in self.recent(64) {
            if !seen.contains(&e.tool_id) {
                seen.push(e.tool_id);
            }
            if seen.len() >= n {
                break;
            }
        }
        seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("panoply-hist-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("history.jsonl")
    }

    fn entry(n: u64) -> Entry {
        Entry {
            at: n,
            tool_id: format!("t.{}", n % 2),
            tool_name: "T".into(),
            inputs: vec![PathBuf::from("/a.pdf")],
            options: Value::Null,
            outputs: vec![],
            headline: format!("job {n}"),
        }
    }

    #[test]
    fn appends_and_reads_newest_first() {
        let h = History::at(temp("basic"));
        for n in 1..=5 {
            h.append(&entry(n)).unwrap();
        }
        let r = h.recent(3);
        assert_eq!(r.iter().map(|e| e.at).collect::<Vec<_>>(), vec![5, 4, 3]);
        assert_eq!(h.recent_tool_ids(5), vec!["t.1", "t.0"]);
    }

    #[test]
    fn missing_file_is_empty_and_bad_lines_are_skipped() {
        let p = temp("bad");
        let h = History::at(p.clone());
        assert!(h.recent(5).is_empty());
        h.append(&entry(1)).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(b"not json\n")
            .unwrap();
        h.append(&entry(2)).unwrap();
        assert_eq!(h.recent(9).len(), 2);
    }

    #[test]
    fn long_history_reads_only_the_tail() {
        let h = History::at(temp("tail"));
        for n in 1..=3000 {
            h.append(&entry(n)).unwrap();
        }
        let r = h.recent(3);
        assert_eq!(r[0].at, 3000);
        assert_eq!(r.len(), 3);
    }
}
