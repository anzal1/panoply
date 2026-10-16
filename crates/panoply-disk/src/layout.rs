//! Disk map geometry: a squarified treemap, a sunburst and the drill animation. Pure maths over a
//! [`Tree`], with no UI types, so every view (and every test) can use it.
//!
//! The treemap is the squarified layout of Bruls, Huizing and van Wijk: items go into rows along the
//! shorter side of the free space, and a row is closed when adding one more item would make its worst
//! aspect ratio worse. Order is deterministic: largest first, ties by name.

use std::time::{Duration, Instant};

use crate::tree::{Kind, NodeId, Tree};

/// A rectangle in whatever unit the caller draws in (pixels, points).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// A rectangle from its corner and size.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    /// Width times height.
    pub fn area(&self) -> f32 {
        self.w * self.h
    }

    /// Right edge.
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// Bottom edge.
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// True when the point is inside (left and top edges count, right and bottom do not).
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    /// Shrink by `d` on every side. Never goes below zero size.
    pub fn inset(&self, d: f32) -> Rect {
        let w = (self.w - 2.0 * d).max(0.0);
        let h = (self.h - 2.0 * d).max(0.0);
        Rect {
            x: self.x + (self.w - w) / 2.0,
            y: self.y + (self.h - h) / 2.0,
            w,
            h,
        }
    }

    /// Linear blend between two rectangles, `t` in 0..=1.
    pub fn lerp(&self, to: &Rect, t: f32) -> Rect {
        let f = |a: f32, b: f32| a + (b - a) * t;
        Rect {
            x: f(self.x, to.x),
            y: f(self.y, to.y),
            w: f(self.w, to.w),
            h: f(self.h, to.h),
        }
    }
}

/// One drawn tile of the treemap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    /// The entry. For a merged "N more" tile this is the folder the merged entries belong to.
    pub node: NodeId,
    /// Where it goes.
    pub rect: Rect,
    /// 0 for the children of the root you asked for, 1 for their children, and so on.
    pub depth: u8,
    /// True when a name label of the minimum label size fits inside the tile.
    pub label_fits: bool,
    /// Set on the merged tile that stands for the entries too small to draw: how many it holds.
    /// `node` is the folder they belong to.
    pub more: Option<MoreInfo>,
}

impl Tile {
    /// True for the merged "N more" tile.
    pub fn is_more(&self) -> bool {
        self.more.is_some()
    }
}

/// What a merged "N more" tile holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoreInfo {
    /// How many entries were merged.
    pub count: u32,
    /// Their total allocated bytes.
    pub bytes: u64,
}

/// Knobs for [`squarify`]. [`LayoutOptions::default`] is what the app uses.
#[derive(Clone, Copy, Debug)]
pub struct LayoutOptions {
    /// How many levels to draw: 1 is the children of the root only, 2 adds their children inside.
    pub levels: u8,
    /// Gap between a folder tile and the tiles nested in it, and the padding at the border.
    pub inset: f32,
    /// Gap kept between sibling tiles (half is cut from each side).
    pub gap: f32,
    /// Tiles with less area than this are merged into one "N more" tile.
    pub min_area: f32,
    /// A label fits when the tile is at least this wide...
    pub label_min_w: f32,
    /// ...and at least this tall.
    pub label_min_h: f32,
    /// A folder gets nested tiles only when its tile is at least this wide and tall.
    pub nest_min_side: f32,
    /// Height kept at the top of a nested folder tile for its own label.
    pub header_h: f32,
}

impl Default for LayoutOptions {
    fn default() -> LayoutOptions {
        LayoutOptions {
            levels: 2,
            inset: 3.0,
            gap: 1.0,
            min_area: 400.0,
            label_min_w: 56.0,
            label_min_h: 18.0,
            nest_min_side: 90.0,
            header_h: 18.0,
        }
    }
}

struct Item {
    node: NodeId,
    size: u64,
}

/// The entries of a folder that have something to show, largest first, ties by name. Entries of
/// size zero are dropped.
fn sorted_children(tree: &Tree, id: NodeId) -> Vec<Item> {
    tree.children_by_size(id)
        .into_iter()
        .map(|n| Item {
            node: n,
            size: tree.size(n),
        })
        .filter(|i| i.size > 0)
        .collect()
}

/// Worst aspect ratio of a row of areas laid along a side of length `side`.
fn worst(row: &[f64], side: f64) -> f64 {
    let sum: f64 = row.iter().sum();
    if sum <= 0.0 || side <= 0.0 {
        return f64::INFINITY;
    }
    let max = row.iter().cloned().fold(f64::MIN, f64::max);
    let min = row.iter().cloned().fold(f64::MAX, f64::min);
    let s2 = side * side;
    ((s2 * max) / (sum * sum)).max((sum * sum) / (s2 * min))
}

/// Squarified layout of `areas` (already scaled to add up to the area of `bounds`, in order,
/// largest first). Returns one rectangle per area. Rows tile `bounds` exactly.
fn squarify_areas(areas: &[f64], bounds: Rect) -> Vec<Rect> {
    let mut out = Vec::with_capacity(areas.len());
    let (mut x, mut y, mut w, mut h) = (
        bounds.x as f64,
        bounds.y as f64,
        bounds.w as f64,
        bounds.h as f64,
    );
    let mut i = 0;
    while i < areas.len() {
        let side = w.min(h);
        // Grow the row while the worst ratio keeps improving.
        let mut j = i + 1;
        while j < areas.len() {
            if worst(&areas[i..j + 1], side) <= worst(&areas[i..j], side) {
                j += 1;
            } else {
                break;
            }
        }
        let row = &areas[i..j];
        let sum: f64 = row.iter().sum();
        let last_row = j == areas.len();
        if w >= h {
            // Row is a column on the left, as tall as the free space.
            let col_w = if last_row || h <= 0.0 {
                w
            } else {
                (sum / h).min(w)
            };
            let mut cy = y;
            for (k, a) in row.iter().enumerate() {
                let th = if k + 1 == row.len() {
                    y + h - cy
                } else if col_w > 0.0 {
                    a / col_w
                } else {
                    0.0
                };
                out.push(Rect::new(x as f32, cy as f32, col_w as f32, th as f32));
                cy += th;
            }
            x += col_w;
            w -= col_w;
        } else {
            // Row is a strip along the top, as wide as the free space.
            let row_h = if last_row || w <= 0.0 {
                h
            } else {
                (sum / w).min(h)
            };
            let mut cx = x;
            for (k, a) in row.iter().enumerate() {
                let tw = if k + 1 == row.len() {
                    x + w - cx
                } else if row_h > 0.0 {
                    a / row_h
                } else {
                    0.0
                };
                out.push(Rect::new(cx as f32, y as f32, tw as f32, row_h as f32));
                cx += tw;
            }
            y += row_h;
            h -= row_h;
        }
        i = j;
    }
    out
}

/// Lay out `items` inside `bounds`: split off the entries that would get less than `min_area`,
/// squarify the rest, and give the leftovers one merged tile. Returns tiles with no nesting yet.
fn layout_level(
    parent: NodeId,
    items: &[Item],
    bounds: Rect,
    depth: u8,
    opts: &LayoutOptions,
) -> Vec<(Tile, Option<usize>)> {
    // The second field is the index into `items` for real tiles, None for the merged tile.
    let total: f64 = items.iter().map(|i| i.size as f64).sum();
    if total <= 0.0 || bounds.area() <= 0.0 {
        return Vec::new();
    }
    let area = bounds.area() as f64;
    let scale = area / total;

    // Entries too small to draw. Items are sorted, so the small ones are a tail. If the tail is a single
    // entry it is still merged, so a lone speck never gets a tile; "1 more" reads fine.
    let mut keep = items.len();
    while keep > 0 && (items[keep - 1].size as f64 * scale) < opts.min_area as f64 {
        keep -= 1;
    }
    let tail = &items[keep..];
    let mut sizes: Vec<f64> = items[..keep].iter().map(|i| i.size as f64).collect();
    let more_bytes: u64 = tail.iter().map(|i| i.size).sum();
    if !tail.is_empty() {
        sizes.push(more_bytes as f64);
    }
    // Merging a tail can make the merged tile large enough to draw (many specks). If the tail as a whole
    // is below the minimum area it is still shown, as the single smallest tile, so area is conserved.
    let areas: Vec<f64> = sizes.iter().map(|s| s * scale).collect();
    let rects = squarify_areas(&areas, bounds);

    let mut out = Vec::with_capacity(rects.len());
    for (k, r) in rects.into_iter().enumerate() {
        let is_more_tile = !tail.is_empty() && k == keep;
        let label_fits = r.w >= opts.label_min_w && r.h >= opts.label_min_h;
        if is_more_tile {
            out.push((
                Tile {
                    node: parent,
                    rect: r,
                    depth,
                    label_fits,
                    more: Some(MoreInfo {
                        count: tail.len() as u32,
                        bytes: more_bytes,
                    }),
                },
                None,
            ));
        } else {
            out.push((
                Tile {
                    node: items[k].node,
                    rect: r,
                    depth,
                    label_fits,
                    more: None,
                },
                Some(k),
            ));
        }
    }
    out
}

/// The squarified treemap of the children of `root` inside `bounds`.
///
/// Tiles come out parents first: every depth 0 tile is followed by the depth 1 tiles nested in it.
/// Siblings are separated by `opts.gap`; nested tiles sit inside their folder's tile, below a header
/// strip and inset by `opts.inset`. Entries too small to draw are merged into one tile per level
/// ([`Tile::is_more`]). Entries of size zero are dropped. The same tree and bounds always give the
/// same tiles.
pub fn squarify(tree: &Tree, root: NodeId, bounds: Rect, opts: &LayoutOptions) -> Vec<Tile> {
    let mut out = Vec::new();
    fill(tree, root, bounds, 0, opts, &mut out);
    out
}

fn fill(
    tree: &Tree,
    parent: NodeId,
    bounds: Rect,
    depth: u8,
    opts: &LayoutOptions,
    out: &mut Vec<Tile>,
) {
    let items = sorted_children(tree, parent);
    for (mut tile, idx) in layout_level(parent, &items, bounds, depth, opts) {
        let nest = idx.is_some_and(|k| {
            tree.kind(items[k].node) == Kind::Dir
                && depth + 1 < opts.levels
                && tile.rect.w >= opts.nest_min_side
                && tile.rect.h >= opts.nest_min_side
        });
        let full = tile.rect;
        // Leave the gap between siblings by shrinking the drawn rectangle only.
        tile.rect = full.inset(opts.gap / 2.0);
        let node = tile.node;
        out.push(tile);
        if nest {
            let mut inner = tile.rect.inset(opts.inset);
            inner.y += opts.header_h - opts.inset;
            inner.h = (inner.h - (opts.header_h - opts.inset)).max(0.0);
            if inner.w > 0.0 && inner.h > 0.0 {
                fill(tree, node, inner, depth + 1, opts, out);
            }
        }
    }
}

/// Which tile is under a point: the deepest one that contains it.
pub fn hit_test(tiles: &[Tile], px: f32, py: f32) -> Option<usize> {
    tiles
        .iter()
        .enumerate()
        .filter(|(_, t)| t.rect.contains(px, py))
        .max_by_key(|(_, t)| t.depth)
        .map(|(i, _)| i)
}

/// One ring segment of the sunburst.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc {
    /// The entry.
    pub node: NodeId,
    /// Start angle in radians, 0 at twelve o'clock, going clockwise.
    pub start: f32,
    /// End angle in radians.
    pub end: f32,
    /// 0 is the ring next to the centre.
    pub ring: u8,
}

/// Knobs for [`sunburst`].
#[derive(Clone, Copy, Debug)]
pub struct SunburstOptions {
    /// Number of rings.
    pub rings: u8,
    /// Arcs narrower than this many radians are left out (their parent shows them).
    pub min_angle: f32,
}

impl Default for SunburstOptions {
    fn default() -> SunburstOptions {
        SunburstOptions {
            rings: 3,
            min_angle: 0.02,
        }
    }
}

/// Sunburst arcs for the children of `root`: ring 0 holds the children, ring 1 their children inside
/// the parent's angle, and so on. Largest first, ties by name, zero sizes dropped.
pub fn sunburst(tree: &Tree, root: NodeId, opts: &SunburstOptions) -> Vec<Arc> {
    let mut out = Vec::new();
    ring(tree, root, 0.0, std::f32::consts::TAU, 0, opts, &mut out);
    out
}

fn ring(
    tree: &Tree,
    parent: NodeId,
    start: f32,
    end: f32,
    ring_no: u8,
    opts: &SunburstOptions,
    out: &mut Vec<Arc>,
) {
    if ring_no >= opts.rings {
        return;
    }
    let items = sorted_children(tree, parent);
    let total: f64 = items.iter().map(|i| i.size as f64).sum();
    if total <= 0.0 {
        return;
    }
    let span = (end - start) as f64;
    let mut a = start as f64;
    for it in &items {
        let b = a + span * it.size as f64 / total;
        if (b - a) as f32 >= opts.min_angle {
            out.push(Arc {
                node: it.node,
                start: a as f32,
                end: b as f32,
                ring: ring_no,
            });
            if tree.kind(it.node) == Kind::Dir {
                ring(tree, it.node, a as f32, b as f32, ring_no + 1, opts, out);
            }
        }
        a = b;
    }
}

/// How long a drill animation runs.
pub const DRILL_DURATION: Duration = Duration::from_millis(280);

/// A calm cubic ease-out: fast start, long soft landing. `t` is clamped to 0..=1.
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// The state of a drill: the clicked tile grows from its own rectangle to fill the map.
#[derive(Clone, Copy, Debug)]
pub struct Drill {
    /// Where the animation starts (the tile that was clicked).
    pub from: Rect,
    /// Where it ends (the full map bounds).
    pub to: Rect,
    /// When it started.
    pub started: Instant,
    /// When true there is no animation: [`Drill::progress`] is 1 from the first frame.
    pub reduce_motion: bool,
}

impl Drill {
    /// Start a drill at `now`.
    pub fn new(from: Rect, to: Rect, now: Instant, reduce_motion: bool) -> Drill {
        Drill {
            from,
            to,
            started: now,
            reduce_motion,
        }
    }

    /// Eased progress at `now`, 0 at the start and exactly 1 at the end.
    pub fn progress(&self, now: Instant) -> f32 {
        if self.reduce_motion {
            return 1.0;
        }
        let elapsed = now.saturating_duration_since(self.started);
        ease_out_cubic(elapsed.as_secs_f32() / DRILL_DURATION.as_secs_f32())
    }

    /// The rectangle to draw the growing tile in at `now`.
    pub fn rect(&self, now: Instant) -> Rect {
        self.from.lerp(&self.to, self.progress(now))
    }

    /// True once the animation has reached its end.
    pub fn done(&self, now: Instant) -> bool {
        self.reduce_motion || now.saturating_duration_since(self.started) >= DRILL_DURATION
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Node;

    /// Build a tree from nested specs: (name, size_or_children).
    enum Spec {
        File(&'static str, u64),
        Dir(&'static str, Vec<Spec>),
    }
    use Spec::*;

    fn build(spec: &Spec) -> Tree {
        let mut tree = Tree::default();
        // Breadth-first so children are contiguous.
        struct Q<'a> {
            spec: &'a Spec,
            idx: u32,
        }
        let push = |tree: &mut Tree, name: &str, kind: Kind, parent: u32| -> u32 {
            let off = tree.names.len() as u32;
            tree.names.extend_from_slice(name.as_bytes());
            tree.nodes.push(Node {
                size: 0,
                items: if kind == Kind::File { 1 } else { 0 },
                name_off: off,
                parent,
                first_child: 0,
                child_count: 0,
                name_len: name.len() as u16,
                kind: kind as u8,
                flags: 0,
            });
            (tree.nodes.len() - 1) as u32
        };
        let root_name = match spec {
            File(n, _) | Dir(n, _) => n,
        };
        let r = push(&mut tree, root_name, Kind::Dir, 0);
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(Q { spec, idx: r });
        while let Some(Q { spec, idx }) = queue.pop_front() {
            if let Dir(_, kids) = spec {
                let first = tree.nodes.len() as u32;
                tree.nodes[idx as usize].first_child = first;
                tree.nodes[idx as usize].child_count = kids.len() as u32;
                for k in kids {
                    let (name, kind) = match k {
                        File(n, _) => (*n, Kind::File),
                        Dir(n, _) => (*n, Kind::Dir),
                    };
                    let ci = push(&mut tree, name, kind, idx);
                    if let File(_, sz) = k {
                        tree.nodes[ci as usize].size = *sz;
                    }
                    queue.push_back(Q { spec: k, idx: ci });
                }
            }
        }
        // Sum sizes bottom-up (children have higher indexes than parents).
        for i in (1..tree.nodes.len()).rev() {
            let p = tree.nodes[i].parent as usize;
            let (s, it) = (tree.nodes[i].size, tree.nodes[i].items);
            tree.nodes[p].size += s;
            tree.nodes[p].items += it;
        }
        tree
    }

    fn sample() -> Tree {
        build(&Dir(
            "/root",
            vec![
                Dir(
                    "a",
                    vec![
                        File("a1", 4000),
                        File("a2", 2500),
                        File("a3", 1500),
                        File("a4", 1000),
                    ],
                ),
                Dir("b", vec![File("b1", 3000), File("b2", 500)]),
                File("c", 2000),
                File("d", 1000),
                File("e", 700),
                File("zero", 0),
                File("f", 5),
                File("g", 4),
            ],
        ))
    }

    const BOUNDS: Rect = Rect::new(10.0, 20.0, 800.0, 500.0);

    fn flat(levels: u8) -> LayoutOptions {
        LayoutOptions {
            levels,
            gap: 0.0,
            min_area: 100.0,
            ..LayoutOptions::default()
        }
    }

    #[test]
    fn areas_sum_to_bounds_area() {
        let t = sample();
        let tiles = squarify(&t, NodeId::ROOT, BOUNDS, &flat(1));
        let sum: f64 = tiles.iter().map(|t| t.rect.area() as f64).sum();
        let want = BOUNDS.area() as f64;
        assert!((sum - want).abs() / want < 0.001, "sum {sum} want {want}");
    }

    #[test]
    fn tiles_do_not_overlap() {
        let t = sample();
        for opts in [flat(1), LayoutOptions::default()] {
            let tiles = squarify(&t, NodeId::ROOT, BOUNDS, &opts);
            let top: Vec<_> = tiles.iter().filter(|t| t.depth == 0).collect();
            for (i, a) in top.iter().enumerate() {
                for b in &top[i + 1..] {
                    let ox = (a.rect.right().min(b.rect.right()) - a.rect.x.max(b.rect.x)).max(0.0);
                    let oy =
                        (a.rect.bottom().min(b.rect.bottom()) - a.rect.y.max(b.rect.y)).max(0.0);
                    assert!(ox * oy < 0.01, "overlap {ox}x{oy}");
                }
            }
        }
    }

    #[test]
    fn tiles_stay_inside_bounds_and_nested_inside_parent() {
        let t = sample();
        let opts = LayoutOptions {
            nest_min_side: 10.0,
            ..LayoutOptions::default()
        };
        let tiles = squarify(&t, NodeId::ROOT, BOUNDS, &opts);
        assert!(tiles.iter().any(|t| t.depth == 1));
        for tile in &tiles {
            let r = tile.rect;
            assert!(r.x >= BOUNDS.x - 0.01 && r.right() <= BOUNDS.right() + 0.01);
            assert!(r.y >= BOUNDS.y - 0.01 && r.bottom() <= BOUNDS.bottom() + 0.01);
        }
        // Each depth 1 tile sits inside some depth 0 tile.
        for c in tiles.iter().filter(|t| t.depth == 1) {
            assert!(
                tiles.iter().filter(|t| t.depth == 0).any(|p| {
                    c.rect.x >= p.rect.x - 0.01
                        && c.rect.y >= p.rect.y - 0.01
                        && c.rect.right() <= p.rect.right() + 0.01
                        && c.rect.bottom() <= p.rect.bottom() + 0.01
                }),
                "child outside parent"
            );
        }
    }

    #[test]
    fn deterministic_and_largest_first() {
        let t = sample();
        let a = squarify(&t, NodeId::ROOT, BOUNDS, &flat(1));
        let b = squarify(&t, NodeId::ROOT, BOUNDS, &flat(1));
        assert_eq!(a, b);
        let sizes: Vec<u64> = a
            .iter()
            .filter(|t| !t.is_more())
            .map(|t| t.node)
            .map(|n| t.size(n))
            .collect();
        assert!(sizes.windows(2).all(|w| w[0] >= w[1]), "{sizes:?}");
    }

    #[test]
    fn ties_break_by_name() {
        let t = build(&Dir(
            "/r",
            vec![File("b", 100), File("a", 100), File("c", 100)],
        ));
        let tiles = squarify(&t, NodeId::ROOT, BOUNDS, &flat(1));
        let names: Vec<String> = tiles.iter().map(|x| t.name(x.node)).collect();
        assert_eq!(names, ["a", "b", "c"]);
    }

    #[test]
    fn zero_size_entries_are_dropped() {
        let t = sample();
        let tiles = squarify(&t, NodeId::ROOT, BOUNDS, &flat(1));
        assert!(
            tiles
                .iter()
                .all(|x| x.is_more() || t.size(x.node) > 0 && t.name(x.node) != "zero")
        );
    }

    #[test]
    fn small_tiles_merge_into_one_more_tile() {
        let t = sample();
        let opts = LayoutOptions {
            min_area: 1000.0,
            ..flat(1)
        };
        let tiles = squarify(&t, NodeId::ROOT, BOUNDS, &opts);
        let more: Vec<_> = tiles.iter().filter(|x| x.is_more()).collect();
        assert_eq!(more.len(), 1);
        let info = more[0].more.unwrap();
        // f (5 bytes) and g (4 bytes) get far less than 1000 px2 each.
        assert!(info.count >= 2);
        assert!(info.bytes >= 9);
        for x in tiles.iter().filter(|x| !x.is_more()) {
            assert!(x.rect.area() >= 1000.0 * 0.999);
        }
        // Still tiles the whole bounds.
        let sum: f64 = tiles.iter().map(|t| t.rect.area() as f64).sum();
        assert!((sum - BOUNDS.area() as f64).abs() / (BOUNDS.area() as f64) < 0.001);
    }

    #[test]
    fn empty_and_zero_trees_give_no_tiles() {
        let t = build(&Dir("/r", vec![File("z", 0)]));
        assert!(squarify(&t, NodeId::ROOT, BOUNDS, &flat(2)).is_empty());
        assert!(
            squarify(
                &sample(),
                NodeId::ROOT,
                Rect::new(0.0, 0.0, 0.0, 10.0),
                &flat(2)
            )
            .is_empty()
        );
    }

    #[test]
    fn hit_test_prefers_the_deepest_tile() {
        let t = sample();
        let opts = LayoutOptions {
            nest_min_side: 10.0,
            ..LayoutOptions::default()
        };
        let tiles = squarify(&t, NodeId::ROOT, BOUNDS, &opts);
        let inner = tiles.iter().position(|t| t.depth == 1).unwrap();
        let c = tiles[inner].rect;
        let hit = hit_test(&tiles, c.x + c.w / 2.0, c.y + c.h / 2.0).unwrap();
        assert_eq!(tiles[hit].depth, 1);
    }

    #[test]
    fn sunburst_rings_cover_the_circle_and_nest() {
        let t = sample();
        let arcs = sunburst(&t, NodeId::ROOT, &SunburstOptions::default());
        let ring0: Vec<_> = arcs.iter().filter(|a| a.ring == 0).collect();
        let covered: f32 = ring0.iter().map(|a| a.end - a.start).sum();
        // The tiny f and g arcs fall under min_angle, so a little is left uncovered.
        assert!(covered > std::f32::consts::TAU * 0.99);
        assert!(arcs.iter().any(|a| a.ring == 1));
        for a in arcs.iter().filter(|a| a.ring == 1) {
            let parent = t.parent(a.node).unwrap();
            let p = ring0.iter().find(|p| p.node == parent).unwrap();
            assert!(a.start >= p.start - 1e-4 && a.end <= p.end + 1e-4);
        }
        assert_eq!(
            arcs,
            sunburst(&t, NodeId::ROOT, &SunburstOptions::default())
        );
    }

    #[test]
    fn drill_eases_out_and_lands_exactly() {
        let from = Rect::new(10.0, 10.0, 100.0, 50.0);
        let to = Rect::new(0.0, 0.0, 800.0, 500.0);
        let t0 = Instant::now();
        let d = Drill::new(from, to, t0, false);
        assert_eq!(d.progress(t0), 0.0);
        let mid = d.progress(t0 + DRILL_DURATION / 2);
        assert!(mid > 0.5, "ease-out is past halfway at half time: {mid}");
        assert_eq!(d.progress(t0 + DRILL_DURATION), 1.0);
        assert_eq!(d.progress(t0 + DRILL_DURATION * 3), 1.0);
        assert!(!d.done(t0 + DRILL_DURATION / 2));
        assert!(d.done(t0 + DRILL_DURATION));
        assert_eq!(d.rect(t0 + DRILL_DURATION), to);
    }

    #[test]
    fn drill_with_reduce_motion_is_instant() {
        let from = Rect::new(10.0, 10.0, 100.0, 50.0);
        let to = Rect::new(0.0, 0.0, 800.0, 500.0);
        let t0 = Instant::now();
        let d = Drill::new(from, to, t0, true);
        assert_eq!(d.progress(t0), 1.0);
        assert!(d.done(t0));
        assert_eq!(d.rect(t0), to);
    }
}
