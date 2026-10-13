//! Data, filtering and list geometry. Pure logic, no GPUI types.

use crate::fuzzy::{self, IndexEntry};
use serde::Deserialize;
use std::path::PathBuf;

pub const WIDTH: f32 = 720.0;
pub const HEIGHT: f32 = 460.0;
pub const RADIUS: f32 = 20.0;
pub const SEARCH_ROW: f32 = 56.0;
pub const FOOTER: f32 = 40.0;
pub const LIST_TOP: f32 = 8.0;
pub const HEADER: f32 = 28.0;
pub const ROW: f32 = 44.0;
pub const VIEWPORT: f32 = HEIGHT - SEARCH_ROW - FOOTER;

#[derive(Debug, Clone, Deserialize)]
pub struct Tool {
    pub id: String,
    pub name: String,
    pub subtitle: String,
    pub category: String,
    #[serde(rename = "sfSymbol")]
    #[allow(dead_code)]
    pub sf_symbol: String,
    pub lucide: String,
    pub accepts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DroppedFile {
    pub name: String,
    pub ext: String,
}

impl DroppedFile {
    pub fn from_path(p: &std::path::Path) -> Self {
        let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut ext = p.extension().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        if ext == "jpeg" {
            ext = "jpg".into();
        }
        if p.is_dir() {
            ext.clear();
        }
        Self { name, ext }
    }
}

#[derive(Debug, Clone)]
pub struct Row {
    pub tool: usize,
    pub matched: Vec<usize>,
    pub ord: usize,
}

#[derive(Debug, Clone)]
pub enum Entry {
    Header { y: f32, title: String },
    Row { y: f32, row: usize },
}

#[derive(Debug, Default)]
pub struct ItemList {
    pub entries: Vec<Entry>,
    pub rows: Vec<Row>,
    pub row_ys: Vec<f32>,
    pub row_after_header: Vec<bool>,
    pub content_height: f32,
}

pub struct Model {
    pub tools: Vec<Tool>,
    pub categories: Vec<String>,
    pub placeholder: String,
    index: Vec<IndexEntry>,
}

fn locate() -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = vec![];
    if let Ok(e) = std::env::var("ARSENAL_DATA") {
        c.push(e.into());
    }
    if let Ok(exe) = std::env::current_exe() {
        let exe = exe.canonicalize().unwrap_or(exe);
        if let Some(contents) = exe.parent().and_then(|p| p.parent()) {
            c.push(contents.join("Resources/utilities.json"));
        }
        let mut d = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..8 {
            if let Some(dir) = d {
                c.push(dir.join("shared/utilities.json"));
                d = dir.parent().map(|p| p.to_path_buf());
            } else {
                break;
            }
        }
    }
    c.push("/Users/anzalhussainabidi/personal/panoply/prototypes/shell-test/shared/utilities.json".into());
    c.into_iter().find(|p| p.exists())
}

impl Model {
    pub fn load() -> Self {
        let tools: Vec<Tool> = locate()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self::new(tools)
    }

    pub fn new(tools: Vec<Tool>) -> Self {
        let mut categories: Vec<String> = vec![];
        for t in &tools {
            if !categories.contains(&t.category) {
                categories.push(t.category.clone());
            }
        }
        let index = tools
            .iter()
            .enumerate()
            .map(|(i, t)| IndexEntry {
                order: i,
                name_lower: fuzzy::lower_chars(&t.name),
                boundary: fuzzy::boundaries(&t.name),
                subtitle_lower: t.subtitle.to_lowercase(),
                category_lower: t.category.to_lowercase(),
            })
            .collect();
        let placeholder = format!("Search {} tools or drop a file", tools.len());
        Self { tools, categories, placeholder, index }
    }

    pub fn build(&self, query: &str, ext: Option<&str>) -> ItemList {
        let pool: Vec<usize> = (0..self.tools.len())
            .filter(|&i| match ext {
                None => true,
                Some(e) => self.tools[i].accepts.iter().any(|a| a == "*" || a == e),
            })
            .collect();

        let mut l = ItemList::default();
        let mut y = LIST_TOP;
        let qs: Vec<char> = fuzzy::lower_chars(query).into_iter().filter(|c| !c.is_whitespace()).collect();
        let qtrim = query.to_lowercase().trim().to_string();

        if qs.is_empty() {
            for cat in &self.categories {
                let group: Vec<usize> = pool.iter().copied().filter(|&i| &self.tools[i].category == cat).collect();
                if group.is_empty() {
                    continue;
                }
                l.entries.push(Entry::Header { y, title: cat.clone() });
                y += HEADER;
                for (k, &ti) in group.iter().enumerate() {
                    let ord = l.rows.len();
                    l.entries.push(Entry::Row { y, row: ord });
                    l.rows.push(Row { tool: ti, matched: vec![], ord });
                    l.row_ys.push(y);
                    l.row_after_header.push(k == 0);
                    y += ROW;
                }
            }
        } else {
            struct Hit {
                tool: usize,
                score: i32,
                idx: Vec<usize>,
            }
            let mut hits: Vec<Hit> = vec![];
            for &ti in &pool {
                let e = &self.index[ti];
                let nlen = e.name_lower.len() as i32;
                if let Some((score, idx)) = fuzzy::matches(&qs, e) {
                    hits.push(Hit { tool: ti, score: score * 4 - nlen, idx });
                } else if !qtrim.is_empty() {
                    if e.subtitle_lower.contains(&qtrim) {
                        hits.push(Hit { tool: ti, score: 10 - nlen, idx: vec![] });
                    } else if e.category_lower.starts_with(&qtrim) {
                        hits.push(Hit { tool: ti, score: 4 - nlen, idx: vec![] });
                    }
                }
            }
            hits.sort_by(|a, b| b.score.cmp(&a.score).then(self.index[a.tool].order.cmp(&self.index[b.tool].order)));
            if !hits.is_empty() {
                l.entries.push(Entry::Header { y, title: "Best matches".into() });
                y += HEADER;
            }
            for (k, h) in hits.into_iter().enumerate() {
                let ord = l.rows.len();
                l.entries.push(Entry::Row { y, row: ord });
                l.rows.push(Row { tool: h.tool, matched: h.idx, ord });
                l.row_ys.push(y);
                l.row_after_header.push(k == 0);
                y += ROW;
            }
        }
        l.content_height = y + LIST_TOP;
        l
    }
}

pub fn tint(category: &str, dark: bool) -> (u8, u8, u8) {
    // macOS system colours, light and dark variants.
    match (category, dark) {
        ("PDF", false) => (255, 59, 48),
        ("PDF", true) => (255, 69, 58),
        ("Image", false) => (175, 82, 222),
        ("Image", true) => (191, 90, 242),
        ("Video", false) => (255, 149, 0),
        ("Video", true) => (255, 159, 10),
        ("Audio", false) => (255, 45, 85),
        ("Audio", true) => (255, 55, 95),
        ("Files", false) => (0, 122, 255),
        ("Files", true) => (10, 132, 255),
        ("Text", false) => (52, 199, 89),
        ("Text", true) => (48, 209, 88),
        ("Capture", false) => (48, 176, 199),
        ("Capture", true) => (64, 200, 224),
        (_, false) => (142, 142, 147),
        (_, true) => (152, 152, 157),
    }
}
