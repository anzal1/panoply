//! The file index service: the one place the app owns a [`LiveIndex`].
//!
//! * First launch: a background thread walks the roots (the home folder unless `PANOPLY_FIND_ROOTS`
//!   says otherwise) with a few worker threads, saves the index under the data folder and starts the
//!   watcher. The UI sees `State::Indexing` with the number of entries read so far.
//! * Later launches: the saved index is read (one file read, milliseconds), the watcher is started
//!   from the saved event id so what changed while the app was closed is replayed.
//! * Idle: nothing runs. The watcher thread sleeps in `recv`; there is no timer here.
//! * Quit: [`Finder::shutdown`] stops the watcher, saves the index and only then moves the replay
//!   point forward, so a crash replays more events, never fewer.
//!
//! Nothing here touches the UI thread, and nothing scans on its own: tests and the snapshot renderer
//! build a [`Finder`] from a tiny index with [`Finder::with_index`].

use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use panoply_find::watch::{self, Updater};
use panoply_find::{
    Exclusions, Index, LiveIndex, SearchOptions, Segment, WalkOptions, walk_with_progress,
};

use crate::catalog::Category;
use crate::search::{Action, Row, Section};
use crate::units::relative_time;

/// What the index is doing, for the screens.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// The first scan is running; this many entries have been read so far.
    Indexing {
        /// Entries read so far.
        entries: u64,
    },
    /// Searching works.
    Ready {
        /// Entries in the index.
        entries: usize,
        /// True while a watcher keeps the index current.
        fresh: bool,
    },
    /// There is no index and there will not be one (the folder cannot be read, or the service is off).
    Unavailable {
        /// One plain sentence.
        reason: String,
    },
}

/// Where the service keeps and looks for things.
#[derive(Clone, Debug)]
pub struct Config {
    /// Folders to index.
    pub roots: Vec<PathBuf>,
    /// The folder for the saved index and the event ids.
    pub dir: PathBuf,
    /// Start the file system watcher.
    pub watch: bool,
    /// The home folder, shown as `~` in paths.
    pub home: PathBuf,
}

impl Config {
    /// The app's setting: the home folder (or `PANOPLY_FIND_ROOTS`, separated like `PATH`), and the
    /// index under the data folder.
    pub fn app() -> Config {
        let home = home_dir();
        let roots = match std::env::var_os("PANOPLY_FIND_ROOTS") {
            Some(v) => std::env::split_paths(&v)
                .filter(|p| !p.as_os_str().is_empty())
                .collect(),
            None => vec![home.clone()],
        };
        Config {
            roots,
            dir: crate::paths::find_dir(),
            watch: true,
            home,
        }
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

struct Files {
    index: PathBuf,
    /// Written by the watcher: the newest event it applied.
    live_events: PathBuf,
    /// Written by us after a save: where the next launch replays from.
    replay: PathBuf,
}

impl Files {
    fn in_dir(dir: &Path) -> Files {
        Files {
            index: dir.join("index.bin"),
            live_events: dir.join("events.live"),
            replay: dir.join("events"),
        }
    }
}

struct Inner {
    state: Mutex<State>,
    live: RwLock<Option<Arc<RwLock<LiveIndex>>>>,
    updater: Mutex<Option<Updater>>,
    files: Option<Files>,
    home: PathBuf,
    notify: Box<dyn Fn() + Send + Sync>,
    /// Set by `shutdown`, so a scan still running does not start a watcher afterwards.
    stopped: AtomicBool,
}

/// A handle to the service. Cheap to clone; every clone is the same service.
#[derive(Clone)]
pub struct Finder {
    inner: Arc<Inner>,
}

impl Finder {
    fn new(
        state: State,
        live: Option<Arc<RwLock<LiveIndex>>>,
        files: Option<Files>,
        home: PathBuf,
        notify: Box<dyn Fn() + Send + Sync>,
    ) -> Finder {
        Finder {
            inner: Arc::new(Inner {
                state: Mutex::new(state),
                live: RwLock::new(live),
                updater: Mutex::new(None),
                files,
                home,
                notify,
                stopped: AtomicBool::new(false),
            }),
        }
    }

    /// Start the service on a background thread and return at once. `notify` is called (from that
    /// thread, at most a few times a second) when the state changes.
    pub fn start(cfg: Config, notify: impl Fn() + Send + Sync + 'static) -> Finder {
        let f = Finder::new(
            State::Indexing { entries: 0 },
            None,
            Some(Files::in_dir(&cfg.dir)),
            cfg.home.clone(),
            Box::new(notify),
        );
        let inner = f.inner.clone();
        let spawned = std::thread::Builder::new()
            .name("panoply-find-boot".into())
            .spawn(move || boot(&inner, &cfg));
        if spawned.is_err() {
            f.set_state(State::Unavailable {
                reason: "The file index could not start.".into(),
            });
        }
        f
    }

    /// A service over a ready-made index, with no watcher and no files on disk. Tests and the
    /// snapshot renderer use this, so nothing ever scans a real folder.
    pub fn with_index(live: LiveIndex, home: PathBuf) -> Finder {
        let entries = live.index().len();
        Finder::new(
            State::Ready {
                entries,
                fresh: false,
            },
            Some(Arc::new(RwLock::new(live))),
            None,
            home,
            Box::new(|| {}),
        )
    }

    /// A service that is in the given state and never changes it (for the screens that show
    /// "indexing" and "unavailable").
    pub fn fixed(state: State, home: PathBuf) -> Finder {
        Finder::new(state, None, None, home, Box::new(|| {}))
    }

    /// The service when it is switched off (`PANOPLY_FIND=off`).
    pub fn off() -> Finder {
        Finder::fixed(
            State::Unavailable {
                reason: "File search is switched off.".into(),
            },
            home_dir(),
        )
    }

    /// The current state.
    pub fn state(&self) -> State {
        self.inner
            .state
            .lock()
            .map(|s| s.clone())
            .unwrap_or(State::Unavailable {
                reason: "The file index stopped.".into(),
            })
    }

    fn set_state(&self, s: State) {
        set_state(&self.inner, s);
    }

    /// The home folder, as shown in paths.
    pub fn home(&self) -> &Path {
        &self.inner.home
    }

    /// Search by name. Blocking and quick (about a millisecond), but it takes the index's read
    /// lock, so callers on the UI thread hand it to a worker. Empty while there is no index.
    pub fn search(&self, query: &str, limit: usize) -> Vec<FileHit> {
        let q = query.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let Some(live) = self.inner.live.read().ok().and_then(|l| l.clone()) else {
            return Vec::new();
        };
        let Ok(live) = live.read() else {
            return Vec::new();
        };
        let idx = live.index();
        let opts = SearchOptions {
            limit,
            ..SearchOptions::default()
        };
        idx.search(q, &opts)
            .into_iter()
            .filter_map(|h| idx.entry(h.id))
            .map(|e| FileHit::new(e.path, e.name, e.is_dir, e.mtime, q, &self.inner.home))
            .collect()
    }

    /// Stop the watcher and save the index. Blocks for as long as the save takes (tens of
    /// milliseconds); called once, when the app quits.
    pub fn shutdown(&self) {
        self.inner.stopped.store(true, Ordering::SeqCst);
        // Dropping the updater stops the watcher and waits for the worker, which writes the id of
        // the newest event it applied on its way out.
        if let Ok(mut u) = self.inner.updater.lock() {
            u.take();
        }
        let Some(files) = &self.inner.files else {
            return;
        };
        let live = self.inner.live.read().ok().and_then(|l| l.clone());
        let Some(live) = live else { return };
        let id = watch::read_event_id(&files.live_events);
        if let Ok(mut l) = live.write()
            && l.save(&files.index).is_ok()
            && let Some(id) = id
        {
            let _ = fs::write(&files.replay, format!("{id}\n"));
        }
    }
}

fn set_state(inner: &Inner, s: State) {
    if let Ok(mut st) = inner.state.lock() {
        *st = s;
    }
    (inner.notify)();
}

fn boot(inner: &Arc<Inner>, cfg: &Config) {
    let Some(files) = &inner.files else { return };
    if cfg.roots.is_empty() {
        set_state(
            inner,
            State::Unavailable {
                reason: "There is no folder to index.".into(),
            },
        );
        return;
    }
    let _ = fs::create_dir_all(&cfg.dir);
    if let Some(live) = load_saved(files, &cfg.roots) {
        let since = watch::read_event_id(&files.replay);
        adopt(inner, live, since, cfg.watch);
        return;
    }
    // Drop a stale replay point: it belongs to an index we no longer have.
    let _ = fs::remove_file(&files.replay);
    let _ = fs::remove_file(&files.live_events);
    // Taken before the scan, so what changes while it runs is replayed.
    let marker = watch::marker();
    let progress = AtomicU64::new(0);
    let done = AtomicBool::new(false);
    let opts = WalkOptions {
        // A few workers: the first scan is a background chore, not a benchmark.
        threads: std::thread::available_parallelism().map_or(2, |n| n.get().min(4)),
        ..WalkOptions::default()
    };
    set_state(inner, State::Indexing { entries: 0 });
    let walked = std::thread::scope(|s| {
        s.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
                if done.load(Ordering::Relaxed) {
                    break;
                }
                set_state(
                    inner,
                    State::Indexing {
                        entries: progress.load(Ordering::Relaxed),
                    },
                );
            }
        });
        let r = walk_with_progress(&cfg.roots, &opts, Some(&progress));
        done.store(true, Ordering::Relaxed);
        r
    });
    match walked {
        Ok((seg, _stats)) => {
            let mut live = LiveIndex::new(Index::from_segment(seg), Exclusions::defaults());
            if live.save(&files.index).is_ok()
                && let Some(m) = marker
            {
                let _ = fs::write(&files.replay, format!("{m}\n"));
            }
            adopt(inner, live, marker, cfg.watch);
        }
        Err(e) => set_state(
            inner,
            State::Unavailable {
                reason: format!("Your files could not be read: {e}."),
            },
        ),
    }
}

/// Read the saved index. `None` when there is none, it is damaged, or it was made for other roots.
fn load_saved(files: &Files, roots: &[PathBuf]) -> Option<LiveIndex> {
    let f = fs::File::open(&files.index).ok()?;
    let seg = Segment::read_from(BufReader::with_capacity(1 << 20, f)).ok()?;
    let live = LiveIndex::new(Index::from_segment(seg), Exclusions::defaults());
    let mut have: Vec<&Path> = live.roots().collect();
    let mut want: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
    have.sort();
    want.sort();
    (have == want).then_some(live)
}

fn adopt(inner: &Arc<Inner>, live: LiveIndex, since: Option<u64>, watch_on: bool) {
    let entries = live.index().len();
    let live = Arc::new(RwLock::new(live));
    if let Ok(mut l) = inner.live.write() {
        *l = Some(live.clone());
    }
    let mut fresh = false;
    if watch_on && !inner.stopped.load(Ordering::SeqCst) {
        let state_file = inner.files.as_ref().map(|f| f.live_events.clone());
        if let Ok(u) = watch::start(live, since, state_file) {
            if let Ok(mut slot) = inner.updater.lock() {
                *slot = Some(u);
            }
            fresh = true;
        }
    }
    set_state(inner, State::Ready { entries, fresh });
}

// ---- hits and rows ----

/// One file found by name, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct FileHit {
    /// The full path.
    pub path: PathBuf,
    /// The file name.
    pub name: String,
    /// The folder it is in, with the home folder shown as `~`.
    pub parent: String,
    /// True for a folder.
    pub is_dir: bool,
    /// Modified time, seconds since the Unix epoch (0 when unknown).
    pub mtime: u32,
    /// Indices (in characters) of the name that the query matched.
    pub matched: Vec<usize>,
}

impl FileHit {
    /// Build a hit from an index entry and the query that found it.
    pub fn new(
        path: PathBuf,
        name: String,
        is_dir: bool,
        mtime: u32,
        query: &str,
        home: &Path,
    ) -> FileHit {
        let parent = path
            .parent()
            .map(|p| home_relative(p, home))
            .unwrap_or_default();
        let matched = highlight(&name, query);
        FileHit {
            path,
            name,
            parent,
            is_dir,
            mtime,
            matched,
        }
    }

    /// What kind of thing this is, for the icon and the tint.
    pub fn kind(&self) -> Kind {
        kind_of(&self.name, self.is_dir)
    }

    /// The row the command bar draws for this file.
    pub fn row(&self, now: u64) -> Row {
        let kind = self.kind();
        Row {
            action: Action::OpenFile {
                path: self.path.clone(),
            },
            title: self.name.clone(),
            subtitle: self.parent.clone(),
            trailing: modified_label(now, self.mtime),
            icon: kind.icon,
            category: kind.category,
            matched: self.matched.clone(),
            chips: Vec::new(),
            estimate: None,
        }
    }
}

/// Put the files under the tools: a "Files" section after every section there already is. Does
/// nothing when there are no hits.
pub fn append_files(results: &mut crate::search::Results, hits: &[FileHit], now: u64) {
    results.sections.retain(|s| s.title != FILES_TITLE);
    if hits.is_empty() {
        return;
    }
    results.sections.push(Section {
        title: FILES_TITLE.into(),
        rows: hits.iter().map(|h| h.row(now)).collect(),
    });
}

/// The command bar's section label for files.
pub const FILES_TITLE: &str = "Files";

/// How many files the command bar shows under the tools.
pub const BAR_FILES: usize = 4;

/// The path of the folder, with the home folder written `~`.
pub fn home_relative(dir: &Path, home: &Path) -> String {
    if !home.as_os_str().is_empty()
        && let Ok(rest) = dir.strip_prefix(home)
    {
        return if rest.as_os_str().is_empty() {
            "~".to_string()
        } else {
            format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display())
        };
    }
    dir.display().to_string()
}

/// What a file looks like in a list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kind {
    /// Icon name.
    pub icon: &'static str,
    /// The tint.
    pub category: Category,
    /// "PDF document", "Folder".
    pub label: &'static str,
}

/// The icon, tint and name of a kind of file, from its name alone (the index holds nothing else).
pub fn kind_of(name: &str, is_dir: bool) -> Kind {
    let k = |icon, category, label| Kind {
        icon,
        category,
        label,
    };
    if is_dir {
        return k("folder", Category::Files, "Folder");
    }
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => k("file-text", Category::Pdf, "PDF document"),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "heif" | "tif" | "tiff" | "bmp"
        | "svg" | "avif" | "raw" | "dng" => k("file-image", Category::Images, "Image"),
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" => k("file-video", Category::Video, "Video"),
        "mp3" | "wav" | "flac" | "m4a" | "aac" | "ogg" | "opus" | "aiff" => {
            k("file-audio", Category::Audio, "Audio")
        }
        "doc" | "docx" | "odt" | "rtf" | "xls" | "xlsx" | "ods" | "ppt" | "pptx" | "odp"
        | "pages" | "numbers" | "key" | "epub" => k("file-text", Category::Documents, "Document"),
        "txt" | "md" | "csv" | "log" | "json" | "yaml" | "yml" | "toml" => {
            k("file-text", Category::Text, "Text file")
        }
        "zip" | "tar" | "gz" | "tgz" | "7z" | "rar" | "bz2" | "xz" | "dmg" | "iso" => {
            k("file-archive", Category::Files, "Archive")
        }
        "rs" | "js" | "ts" | "tsx" | "jsx" | "py" | "go" | "c" | "h" | "cpp" | "java" | "swift"
        | "rb" | "sh" | "html" | "css" => k("file-code", Category::Developer, "Source file"),
        _ => k("file", Category::Files, "File"),
    }
}

/// Character indices of `name` that the query matched: each space separated word is looked up as a
/// case blind substring first, then as a loose subsequence (the same two ways the engine matches).
/// Used only to draw the match in bold; the ranking is the engine's.
pub fn highlight(name: &str, query: &str) -> Vec<usize> {
    let chars: Vec<char> = name.chars().map(fold_char).collect();
    let mut hit = vec![false; chars.len()];
    for word in query.split_whitespace() {
        // A path word ("src/lib") matches the name by its last part.
        let word = word
            .rsplit(['/', '\\'])
            .find(|s| !s.is_empty())
            .unwrap_or("");
        let w: Vec<char> = word.chars().map(fold_char).collect();
        if w.is_empty() || w.len() > chars.len() {
            continue;
        }
        if let Some(at) = chars.windows(w.len()).position(|win| win == w.as_slice()) {
            for h in &mut hit[at..at + w.len()] {
                *h = true;
            }
            continue;
        }
        // Subsequence, only when every letter of the word is found in order.
        let mut at = 0;
        let mut found = Vec::with_capacity(w.len());
        for &c in &w {
            match chars[at..].iter().position(|&x| x == c) {
                Some(p) => {
                    found.push(at + p);
                    at += p + 1;
                }
                None => {
                    found.clear();
                    break;
                }
            }
        }
        for i in found {
            hit[i] = true;
        }
    }
    hit.iter()
        .enumerate()
        .filter_map(|(i, &h)| h.then_some(i))
        .collect()
}

fn fold_char(c: char) -> char {
    // Lower case, and the common Latin accents flattened, as the engine's folding does.
    let l = c.to_lowercase().next().unwrap_or(c);
    match l {
        'à'..='å' => 'a',
        'ç' => 'c',
        'è'..='ë' => 'e',
        'ì'..='ï' => 'i',
        'ñ' => 'n',
        'ò'..='ö' => 'o',
        'ù'..='ü' => 'u',
        'ý' | 'ÿ' => 'y',
        other => other,
    }
}

/// "5 min ago", "Yesterday", "12 Mar", "3 Nov 2023": when a file changed, for a list. Dates are
/// in UTC (the app has no time zone database).
pub fn modified_label(now: u64, mtime: u32) -> String {
    let then = u64::from(mtime);
    if then == 0 {
        return String::new();
    }
    let age = now.saturating_sub(then);
    if age < 7 * 86_400 {
        return relative_time(now, then);
    }
    let (y, m, d) = civil_from_days((then / 86_400) as i64);
    let (ny, _, _) = civil_from_days((now / 86_400) as i64);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = MONTHS[(m as usize).clamp(1, 12) - 1];
    if y == ny {
        format!("{d} {month}")
    } else {
        format!("{d} {month} {y}")
    }
}

/// Days since 1970-01-01 to (year, month, day).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn tree() -> tempdir::Tmp {
        let t = tempdir::Tmp::new("finder");
        for p in [
            "Documents/Taxes/Quarterly report 2025.pdf",
            "Documents/Taxes/notes.txt",
            "Pictures/holiday/IMG_0001.heic",
            "Pictures/holiday/IMG_0002.heic",
            "projects/panoply/README.md",
            "projects/panoply/src/main.rs",
        ] {
            let f = t.path().join(p);
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, b"x").unwrap();
        }
        t
    }

    /// A tiny temp folder with no dependencies of its own.
    mod tempdir {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        pub struct Tmp(PathBuf);
        impl Tmp {
            pub fn new(tag: &str) -> Tmp {
                let p = std::env::temp_dir().join(format!(
                    "panoply-{tag}-{}-{}",
                    std::process::id(),
                    N.fetch_add(1, Ordering::Relaxed)
                ));
                let _ = std::fs::remove_dir_all(&p);
                std::fs::create_dir_all(&p).unwrap();
                // Resolve symlinks (/var is /private/var on a Mac) so roots compare equal.
                Tmp(std::fs::canonicalize(&p).unwrap())
            }
            pub fn path(&self) -> &Path {
                &self.0
            }
        }
        impl Drop for Tmp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    fn small_index(root: &Path) -> LiveIndex {
        let o = WalkOptions {
            threads: 2,
            ..WalkOptions::default()
        };
        let (idx, _) = Index::build(&[root.to_path_buf()], &o).unwrap();
        LiveIndex::new(idx, Exclusions::none())
    }

    #[test]
    fn a_search_returns_home_relative_hits_with_the_match_marked() {
        let t = tree();
        let f = Finder::with_index(small_index(t.path()), t.path().to_path_buf());
        let hits = f.search("quarterly", 10);
        assert_eq!(hits.len(), 1, "{hits:?}");
        let h = &hits[0];
        assert_eq!(h.name, "Quarterly report 2025.pdf");
        assert_eq!(
            h.parent,
            format!("~{0}Documents{0}Taxes", std::path::MAIN_SEPARATOR)
        );
        assert_eq!(h.matched, (0..9).collect::<Vec<_>>());
        assert_eq!(h.kind().label, "PDF document");
        assert!(f.search("   ", 10).is_empty());
        assert!(f.search("zzzqqq", 10).is_empty());
    }

    #[test]
    fn rows_for_the_bar_carry_the_open_action_and_the_match() {
        let t = tree();
        let f = Finder::with_index(small_index(t.path()), t.path().to_path_buf());
        let hits = f.search("img_000", 10);
        assert_eq!(hits.len(), 2);
        let row = hits[0].row(crate::units::now_secs());
        assert!(matches!(row.action, Action::OpenFile { .. }));
        assert_eq!(row.icon, "file-image");
        assert!(!row.matched.is_empty());
        let mut r = crate::search::Results::default();
        append_files(&mut r, &hits, crate::units::now_secs());
        assert_eq!(r.sections.last().unwrap().title, FILES_TITLE);
        append_files(&mut r, &[], 0);
        assert!(r.sections.is_empty(), "an empty answer removes the section");
    }

    #[test]
    fn highlighting_handles_words_subsequences_accents_and_paths() {
        assert_eq!(highlight("Report.pdf", "rep"), vec![0, 1, 2]);
        assert_eq!(highlight("Report.pdf", "rpt"), vec![0, 3, 5]);
        assert_eq!(highlight("Café menu.txt", "cafe"), vec![0, 1, 2, 3]);
        assert_eq!(highlight("lib.rs", "src/lib"), vec![0, 1, 2]);
        assert_eq!(highlight("a b", "zzz"), Vec::<usize>::new());
        assert_eq!(highlight("Quarterly report", "report quarterly").len(), 15);
    }

    #[test]
    fn home_paths_are_written_with_a_tilde() {
        let home = Path::new("/Users/me");
        assert_eq!(home_relative(Path::new("/Users/me"), home), "~");
        assert_eq!(
            home_relative(Path::new("/Users/me/a/b"), home),
            format!("~{0}a{0}b", std::path::MAIN_SEPARATOR)
        );
        assert_eq!(home_relative(Path::new("/Volumes/x"), home), "/Volumes/x");
        assert_eq!(
            home_relative(Path::new("/Users/mellon"), home),
            "/Users/mellon"
        );
    }

    #[test]
    fn kinds_come_from_the_extension() {
        assert_eq!(kind_of("a.HEIC", false).category, Category::Images);
        assert_eq!(kind_of("Makefile", false).label, "File");
        assert_eq!(kind_of("src", true).icon, "folder");
        assert_eq!(kind_of("x.rs", false).icon, "file-code");
    }

    #[test]
    fn modified_times_read_like_a_file_manager() {
        let now = 1_760_000_000u64; // 9 Oct 2025
        assert_eq!(modified_label(now, 0), "");
        assert_eq!(modified_label(now, (now - 5 * 60) as u32), "5 min ago");
        assert_eq!(modified_label(now, (now - 86_400 - 5) as u32), "Yesterday");
        assert_eq!(modified_label(now, 1_757_000_000), "4 Sep");
        assert_eq!(modified_label(now, 1_700_000_000), "14 Nov 2023");
    }

    #[test]
    fn first_launch_builds_and_saves_then_the_next_launch_loads_it_fast() {
        let t = tree();
        let dir = tempdir::Tmp::new("finder-data");
        let cfg = Config {
            roots: vec![t.path().to_path_buf()],
            dir: dir.path().to_path_buf(),
            watch: false,
            home: t.path().to_path_buf(),
        };
        let wait = |f: &Finder| {
            let t0 = Instant::now();
            loop {
                if let State::Ready { entries, .. } = f.state() {
                    return entries;
                }
                assert!(t0.elapsed() < Duration::from_secs(20), "{:?}", f.state());
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let first = Finder::start(cfg.clone(), || {});
        let n = wait(&first);
        assert!(n >= 12, "{n}");
        first.shutdown();
        assert!(dir.path().join("index.bin").exists());

        let t0 = Instant::now();
        let second = Finder::start(cfg.clone(), || {});
        let m = wait(&second);
        let ms = t0.elapsed().as_millis();
        assert_eq!(m, n);
        assert!(ms < 300, "a saved index loads in {ms} ms");
        assert_eq!(second.search("notes", 5).len(), 1);

        // An index made for other folders is not used.
        let other = Config {
            roots: vec![t.path().join("Pictures")],
            ..cfg
        };
        let third = Finder::start(other, || {});
        let k = wait(&third);
        assert!(k < n, "rebuilt for the new root: {k} < {n}");
    }

    #[test]
    fn an_unreadable_root_is_reported_not_hidden() {
        let dir = tempdir::Tmp::new("finder-data2");
        let cfg = Config {
            roots: Vec::new(),
            dir: dir.path().to_path_buf(),
            watch: false,
            home: PathBuf::new(),
        };
        let f = Finder::start(cfg, || {});
        let t0 = Instant::now();
        while matches!(f.state(), State::Indexing { .. }) {
            assert!(t0.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            matches!(f.state(), State::Unavailable { .. }),
            "{:?}",
            f.state()
        );
        assert!(f.search("a", 5).is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_new_file_is_found_without_a_rescan() {
        let t = tree();
        let dir = tempdir::Tmp::new("finder-data3");
        let cfg = Config {
            roots: vec![t.path().to_path_buf()],
            dir: dir.path().to_path_buf(),
            watch: true,
            home: t.path().to_path_buf(),
        };
        let f = Finder::start(cfg, || {});
        let t0 = Instant::now();
        while !matches!(f.state(), State::Ready { fresh: true, .. }) {
            assert!(t0.elapsed() < Duration::from_secs(20), "{:?}", f.state());
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::write(t.path().join("Documents/zebra-invoice.pdf"), b"x").unwrap();
        let t1 = Instant::now();
        while f.search("zebra-invoice", 3).is_empty() {
            assert!(
                t1.elapsed() < Duration::from_secs(8),
                "the watcher missed it"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        f.shutdown();
    }

    #[test]
    fn open_and_reveal_commands_hold_the_path_as_one_argument() {
        let p = Path::new("/tmp/a b & c.pdf");
        let o = crate::platform::open_command(p);
        assert!(
            o.get_args()
                .any(|a| a == p.as_os_str() || a.to_string_lossy().ends_with("a b & c.pdf"))
        );
        let r = crate::platform::reveal_command(p);
        assert!(r.get_args().count() >= 1);
    }
}
