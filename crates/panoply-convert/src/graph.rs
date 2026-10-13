use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const GRAPH_JSON: &str = include_str!("../data/graph.json");

/// Longest chain of plain steps the planner will use.
pub const MAX_HOPS: usize = 4;

/// Order used to pick a format when an extension is ambiguous. Media first, then documents,
/// then the rest. Families not listed rank after these, in alphabetical order.
pub const FAMILY_PRIORITY: &[&str] = &[
    "Image",
    "Video",
    "Audio",
    "Document",
    "PDF",
    "Spreadsheet",
    "Presentation",
    "Ebook",
    "Data",
    "Archive",
    "Vector",
    "Subtitles",
    "Font",
];

/// Preferred middle steps when several routes are the same length, best first.
pub const HUBS: &[&str] = &[
    "PDF:pdf",
    "Image:png",
    "Audio:wav",
    "Video:mp4",
    "Data:json",
    "Spreadsheet:csv",
];

/// One file format in the graph.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Format {
    /// Unique id, `Family:ext`.
    pub id: String,
    /// Family such as `Image` or `Audio`.
    pub family: String,
    /// Extension without the dot.
    pub ext: String,
    /// Display name.
    pub name: String,
    /// Can be read (a source).
    pub read: bool,
    /// Can be written (a target).
    pub write: bool,
}

/// One conversion step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    /// Source format.
    pub from: Format,
    /// Target format.
    pub to: Format,
    /// True when this step runs a local model (transcribe, OCR, read aloud).
    pub model: bool,
}

/// A planned conversion: one or more steps, end to end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Route {
    /// The steps in order. Never empty.
    pub steps: Vec<Step>,
}

impl Route {
    /// Number of steps.
    pub fn hops(&self) -> usize {
        self.steps.len()
    }

    /// True when the route is a single model step.
    pub fn is_model(&self) -> bool {
        self.steps.iter().any(|s| s.model)
    }

    /// Extensions along the route, source first.
    pub fn exts(&self) -> Vec<&str> {
        let mut v = vec![self.steps[0].from.ext.as_str()];
        v.extend(self.steps.iter().map(|s| s.to.ext.as_str()));
        v
    }

    /// The chain as text, for example `docx -> pdf -> jpg`.
    pub fn chain(&self) -> String {
        self.exts().join(" -> ")
    }
}

/// Problems loading the embedded graph.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    /// The embedded JSON could not be parsed.
    #[error("the embedded conversion graph is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// An edge or bridge names a format that does not exist.
    #[error("the conversion graph mentions an unknown format: {0}")]
    UnknownFormat(String),
}

#[derive(Deserialize)]
struct RawBridge {
    from: String,
    to: String,
    #[serde(default)]
    model: bool,
}

#[derive(Deserialize)]
struct RawGraph {
    formats: Vec<Format>,
    edges: Vec<[String; 2]>,
    model_edges: Vec<[String; 2]>,
    bridges: Vec<RawBridge>,
    composite_families: Vec<[String; 2]>,
}

/// The loaded conversion graph.
#[derive(Debug)]
pub struct Graph {
    formats: Vec<Format>,
    by_id: HashMap<String, usize>,
    by_ext: HashMap<String, Vec<usize>>,
    fwd: Vec<Vec<usize>>,
    rev: Vec<Vec<usize>>,
    model: HashSet<(usize, usize)>,
    allowed: HashSet<(String, String)>,
}

fn family_of(spec: &str) -> &str {
    spec.split(':').next().unwrap_or(spec)
}

fn normalize_ext(ext: &str) -> String {
    let e = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    match e.as_str() {
        "jpeg" | "jpe" => "jpg".into(),
        "tif" => "tiff".into(),
        "htm" => "html".into(),
        "yml" => "yaml".into(),
        "markdown" => "md".into(),
        "midi" => "mid".into(),
        "mpeg" => "mpg".into(),
        _ => e,
    }
}

fn family_rank(family: &str) -> (usize, String) {
    let i = FAMILY_PRIORITY
        .iter()
        .position(|f| *f == family)
        .unwrap_or(FAMILY_PRIORITY.len());
    (i, family.to_string())
}

impl Graph {
    /// Parse the embedded graph. Costs about a millisecond.
    pub fn load() -> Result<Graph, GraphError> {
        let raw: RawGraph = serde_json::from_str(GRAPH_JSON)?;
        let mut by_id = HashMap::with_capacity(raw.formats.len());
        let mut by_ext: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, f) in raw.formats.iter().enumerate() {
            by_id.insert(f.id.clone(), i);
            by_ext.entry(f.ext.clone()).or_default().push(i);
        }
        for list in by_ext.values_mut() {
            list.sort_by_key(|&i| family_rank(&raw.formats[i].family));
        }
        let look = |id: &str| {
            by_id
                .get(id)
                .copied()
                .ok_or_else(|| GraphError::UnknownFormat(id.into()))
        };
        let n = raw.formats.len();
        let mut fwd = vec![Vec::new(); n];
        let mut rev = vec![Vec::new(); n];
        for [a, b] in &raw.edges {
            let (a, b) = (look(a)?, look(b)?);
            fwd[a].push(b);
            rev[b].push(a);
        }
        let mut model = HashSet::with_capacity(raw.model_edges.len());
        for [a, b] in &raw.model_edges {
            model.insert((look(a)?, look(b)?));
        }
        let mut allowed: HashSet<(String, String)> = raw
            .formats
            .iter()
            .map(|f| (f.family.clone(), f.family.clone()))
            .collect();
        for [a, b] in raw.composite_families {
            allowed.insert((a, b));
        }
        for br in &raw.bridges {
            if !br.model {
                allowed.insert((
                    family_of(&br.from).to_string(),
                    family_of(&br.to).to_string(),
                ));
            }
        }
        Ok(Graph {
            formats: raw.formats,
            by_id,
            by_ext,
            fwd,
            rev,
            model,
            allowed,
        })
    }

    /// The graph, loaded once per process.
    pub fn shared() -> &'static Graph {
        static G: OnceLock<Graph> = OnceLock::new();
        G.get_or_init(|| Graph::load().expect("embedded conversion graph is valid"))
    }

    /// Every format, in generator order.
    pub fn formats(&self) -> &[Format] {
        &self.formats
    }

    /// Family names in first-seen order.
    pub fn families(&self) -> Vec<&str> {
        let mut seen = Vec::new();
        for f in &self.formats {
            if !seen.contains(&f.family.as_str()) {
                seen.push(f.family.as_str());
            }
        }
        seen
    }

    /// Look a format up by its exact id such as `Image:jpg`.
    pub fn format_by_id(&self, id: &str) -> Option<&Format> {
        self.by_id.get(id).map(|&i| &self.formats[i])
    }

    /// Every format with this extension, best first by [`FAMILY_PRIORITY`]. Accepts a leading
    /// dot, any case, and common aliases (`jpeg`, `tif`, `htm`, `yml`).
    pub fn formats_by_ext(&self, ext: &str) -> Vec<&Format> {
        self.by_ext
            .get(&normalize_ext(ext))
            .map(|v| v.iter().map(|&i| &self.formats[i]).collect())
            .unwrap_or_default()
    }

    /// The format for an extension. When several share it, the highest-priority family wins.
    pub fn format_by_ext(&self, ext: &str) -> Option<&Format> {
        self.formats_by_ext(ext).into_iter().next()
    }

    /// Plan the shortest route between two extensions. See the crate docs for the rules.
    pub fn route(&self, from_ext: &str, to_ext: &str) -> Option<Route> {
        let (from, to) = (normalize_ext(from_ext), normalize_ext(to_ext));
        let srcs = self.by_ext.get(&from)?;
        let dsts = self.by_ext.get(&to)?;
        let mut best: Option<Route> = None;
        for &s in srcs {
            for &d in dsts {
                if let Some(r) = self.route_idx(s, d)
                    && best.as_ref().is_none_or(|b| r.hops() < b.hops())
                {
                    best = Some(r);
                }
            }
        }
        best
    }

    /// Plan a route between two exact format ids.
    pub fn route_ids(&self, from_id: &str, to_id: &str) -> Option<Route> {
        self.route_idx(*self.by_id.get(from_id)?, *self.by_id.get(to_id)?)
    }

    fn route_idx(&self, src: usize, dst: usize) -> Option<Route> {
        if src == dst || !self.formats[src].read || !self.formats[dst].write {
            return None;
        }
        let key = (
            self.formats[src].family.clone(),
            self.formats[dst].family.clone(),
        );
        if self.allowed.contains(&key)
            && let Some(path) = self.shortest_plain(src, dst)
        {
            return Some(self.to_route(&path, false));
        }
        if self.model.contains(&(src, dst)) {
            return Some(self.to_route(&[src, dst], true));
        }
        None
    }

    /// Breadth-first distances from `src`, then walk back from `dst` choosing hub formats
    /// among equally short predecessors.
    fn shortest_plain(&self, src: usize, dst: usize) -> Option<Vec<usize>> {
        let n = self.formats.len();
        let mut dist = vec![usize::MAX; n];
        dist[src] = 0;
        let mut q = VecDeque::from([src]);
        while let Some(u) = q.pop_front() {
            if dist[u] >= MAX_HOPS {
                continue;
            }
            for &v in &self.fwd[u] {
                if dist[v] == usize::MAX {
                    dist[v] = dist[u] + 1;
                    q.push_back(v);
                }
            }
        }
        if dist[dst] == usize::MAX {
            return None;
        }
        let mut path = vec![dst];
        let mut cur = dst;
        while cur != src {
            let want = dist[cur] - 1;
            let pred = self.rev[cur]
                .iter()
                .copied()
                .filter(|&p| dist[p] == want)
                .min_by_key(|&p| self.hub_rank(p, src))?;
            path.push(pred);
            cur = pred;
        }
        path.reverse();
        Some(path)
    }

    fn hub_rank(&self, idx: usize, src: usize) -> usize {
        if idx == src {
            return 0;
        }
        let id = self.formats[idx].id.as_str();
        HUBS.iter().position(|h| *h == id).unwrap_or(HUBS.len())
    }

    fn to_route(&self, path: &[usize], model: bool) -> Route {
        let steps = path
            .windows(2)
            .map(|w| Step {
                from: self.formats[w[0]].clone(),
                to: self.formats[w[1]].clone(),
                model,
            })
            .collect();
        Route { steps }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g() -> Graph {
        Graph::load().unwrap()
    }

    #[test]
    fn has_214_formats() {
        assert_eq!(g().formats().len(), 214);
    }

    #[test]
    fn heic_to_jpg_is_one_step() {
        let r = g().route("heic", "jpg").unwrap();
        assert_eq!(r.hops(), 1);
        assert_eq!(r.chain(), "heic -> jpg");
        assert!(!r.is_model());
    }

    #[test]
    fn docx_to_jpg_goes_through_pdf() {
        let r = g().route("docx", "jpg").unwrap();
        assert_eq!(r.chain(), "docx -> pdf -> jpg");
    }

    #[test]
    fn mkv_to_mp3_is_one_step() {
        let r = g().route("mkv", "mp3").unwrap();
        assert_eq!(r.hops(), 1);
    }

    #[test]
    fn mp3_to_srt_only_via_model_edge() {
        let r = g().route("mp3", "srt").unwrap();
        assert_eq!(r.hops(), 1);
        assert!(r.is_model());
    }

    #[test]
    fn model_steps_are_never_chained() {
        let g = g();
        // No plain route exists from audio to a subtitle file, so a chain through the
        // transcribe step (mp3 -> txt -> something) must not appear either.
        for r in ["srt", "vtt", "ass", "sbv"]
            .iter()
            .filter_map(|t| g.route("mp3", t))
        {
            assert!(r.hops() == 1 && r.is_model());
        }
    }

    #[test]
    fn stl_to_mp3_is_gated_out() {
        assert!(g().route("stl", "mp3").is_none());
    }

    #[test]
    fn gate_blocks_unrelated_families_everywhere() {
        let g = g();
        for (a, b) in [
            ("stl", "mp3"),
            ("ttf", "mp3"),
            ("zip", "mp3"),
            ("sqlite", "mp4"),
        ] {
            assert!(g.route(a, b).is_none(), "{a} -> {b} should not route");
        }
    }

    #[test]
    fn every_plain_route_respects_the_hop_cap() {
        let g = g();
        for a in ["docx", "heic", "mkv", "xlsx", "epub", "svg", "pptx"] {
            for f in g.formats() {
                if let Some(r) = g.route(a, &f.ext) {
                    assert!(r.hops() <= MAX_HOPS, "{a} -> {}", f.ext);
                    if r.is_model() {
                        assert_eq!(r.hops(), 1);
                    }
                }
            }
        }
    }

    #[test]
    fn routes_are_connected_chains() {
        let g = g();
        let r = g.route("pptx", "jpg").unwrap();
        for w in r.steps.windows(2) {
            assert_eq!(w[0].to, w[1].from);
        }
    }

    #[test]
    fn same_format_has_no_route() {
        assert!(g().route("jpg", "jpg").is_none());
    }

    #[test]
    fn unknown_extension_has_no_route() {
        assert!(g().route("xyzzy", "jpg").is_none());
    }

    #[test]
    fn aliases_and_dots_are_accepted() {
        let g = g();
        assert_eq!(g.format_by_ext(".JPEG").unwrap().id, "Image:jpg");
        assert_eq!(g.format_by_ext("tif").unwrap().id, "Image:tiff");
        assert!(g.route(".HEIC", "jpeg").is_some());
    }

    #[test]
    fn every_extension_resolves_to_its_own_format() {
        let g = g();
        for f in g.formats() {
            let got = g.formats_by_ext(&f.ext);
            assert!(got.iter().any(|c| c.id == f.id), "{}", f.id);
        }
    }

    #[test]
    fn ambiguity_picks_by_family_priority() {
        // Build a tiny synthetic ordering check with the real ranking function.
        let mut fams = ["Subtitles", "Audio", "Image", "Zebra", "Video"];
        fams.sort_by_key(|f| family_rank(f));
        assert_eq!(fams, ["Image", "Video", "Audio", "Subtitles", "Zebra"]);
    }

    #[test]
    fn shared_graph_is_a_singleton() {
        assert!(std::ptr::eq(Graph::shared(), Graph::shared()));
    }

    #[test]
    fn families_are_listed() {
        let g = g();
        let fams = g.families();
        assert_eq!(fams.len(), 20);
        assert!(fams.contains(&"Audio"));
    }
}
