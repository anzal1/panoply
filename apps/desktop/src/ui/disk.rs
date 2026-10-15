//! The disk map page: a path bar, the squarified treemap, and a list of the largest items.
//!
//! The geometry lives in `panoply_disk::layout` (pure, tested there). This file draws it: tiles are
//! tinted by file type with the app's category tints, folders sit on a neutral ground, a click on a
//! folder tile grows it to fill the map (280 ms, calm ease-out, instant under Reduce Motion), and
//! Backspace or the path bar goes back up. The scan runs on its own thread with a Cancel button.
//!
//! Hooks for the next unit: the `Review` button is disabled for now, and `fda_banner` is the slot that
//! the Full Disk Access explainer fills when a scan was refused folders.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::*;
use panoply_disk::{
    Drill, Kind, LayoutOptions, NodeId, Progress, Rect, Scan, ScanError, ScanOptions, Tile, Tree,
    squarify,
};

use super::chrome;
use super::theme::{Rgb, Theme, is_dark, radius, sp, tint, ty};
use super::widgets::{
    ButtonKind, Ramp, button, button_ex, caption, focus_border, icon, icon_button, notice, tip,
};
use crate::catalog::Category;
use crate::units::format_bytes;

/// Width of the right-hand list.
const SIDE_W: f32 = 272.0;
/// The app sidebar's width, to size the map from the window.
const APP_SIDEBAR: f32 = 232.0;
/// Height of the path bar and of the legend row.
const BAR_H: f32 = 28.0;
/// Height of the Full Disk Access banner, gap included.
const BANNER_H: f32 = 48.0;
/// How many rows the "in this folder" list shows.
const LIST_ROWS: usize = 9;
/// How many rows the "largest files" list shows.
const BIG_ROWS: usize = 6;

/// Tile fill strength over the window colour, light and dark. Chosen so the label colour passes 4.5:1
/// on every tint (see the test).
const FILL_ALPHA: (f32, f32) = (0.34, 0.46);

/// The colour group of a file, by its extension. `None` is "other", drawn neutral.
///
/// Archives borrow the capture teal and databases the calculator indigo: only the colours are reused.
pub(crate) fn category_of(name: &str) -> Option<Category> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    let c = match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "heif" | "tif" | "tiff" | "bmp"
        | "raw" | "dng" | "cr2" | "nef" | "arw" | "psd" | "svg" | "ico" | "avif" => {
            Category::Images
        }
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" | "wmv" | "flv" | "mpg" | "mpeg" => {
            Category::Video
        }
        "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" | "opus" | "aiff" | "wma" => Category::Audio,
        "pdf" => Category::Pdf,
        "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "pages" | "numbers" | "key" | "epub"
        | "odt" | "ods" | "odp" | "rtf" => Category::Documents,
        "txt" | "md" | "log" | "csv" | "tsv" | "rst" => Category::Text,
        "rs" | "js" | "ts" | "jsx" | "tsx" | "py" | "rb" | "go" | "java" | "kt" | "swift" | "c"
        | "h" | "cpp" | "hpp" | "cc" | "cs" | "php" | "sh" | "toml" | "yaml" | "yml" | "json"
        | "o" | "a" | "so" | "dylib" | "rlib" | "rmeta" | "class" | "jar" | "wasm" | "lock"
        | "map" | "d" | "pyc" | "node" | "dll" | "exe" => Category::Developer,
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "dmg" | "iso" | "pkg"
        | "deb" | "rpm" | "img" | "cab" => Category::Capture,
        "db" | "sqlite" | "sqlite3" | "dat" | "bin" | "sql" | "parquet" => Category::Calculators,
        _ => return None,
    };
    Some(c)
}

/// The colour group a folder is mostly made of: follow the largest entry down to a file.
fn dominant(tree: &Tree, mut node: NodeId) -> Option<Category> {
    for _ in 0..8 {
        let first = tree
            .children_by_size(node)
            .into_iter()
            .find(|n| tree.size(*n) > 0)?;
        match tree.kind(first) {
            Kind::Dir => node = first,
            Kind::File | Kind::Symlink => return category_of(&tree.name(first)),
            _ => return None,
        }
    }
    None
}

/// What the legend names, in order.
const LEGEND: [(&str, Category); 9] = [
    ("Images", Category::Images),
    ("Video", Category::Video),
    ("Audio", Category::Audio),
    ("PDF", Category::Pdf),
    ("Documents", Category::Documents),
    ("Text", Category::Text),
    ("Code", Category::Developer),
    ("Archives", Category::Capture),
    ("Data", Category::Calculators),
];

/// The fill of a tile as an opaque colour: the category tint over the window colour, or a neutral
/// wash of the text colour. Opaque on purpose, so the label contrast is exactly what the test measures.
fn tile_rgb(
    cat: Option<Category>,
    folder: bool,
    depth: u8,
    dark: bool,
    window: Rgb,
    ink: Rgb,
) -> Rgb {
    match cat {
        Some(c) => {
            let (r, g, b) = tint(c, dark);
            let a = if dark { FILL_ALPHA.1 } else { FILL_ALPHA.0 };
            Rgb::u8(r, g, b).over(a, window)
        }
        None if folder => window.mix(ink, if depth == 0 { 0.06 } else { 0.10 }),
        None => window.mix(ink, 0.14),
    }
}

/// Where the page is in its life.
enum Phase {
    /// Nothing scanned yet.
    Idle,
    /// A scan is running.
    Scanning { root: PathBuf, started: Instant },
    /// A tree is on screen.
    Done,
    /// The scan could not start.
    Failed(String),
}

/// A layout kept until the folder or the size changes: folder, size in whole points, tiles.
type CachedLayout = (NodeId, (i32, i32), Rc<Vec<Tile>>);

/// A drill in flight: the tile that was clicked grows to fill the map.
struct Anim {
    drill: Drill,
    target: NodeId,
    /// The layout that was on screen when the click happened, kept still underneath.
    from: Rc<Vec<Tile>>,
}

/// The disk map view.
pub(crate) struct DiskView {
    focus: FocusHandle,
    phase: Phase,
    scan: Option<Arc<Scan>>,
    root_path: PathBuf,
    current: NodeId,
    /// The tile the keyboard has selected (an index into the current layout).
    selected: Option<usize>,
    cancel: Option<Arc<AtomicBool>>,
    progress: Option<Arc<Progress>>,
    volume_used: Option<u64>,
    scan_id: u64,
    anim: Option<Anim>,
    layout: Option<CachedLayout>,
    scroll: ScrollHandle,
    /// Show the Full Disk Access banner even when the scan was not refused anything (review renders).
    fda_preview: bool,
    /// The map's width at the last frame, so the legend row can match it.
    map_w: f32,
}

impl DiskView {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            phase: Phase::Idle,
            scan: None,
            root_path: PathBuf::new(),
            current: NodeId::ROOT,
            selected: None,
            cancel: None,
            progress: None,
            volume_used: None,
            scan_id: 0,
            anim: None,
            layout: None,
            scroll: ScrollHandle::new(),
            fda_preview: false,
            map_w: 600.0,
        }
    }

    // ---- scanning ----

    /// Scan a folder on a worker thread. Progress is polled; `cancel_scan` stops it.
    pub(crate) fn start_scan(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.cancel_scan_quiet();
        self.scan_id += 1;
        let id = self.scan_id;
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Progress::default());
        let opts = ScanOptions {
            cancel: Some(cancel.clone()),
            progress: Some(progress.clone()),
            ..ScanOptions::default()
        };
        self.cancel = Some(cancel);
        self.progress = Some(progress);
        self.volume_used = panoply_disk::volume_info(&root)
            .ok()
            .map(|v| v.total.saturating_sub(v.free));
        self.phase = Phase::Scanning {
            root: root.clone(),
            started: Instant::now(),
        };
        let (tx, rx) = futures::channel::oneshot::channel();
        let scan_root = root.clone();
        std::thread::spawn(move || {
            let _ = tx.send(panoply_disk::scan(&scan_root, &opts));
        });
        cx.spawn(async move |this, cx| {
            let done = rx.await;
            if let Ok(r) = done {
                let _ = this.update(cx, |v, cx| v.finished(id, root, r, cx));
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let alive = this
                    .update(cx, |v, cx| {
                        let live = v.scan_id == id && matches!(v.phase, Phase::Scanning { .. });
                        if live {
                            cx.notify();
                        }
                        live
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn finished(
        &mut self,
        id: u64,
        root: PathBuf,
        result: Result<Scan, ScanError>,
        cx: &mut Context<Self>,
    ) {
        if id != self.scan_id || !matches!(self.phase, Phase::Scanning { .. }) {
            return;
        }
        self.cancel = None;
        match result {
            Ok(scan) if scan.report.cancelled => self.phase = Phase::Idle,
            Ok(scan) => self.load(scan, root, cx),
            Err(e) => self.phase = Phase::Failed(e.to_string()),
        }
        cx.notify();
    }

    /// Show a finished scan.
    pub(crate) fn load(&mut self, scan: Scan, root: PathBuf, cx: &mut Context<Self>) {
        self.scan = Some(Arc::new(scan));
        self.root_path = root;
        self.current = NodeId::ROOT;
        self.selected = None;
        self.layout = None;
        self.anim = None;
        self.phase = Phase::Done;
        cx.notify();
    }

    fn cancel_scan_quiet(&mut self) {
        if let Some(c) = self.cancel.take() {
            c.store(true, Ordering::Relaxed);
        }
    }

    fn cancel_scan(&mut self, cx: &mut Context<Self>) {
        self.cancel_scan_quiet();
        self.scan_id += 1;
        self.phase = if self.scan.is_some() {
            Phase::Done
        } else {
            Phase::Idle
        };
        cx.notify();
    }

    fn scan_home(&mut self, cx: &mut Context<Self>) {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        self.start_scan(home, cx);
    }

    fn choose_folder(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Scan".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(p) = paths.into_iter().next()
            {
                let _ = this.update(cx, |v, cx| v.start_scan(p, cx));
            }
        })
        .detach();
    }

    // ---- navigation ----

    /// Folders from the scan root down to the current one.
    fn crumbs(&self) -> Vec<NodeId> {
        let Some(scan) = &self.scan else {
            return Vec::new();
        };
        let mut chain = vec![self.current];
        let mut cur = self.current;
        while let Some(p) = scan.tree.parent(cur) {
            chain.push(p);
            cur = p;
        }
        chain.reverse();
        chain
    }

    pub(crate) fn go_up(&mut self, cx: &mut Context<Self>) {
        if self.anim.is_some() {
            return;
        }
        if let Some(scan) = &self.scan
            && let Some(p) = scan.tree.parent(self.current)
        {
            self.jump(p, cx);
        }
    }

    /// Go straight to a folder, no animation.
    fn jump(&mut self, node: NodeId, cx: &mut Context<Self>) {
        self.current = node;
        self.selected = None;
        self.layout = None;
        self.anim = None;
        cx.notify();
    }

    /// Open a folder with the drill animation from `from` (the tile's rectangle), or instantly when
    /// Reduce Motion is on or there is no rectangle to grow from.
    fn drill_into(
        &mut self,
        node: NodeId,
        from: Option<Rect>,
        bounds: Rect,
        cx: &mut Context<Self>,
    ) {
        let reduce = crate::platform::reduce_motion();
        match (from, self.layout.as_ref()) {
            (Some(from), Some((_, _, tiles))) if !reduce => {
                self.anim = Some(Anim {
                    drill: Drill::new(from, bounds, Instant::now(), false),
                    target: node,
                    from: tiles.clone(),
                });
                cx.notify();
            }
            _ => self.jump(node, cx),
        }
    }

    fn tile_clicked(&mut self, idx: usize, bounds: Rect, cx: &mut Context<Self>) {
        if self.anim.is_some() {
            return;
        }
        let (Some(scan), Some((_, _, tiles))) = (self.scan.clone(), self.layout.clone()) else {
            return;
        };
        let Some(t) = tiles.get(idx) else { return };
        if t.is_more() {
            self.selected = Some(idx);
            cx.notify();
        } else if scan.tree.kind(t.node) == Kind::Dir && scan.tree.child_count(t.node) > 0 {
            self.drill_into(t.node, Some(t.rect), bounds, cx);
        } else {
            self.selected = Some(idx);
            cx.notify();
        }
    }

    /// Open a folder named in the list: grow it from its tile when it has one on screen.
    fn open_node(&mut self, node: NodeId, bounds: Rect, cx: &mut Context<Self>) {
        let rect = self.layout.as_ref().and_then(|(_, _, tiles)| {
            tiles
                .iter()
                .find(|t| t.node == node && !t.is_more())
                .map(|t| t.rect)
        });
        self.drill_into(node, rect, bounds, cx);
    }

    /// Arrow keys and Enter on the map.
    fn on_map_key(&mut self, ev: &KeyDownEvent, bounds: Rect, cx: &mut Context<Self>) -> bool {
        let Some((_, _, tiles)) = self.layout.clone() else {
            return false;
        };
        let top: Vec<usize> = tiles
            .iter()
            .enumerate()
            .filter(|(_, t)| t.depth == 0)
            .map(|(i, _)| i)
            .collect();
        if top.is_empty() {
            return false;
        }
        let pos = self
            .selected
            .and_then(|s| top.iter().position(|i| *i == s))
            .unwrap_or(usize::MAX);
        match ev.keystroke.key.as_str() {
            "right" | "down" => {
                let next = if pos == usize::MAX {
                    0
                } else {
                    (pos + 1).min(top.len() - 1)
                };
                self.selected = Some(top[next]);
            }
            "left" | "up" => {
                let prev = if pos == usize::MAX {
                    0
                } else {
                    pos.saturating_sub(1)
                };
                self.selected = Some(top[prev]);
            }
            "enter" => {
                if let Some(s) = self.selected {
                    self.tile_clicked(s, bounds, cx);
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    // ---- sizes ----

    /// The map's size in points, from the window: what is left of the width after the app sidebar,
    /// the list and the margins, and of the height after the toolbar, path bar and legend.
    fn map_size(&self, window: &Window) -> (f32, f32) {
        let vp = window.viewport_size();
        let title = if chrome::draws_title_bar(window) {
            chrome::top_inset(window)
        } else {
            0.0
        };
        let banner = if self.banner_shown() { BANNER_H } else { 0.0 };
        let w = vp.width.as_f32() - APP_SIDEBAR - SIDE_W - sp::L * 3.0 - 4.0;
        let h = vp.height.as_f32()
            - title
            - chrome::MAC_TOOLBAR
            - BAR_H * 2.0
            - sp::L * 2.0
            - sp::S * 2.0
            - banner;
        (w.floor().max(120.0), h.floor().max(120.0))
    }

    fn banner_shown(&self) -> bool {
        matches!(self.phase, Phase::Done)
            && (self.fda_preview
                || self
                    .scan
                    .as_ref()
                    .is_some_and(|s| s.report.needs_full_disk_access()))
    }

    /// Review renders: open a folder of the current one by name, no animation.
    #[cfg(feature = "snapshot")]
    pub(crate) fn open_named(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(scan) = self.scan.clone() else {
            return;
        };
        if let Some(n) = scan
            .tree
            .children(self.current)
            .find(|n| scan.tree.name(*n) == name)
        {
            self.jump(n, cx);
        }
    }

    /// Review renders: show the banner as if the scan had been refused folders.
    #[cfg(feature = "snapshot")]
    pub(crate) fn preview_fda_banner(&mut self, on: bool, cx: &mut Context<Self>) {
        self.fda_preview = on;
        cx.notify();
    }

    fn options() -> LayoutOptions {
        LayoutOptions {
            label_min_h: 22.0,
            header_h: 22.0,
            min_area: 700.0,
            ..LayoutOptions::default()
        }
    }

    /// The layout of the current folder at this size, cached until either changes.
    fn tiles(&mut self, w: f32, h: f32) -> Rc<Vec<Tile>> {
        let key = (w as i32, h as i32);
        if let Some((n, k, t)) = &self.layout
            && *n == self.current
            && *k == key
        {
            return t.clone();
        }
        let Some(scan) = &self.scan else {
            return Rc::new(Vec::new());
        };
        let tiles = Rc::new(squarify(
            &scan.tree,
            self.current,
            Rect::new(0.0, 0.0, w, h),
            &Self::options(),
        ));
        self.layout = Some((self.current, key, tiles.clone()));
        tiles
    }

    // ---- pieces ----

    /// The slot for the Full Disk Access explainer. Unit B fills it with the real explainer; for now
    /// it is one calm line.
    fn fda_banner(&self, t: Theme) -> Option<AnyElement> {
        if !self.banner_shown() {
            return None;
        }
        Some(
            div()
                .id("disk-fda-banner")
                .role(Role::Group)
                .aria_label("Full Disk Access is off")
                .flex_none()
                .h(px(BANNER_H - sp::S))
                .overflow_hidden()
                .child(notice(
                    "triangle-alert",
                    t.warn,
                    "Some folders were not counted. Full Disk Access would let Panoply read them.",
                    t,
                ))
                .into_any_element(),
        )
    }

    fn toolbar_buttons(&self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let scanning = matches!(self.phase, Phase::Scanning { .. });
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(sp::S))
            .when(scanning, |d| {
                d.child(button(
                    "disk-cancel",
                    "Cancel",
                    ButtonKind::Secondary,
                    Some("x"),
                    t,
                    cx.listener(|v, _, _, cx| v.cancel_scan(cx)),
                ))
            })
            .when(!scanning, |d| {
                d.child(button(
                    "disk-choose",
                    "Choose folder",
                    ButtonKind::Secondary,
                    Some("folder-open"),
                    t,
                    cx.listener(|v, _, _, cx| v.choose_folder(cx)),
                ))
                .child(button(
                    "disk-rescan",
                    "Scan again",
                    ButtonKind::Secondary,
                    Some("refresh-cw"),
                    t,
                    cx.listener(|v, _, _, cx| {
                        let root = if v.root_path.as_os_str().is_empty() {
                            None
                        } else {
                            Some(v.root_path.clone())
                        };
                        match root {
                            Some(r) => v.start_scan(r, cx),
                            None => v.scan_home(cx),
                        }
                    }),
                ))
            })
            // The review flow (move to trash) is the next unit's job: the button holds its place.
            .child(button_ex(
                "disk-review",
                "Review and move to trash",
                ButtonKind::Primary,
                None,
                false,
                t,
                |_, _, _| {},
            ))
            .into_any_element()
    }

    fn crumb_bar(&self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(scan) = self.scan.clone() else {
            return div().h(px(BAR_H)).into_any_element();
        };
        let chain = self.crumbs();
        let n = chain.len();
        let label = |id: NodeId| -> String {
            if id == NodeId::ROOT {
                let p = &self.root_path;
                p.file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.to_string_lossy().into_owned())
            } else {
                scan.tree.name(id)
            }
        };
        // A long path keeps the first folder, an ellipsis and the last three.
        let hidden = n.saturating_sub(5);
        let mut row = div()
            .id("disk-path")
            .role(Role::Navigation)
            .aria_label("Folder path")
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .h(px(BAR_H))
            .gap(px(2.))
            .overflow_hidden()
            .child(
                icon_button(
                    "disk-up",
                    "Up one folder",
                    "arrow-left",
                    false,
                    t,
                    cx.listener(|v, _, _, cx| v.go_up(cx)),
                )
                .when(n <= 1, |b| b.opacity(0.4)),
            );
        for (i, id) in chain.iter().enumerate() {
            if hidden > 0 && i > 0 && i <= hidden {
                if i == 1 {
                    row = row.child(sep(t)).child(
                        div()
                            .ramp(ty::SMALL)
                            .text_color(t.ink3)
                            .px(px(sp::XS))
                            .child("..."),
                    );
                }
                continue;
            }
            if i > 0 {
                row = row.child(sep(t));
            }
            let last = i + 1 == n;
            let id = *id;
            let name = label(id);
            row = row.child(
                focus_border(div().id(("disk-crumb", i)), t)
                    .role(Role::Button)
                    .aria_label(format!("Go to {name}"))
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(px(24.))
                    .px(px(6.))
                    .max_w(px(180.))
                    .rounded(px(radius::KEY))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .ramp(ty::SMALL)
                    .font_weight(if last {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if last { t.ink } else { t.ink2 })
                    .hover(move |s| s.bg(t.fill8))
                    .on_click(cx.listener(move |v, _, _, cx| v.jump(id, cx)))
                    .child(name),
            );
        }
        row.into_any_element()
    }

    fn legend(&self, t: Theme) -> AnyElement {
        let pal = t.palette;
        let swatch = |rgb: Rgb| {
            div()
                .flex_none()
                .size(px(10.))
                .rounded(px(3.))
                .border_1()
                .border_color(t.hair)
                .bg(rgb.hsla(1.0))
        };
        let item = |label: &str, rgb: Rgb| {
            div()
                .flex()
                .flex_none()
                .flex_row()
                .items_center()
                .gap(px(5.))
                .child(swatch(rgb))
                .child(
                    div()
                        .ramp(ty::CAPTION)
                        .text_color(t.ink2)
                        .child(label.to_string()),
                )
        };
        div()
            .id("disk-legend")
            .role(Role::Group)
            .aria_label("Colour key: images, video, audio, PDF, documents, text, code, archives, data, folders")
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .h(px(BAR_H))
            .gap(px(10.))
            .w(px(self.map_w))
            .overflow_hidden()
            .children(LEGEND.iter().map(|(label, c)| {
                item(label, tile_rgb(Some(*c), false, 0, t.dark, pal.window, pal.ink))
            }))
            .child(item(
                "Folders",
                tile_rgb(None, true, 0, t.dark, pal.window, pal.ink),
            ))
            .into_any_element()
    }

    /// One tile of the map.
    #[allow(clippy::too_many_arguments)]
    fn tile_el(
        &self,
        i: usize,
        tile: &Tile,
        tree: &Tree,
        t: Theme,
        interactive: Option<Rect>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pal = t.palette;
        let kind = tree.kind(tile.node);
        let more = tile.more;
        let (name, bytes) = match more {
            Some(m) => (format!("{} more", m.count), m.bytes),
            None => (tree.name(tile.node), tree.size(tile.node)),
        };
        let folder = more.is_none() && kind == Kind::Dir;
        let cat = if more.is_none() && matches!(kind, Kind::File | Kind::Symlink) {
            category_of(&name)
        } else {
            None
        };
        let fill = tile_rgb(
            cat,
            folder || more.is_some(),
            tile.depth,
            t.dark,
            pal.window,
            pal.ink,
        );
        let r = tile.rect;
        let selected = self.selected == Some(i);
        let size_text = format_bytes(bytes);
        let nested_header = folder && tile.depth == 0;
        let drillable = folder && tree.child_count(tile.node) > 0;
        let label = format!(
            "{name}, {size_text}{}",
            if drillable { ", folder" } else { "" }
        );
        let el = div()
            .id(("disk-tile", i))
            .role(Role::Button)
            .aria_label(label.clone())
            .absolute()
            .left(px(r.x))
            .top(px(r.y))
            .w(px(r.w))
            .h(px(r.h))
            .rounded(px(if tile.depth == 0 { 4.0 } else { 3.0 }))
            .border_2()
            .border_color(if selected {
                t.accent_text
            } else {
                gpui::transparent_black()
            })
            .bg(fill.hsla(1.0))
            .overflow_hidden()
            .when(interactive.is_some(), |d| {
                let bounds = interactive.unwrap_or_default();
                d.hover(move |s| s.border_color(t.accent_text))
                    .tooltip(tip(&label, &[]))
                    .on_click(cx.listener(move |v, _, _, cx| v.tile_clicked(i, bounds, cx)))
            })
            .when(tile.label_fits, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .px(px(4.))
                        .child(
                            div()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .ramp(ty::SMALL)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.ink)
                                .child(name.clone()),
                        )
                        .when(!nested_header && r.h >= 44.0, |d| {
                            d.child(
                                div()
                                    .ramp(ty::CAPTION)
                                    .text_color(t.ink)
                                    .child(size_text.clone()),
                            )
                        }),
                )
            });
        el.into_any_element()
    }

    fn map(&mut self, window: &mut Window, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let (w, h) = self.map_size(window);
        self.map_w = w + 4.0;
        let bounds = Rect::new(0.0, 0.0, w, h);
        let Some(scan) = self.scan.clone() else {
            return div().into_any_element();
        };
        let now = Instant::now();
        // A finished drill lands on the folder it grew into.
        if let Some(a) = &self.anim
            && a.drill.done(now)
        {
            let node = a.target;
            self.jump(node, cx);
        }
        let tiles = self.tiles(w, h);
        let tree = &scan.tree;
        let mut layer = div()
            .relative()
            .size_full()
            .overflow_hidden()
            .rounded(px(radius::CONTROL));
        if let Some(a) = &self.anim {
            window.request_animation_frame();
            let p = a.drill.progress(now);
            let r = a.drill.rect(now);
            let inner = squarify(
                tree,
                a.target,
                Rect::new(0.0, 0.0, r.w, r.h),
                &Self::options(),
            );
            layer = layer
                .children(
                    a.from
                        .iter()
                        .enumerate()
                        .map(|(i, tile)| self.tile_el(i, tile, tree, t, None, cx)),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(r.x))
                        .top(px(r.y))
                        .w(px(r.w))
                        .h(px(r.h))
                        .overflow_hidden()
                        .rounded(px(4.))
                        .bg(t.window)
                        .opacity(p)
                        .children(
                            inner
                                .iter()
                                .enumerate()
                                .map(|(i, tile)| self.tile_el(i, tile, tree, t, None, cx)),
                        ),
                );
        } else {
            layer = layer.children(
                tiles
                    .iter()
                    .enumerate()
                    .map(|(i, tile)| self.tile_el(i, tile, tree, t, Some(bounds), cx)),
            );
        }
        let empty = tiles.is_empty() && self.anim.is_none();
        let current_name = if self.current == NodeId::ROOT {
            self.root_path.to_string_lossy().into_owned()
        } else {
            tree.name(self.current)
        };
        focus_border(div().id("disk-map"), t)
            .track_focus(&self.focus)
            .role(Role::Group)
            .aria_label(format!(
                "Disk map of {current_name}. Arrow keys choose a tile, Enter opens a folder, Backspace goes up."
            ))
            .flex_none()
            .w(px(w + 4.0))
            .h(px(h + 4.0))
            .rounded(px(radius::CONTROL + 2.0))
            .on_key_down(cx.listener(move |v, ev: &KeyDownEvent, _, cx| {
                if v.on_map_key(ev, bounds, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(layer.bg(t.window))
            .when(empty, |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .ramp(ty::BODY)
                        .text_color(t.ink2)
                        .child("This folder is empty."),
                )
            })
            .into_any_element()
    }

    /// One row of a list: a colour dot, the name, the size.
    fn list_row(
        &self,
        id: impl Into<ElementId>,
        node: NodeId,
        t: Theme,
        scan: &Scan,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Stateful<Div> {
        let tree = &scan.tree;
        let name = tree.name(node);
        let size = format_bytes(tree.size(node));
        let kind = tree.kind(node);
        let pal = t.palette;
        let cat = if matches!(kind, Kind::File | Kind::Symlink) {
            category_of(&name)
        } else if kind == Kind::Dir {
            dominant(tree, node)
        } else {
            None
        };
        let dot = tile_rgb(cat, kind == Kind::Dir, 0, t.dark, pal.window, pal.ink);
        let path = tree.path(node).to_string_lossy().into_owned();
        focus_border(div().id(id), t)
            .role(Role::Button)
            .aria_label(format!("{name}, {size}"))
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .gap(px(sp::S))
            .h(px(32.))
            .px(px(sp::S))
            .rounded(px(radius::CONTROL))
            .hover(move |s| s.bg(t.fill8))
            .active(move |s| s.bg(t.fill12))
            .tooltip(tip(&path, &[]))
            .on_click(on_click)
            .child(
                div()
                    .flex_none()
                    .size(px(10.))
                    .rounded(px(3.))
                    .border_1()
                    .border_color(t.hair)
                    .bg(dot.hsla(1.0)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .ramp(ty::BODY)
                    .text_color(t.ink)
                    .child(name),
            )
            .child(
                div()
                    .flex_none()
                    .ramp(ty::SMALL)
                    .text_color(t.ink2)
                    .child(size),
            )
    }

    fn side_list(&mut self, window: &Window, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(scan) = self.scan.clone() else {
            return div().into_any_element();
        };
        let (w, h) = self.map_size(window);
        let bounds = Rect::new(0.0, 0.0, w, h);
        let tree = &scan.tree;
        let here: Vec<NodeId> = tree
            .children_by_size(self.current)
            .into_iter()
            .filter(|n| tree.size(*n) > 0)
            .take(LIST_ROWS)
            .collect();
        let big = tree.largest(BIG_ROWS, Kind::File);
        let total = tree.size(self.current);
        let mut col = div()
            .id("disk-side-scroll")
            .flex()
            .flex_col()
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .gap(px(2.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_baseline()
                    .justify_between()
                    .px(px(sp::S))
                    .pb(px(sp::XS))
                    .child(caption("In this folder", t))
                    .child(
                        div()
                            .ramp(ty::SMALL)
                            .text_color(t.ink3)
                            .child(format_bytes(total)),
                    ),
            );
        for (i, n) in here.iter().enumerate() {
            let n = *n;
            let is_dir = tree.kind(n) == Kind::Dir && tree.child_count(n) > 0;
            col = col.child(self.list_row(
                ("disk-row", i),
                n,
                t,
                &scan,
                cx.listener(move |v, _, _, cx| {
                    if is_dir {
                        v.open_node(n, bounds, cx);
                    }
                }),
            ));
        }
        if !big.is_empty() {
            col = col.child(
                div()
                    .px(px(sp::S))
                    .pt(px(sp::M))
                    .pb(px(sp::XS))
                    .child(caption("Largest files", t)),
            );
            for (i, n) in big.iter().enumerate() {
                let n = *n;
                col = col.child(self.list_row(
                    ("disk-big", i),
                    n,
                    t,
                    &scan,
                    // Show the folder the file lives in.
                    cx.listener(move |v, _, _, cx| {
                        if let Some(p) = v.scan.as_ref().and_then(|s| s.tree.parent(n)) {
                            v.jump(p, cx);
                        }
                    }),
                ));
            }
        }
        div()
            .id("disk-side")
            .role(Role::Group)
            .aria_label("Largest items")
            .flex_none()
            .w(px(SIDE_W))
            .h_full()
            .p(px(sp::S))
            .rounded(px(radius::CARD))
            .bg(t.card)
            .border_1()
            .border_color(t.hair)
            .child(col)
            .into_any_element()
    }

    fn start_state(&self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let body = match &self.phase {
            Phase::Scanning { root, started } => {
                let p = self.progress.as_ref();
                let files = p.map_or(0, |p| p.files.load(Ordering::Relaxed));
                let bytes = p.map_or(0, |p| p.bytes.load(Ordering::Relaxed));
                let fraction = self
                    .volume_used
                    .filter(|u| *u > 0)
                    .map(|u| (bytes as f32 / u as f32).min(0.99));
                let line = format!(
                    "{} files, {}, {} s",
                    group_digits(files),
                    format_bytes(bytes),
                    started.elapsed().as_secs()
                );
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(sp::S))
                    .child(icon("hard-drive", 28., t.ink2))
                    .child(
                        div()
                            .ramp(ty::TITLE)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.ink)
                            .child(format!("Scanning {}", root.to_string_lossy())),
                    )
                    .child(
                        div()
                            .w(px(320.))
                            .child(super::widgets::progress_bar(fraction, "Scan progress", t)),
                    )
                    .child(div().ramp(ty::BODY).text_color(t.ink2).child(line))
                    .child(button(
                        "disk-cancel-center",
                        "Cancel",
                        ButtonKind::Secondary,
                        Some("x"),
                        t,
                        cx.listener(|v, _, _, cx| v.cancel_scan(cx)),
                    ))
            }
            Phase::Failed(msg) => div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(sp::S))
                .child(icon("triangle-alert", 28., t.warn))
                .child(
                    div()
                        .ramp(ty::TITLE)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.ink)
                        .child("The scan could not start"),
                )
                .child(
                    div()
                        .max_w(px(420.))
                        .text_center()
                        .ramp(ty::BODY)
                        .text_color(t.ink2)
                        .child(msg.clone()),
                )
                .child(start_buttons(t, cx)),
            _ => div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(sp::S))
                .child(icon("hard-drive", 28., t.ink2))
                .child(
                    div()
                        .ramp(ty::TITLE)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.ink)
                        .child("See what is using your space"),
                )
                .child(
                    div()
                        .max_w(px(420.))
                        .text_center()
                        .ramp(ty::BODY)
                        .text_color(t.ink2)
                        .child("Panoply reads sizes only and never changes a file. Scan your home folder, or choose another."),
                )
                .child(start_buttons(t, cx)),
        };
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .child(body)
            .into_any_element()
    }

    /// The summary line under the map: what the scan counted.
    fn summary(&self, t: Theme) -> Option<AnyElement> {
        let scan = self.scan.as_ref()?;
        let r = &scan.report;
        let line = format!(
            "{} in {} files, scanned in {:.1} s",
            format_bytes(r.total_bytes),
            group_digits(r.files),
            r.elapsed.as_secs_f32()
        );
        Some(
            div()
                .ramp(ty::SMALL)
                .text_color(t.ink3)
                .child(line)
                .into_any_element(),
        )
    }
}

fn sep(t: Theme) -> impl IntoElement {
    div().flex_none().child(icon("chevron-right", 12., t.ink3))
}

fn start_buttons(t: Theme, cx: &mut Context<DiskView>) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .gap(px(sp::S))
        .pt(px(sp::S))
        .child(button(
            "disk-scan-home",
            "Scan home folder",
            ButtonKind::Primary,
            Some("hard-drive"),
            t,
            cx.listener(|v, _, _, cx| v.scan_home(cx)),
        ))
        .child(button(
            "disk-pick",
            "Choose folder",
            ButtonKind::Secondary,
            Some("folder-open"),
            t,
            cx.listener(|v, _, _, cx| v.choose_folder(cx)),
        ))
}

/// 1234567 as "1,234,567".
fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

impl Render for DiskView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::new(is_dark(window));
        let trailing = self.toolbar_buttons(t, cx);
        let body: AnyElement = if matches!(self.phase, Phase::Done) && self.scan.is_some() {
            let map = self.map(window, t, cx);
            let side = self.side_list(window, t, cx);
            let crumbs = self.crumb_bar(t, cx);
            let legend = self.legend(t);
            let summary = self.summary(t);
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .p(px(sp::L))
                .gap(px(sp::S))
                .children(self.fda_banner(t))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_1()
                        .min_h_0()
                        .gap(px(sp::L))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_none()
                                .w(px(self.map_w))
                                .gap(px(sp::S))
                                .child(crumbs)
                                .child(map)
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(sp::M))
                                        .child(legend),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_none()
                                .gap(px(sp::S))
                                .h_full()
                                .child(div().h(px(BAR_H)).flex().items_center().children(summary))
                                .child(div().flex_1().min_h_0().child(side)),
                        ),
                )
                .into_any_element()
        } else {
            self.start_state(t, cx)
        };
        div()
            .id("disk")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(super::app::toolbar("Disk map", Some(trailing), t))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    // Not `use super::*`: that pulls in gpui's own `test` macro over the standard one.
    use super::{Category, LEGEND, Rgb, category_of, group_digits, tile_rgb};
    use crate::ui::theme::Palette;

    #[test]
    fn extensions_map_to_colour_groups() {
        assert_eq!(category_of("IMG_0001.HEIC"), Some(Category::Images));
        assert_eq!(category_of("clip.mp4"), Some(Category::Video));
        assert_eq!(category_of("song.flac"), Some(Category::Audio));
        assert_eq!(category_of("Lease.pdf"), Some(Category::Pdf));
        assert_eq!(category_of("budget.xlsx"), Some(Category::Documents));
        assert_eq!(category_of("main.rs"), Some(Category::Developer));
        assert_eq!(category_of("libfoo.rlib"), Some(Category::Developer));
        assert_eq!(category_of("backup.dmg"), Some(Category::Capture));
        assert_eq!(category_of("cache.sqlite"), Some(Category::Calculators));
        assert_eq!(category_of("README"), None);
        assert_eq!(category_of("weird.zzz"), None);
    }

    #[test]
    fn digits_group_by_three() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1234), "1,234");
        assert_eq!(group_digits(1234567), "1,234,567");
    }

    /// Tile labels (both lines use the primary text colour) read at 4.5:1 or better on every tile fill, in light, dark and
    /// both contrast modes, as theme.rs measures its own pairs.
    #[test]
    fn tile_labels_pass_contrast_on_every_fill() {
        let cats = [
            Category::Images,
            Category::Video,
            Category::Audio,
            Category::Pdf,
            Category::Documents,
            Category::Text,
            Category::Developer,
            Category::Capture,
            Category::Calculators,
        ];
        for dark in [false, true] {
            for hc in [false, true] {
                let p = Palette::new(dark, hc, Rgb::u8(0, 122, 255));
                let mut fills: Vec<(String, Rgb)> = cats
                    .iter()
                    .map(|c| {
                        (
                            format!("{c:?}"),
                            tile_rgb(Some(*c), false, 0, dark, p.window, p.ink),
                        )
                    })
                    .collect();
                for depth in [0u8, 1] {
                    fills.push((
                        format!("folder{depth}"),
                        tile_rgb(None, true, depth, dark, p.window, p.ink),
                    ));
                }
                fills.push((
                    "other".into(),
                    tile_rgb(None, false, 0, dark, p.window, p.ink),
                ));
                for (name, f) in fills {
                    let c = p.ink.contrast(f);
                    assert!(c >= 4.5, "ink on {name} (dark {dark}, hc {hc}) is {c:.2}:1");
                }
            }
        }
    }

    /// The legend swatches tell tints apart from the window and from each other: at least 1.15:1
    /// against the window, so none vanishes.
    #[test]
    fn tiles_stand_off_the_window() {
        for dark in [false, true] {
            let p = Palette::new(dark, false, Rgb::u8(0, 122, 255));
            for (_, c) in LEGEND {
                let f = tile_rgb(Some(c), false, 0, dark, p.window, p.ink);
                assert!(f.contrast(p.window) >= 1.15, "{c:?} dark {dark}");
            }
        }
    }
}
