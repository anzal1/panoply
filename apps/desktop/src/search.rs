//! The command bar's brain: what rows to show for a query and the files on the bar.
//!
//! Real registry in, rows out. Fuzzy search over tool names, plain-words requests ("make this
//! under 50 kb"), and the conversion router for "x to y" queries all end up as [`Row`]s that the
//! bar draws and that open a tool through [`Action`].

use panoply_convert::Graph;
use panoply_manifest::Pattern;
use serde_json::Value;

use crate::catalog::{Catalog, Category};
use crate::form::Form;
use crate::fuzzy::{self, IndexEntry};
use crate::history::Entry;
use crate::intent::{self, Intent};
use crate::profile::{FileProfile, files_noun, tools_for_files};
use crate::units::relative_time;

/// What choosing a row does.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Open a tool, with these options already set.
    OpenTool {
        /// Index into the catalog.
        tool: usize,
        /// Option values the request filled in.
        preset: Vec<(String, Value)>,
    },
    /// Open the tool of a history entry again, on the same files when they still exist.
    ReRun {
        /// Index into the history entries given to [`search`].
        entry: usize,
    },
    /// Open a file from the index with its default app (Command+Return shows it in the file
    /// manager instead).
    OpenFile {
        /// The file.
        path: std::path::PathBuf,
    },
    /// Copy this text to the clipboard (the calculator's answer).
    CopyText {
        /// What goes on the clipboard.
        text: String,
    },
    /// The row cannot run. The text says why.
    Unavailable(String),
}

/// One row of the bar.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// What Return does.
    pub action: Action,
    /// Title.
    pub title: String,
    /// One line under or beside the title.
    pub subtitle: String,
    /// Small text at the right when the row is not selected.
    pub trailing: String,
    /// Icon name.
    pub icon: &'static str,
    /// Category, for the tint.
    pub category: Category,
    /// Indices of title characters the query matched.
    pub matched: Vec<usize>,
    /// Option chips shown under the best match: `(label, value)`.
    pub chips: Vec<(String, String)>,
    /// The estimate shown at the right of the chips.
    pub estimate: Option<String>,
}

/// A titled group of rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// "Suggested", "Best match", "For this HEIC photo".
    pub title: String,
    /// Rows, in order.
    pub rows: Vec<Row>,
}

/// Everything the bar shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Results {
    /// Sections in order.
    pub sections: Vec<Section>,
    /// What the footer says on the left when nothing else does.
    pub footer: Option<String>,
    /// "Understood “under 50 kb” as a target size".
    pub understood: Option<String>,
}

impl Results {
    /// Number of rows across all sections.
    pub fn len(&self) -> usize {
        self.sections.iter().map(|s| s.rows.len()).sum()
    }

    /// True when there are no rows.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The row at a flat index.
    pub fn get(&self, mut i: usize) -> Option<&Row> {
        for s in &self.sections {
            if i < s.rows.len() {
                return s.rows.get(i);
            }
            i -= s.rows.len();
        }
        None
    }

    /// All rows in order.
    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.sections.iter().flat_map(|s| s.rows.iter())
    }
}

/// What a search needs to know.
pub struct Context<'a> {
    /// The tools.
    pub catalog: &'a Catalog,
    /// Files on the bar.
    pub files: &'a [FileProfile],
    /// Recent job history, newest first.
    pub history: &'a [Entry],
    /// Now, seconds since the epoch.
    pub now: u64,
}

/// A search that remembers what it showed a moment ago, for a person typing. Typing "under 50 kb"
/// passes through "u", "under" and "under 5", which match nothing by themselves; the bar must not
/// flash "No tools match" in between, so while the query is plainly unfinished the last answer
/// stays on screen. A query that is not a way to finish (a typo, "under 50 kbx") gets the empty
/// state at once.
#[derive(Default)]
pub struct Typing {
    files: Vec<std::path::PathBuf>,
    last: Results,
}

impl Typing {
    /// Rows for the query as typed so far.
    pub fn search(&mut self, cx: &Context, query: &str) -> Results {
        let files: Vec<std::path::PathBuf> = cx.files.iter().map(|f| f.path.clone()).collect();
        if files != self.files {
            self.files = files;
            self.last = Results::default();
        }
        let r = search(cx, query);
        let q = query.trim();
        if r.is_empty() && !q.is_empty() && !self.last.is_empty() && intent::is_unfinished(q) {
            return self.last.clone();
        }
        self.last = r.clone();
        r
    }
}

fn tool_row(cx: &Context, tool: usize, subtitle: String) -> Row {
    let t = cx.catalog.tool(tool);
    Row {
        action: Action::OpenTool {
            tool,
            preset: Vec::new(),
        },
        title: t.name.clone(),
        subtitle,
        trailing: t.category.short().to_string(),
        icon: t.category.icon(),
        category: t.category,
        matched: Vec::new(),
        chips: Vec::new(),
        estimate: None,
    }
}

/// Rows for the bar. `query` is what is typed.
pub fn search(cx: &Context, query: &str) -> Results {
    let q = query.trim();
    if q.is_empty() {
        return if cx.files.is_empty() {
            at_rest(cx)
        } else {
            for_files(cx)
        };
    }
    let mut results = from_query(cx, q);
    if let Some(a) = crate::calc::answer(q) {
        results.sections.insert(
            0,
            Section {
                title: "Answer".into(),
                rows: vec![calc_row(a)],
            },
        );
        results.footer = Some("Return copies the answer".into());
    }
    results
}

/// The calculator's answer as the top row of the bar.
fn calc_row(a: crate::calc::Answer) -> Row {
    Row {
        action: Action::CopyText { text: a.copy },
        title: a.shown,
        subtitle: a.asked,
        trailing: "Copy".into(),
        icon: "calculator",
        category: Category::Calculators,
        matched: Vec::new(),
        chips: Vec::new(),
        estimate: None,
    }
}

/// The bar at rest: suggested tools, then recent jobs.
fn at_rest(cx: &Context) -> Results {
    let recent_ids: Vec<String> = {
        let mut v: Vec<String> = Vec::new();
        for e in cx.history {
            if !v.contains(&e.tool_id) {
                v.push(e.tool_id.clone());
            }
        }
        v
    };
    let mut picks: Vec<usize> = recent_ids
        .iter()
        .filter_map(|id| cx.catalog.index_of(id))
        .filter(|&i| cx.catalog.tool(i).on_bar())
        .take(2)
        .collect();
    for cat in Category::ALL {
        if picks.len() >= 4 {
            break;
        }
        let first = (0..cx.catalog.tools().len()).find(|&i| {
            let t = cx.catalog.tool(i);
            t.category == cat
                && t.on_bar()
                && t.takes_files()
                && matches!(
                    t.pattern,
                    Pattern::InstantTransform | Pattern::TunedTransform
                )
                && !picks.contains(&i)
        });
        if let Some(i) = first {
            picks.push(i);
        }
    }
    let mut sections = vec![Section {
        title: "Suggested".into(),
        rows: picks
            .into_iter()
            .map(|i| tool_row(cx, i, cx.catalog.tool(i).summary.clone()))
            .collect(),
    }];
    let recent: Vec<Row> = cx
        .history
        .iter()
        .enumerate()
        .take(3)
        .filter_map(|(n, e)| {
            let idx = cx.catalog.index_of(&e.tool_id)?;
            let t = cx.catalog.tool(idx);
            let subtitle = match e.inputs.as_slice() {
                [one] => one
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                many => format!("{} files", many.len()),
            };
            Some(Row {
                action: Action::ReRun { entry: n },
                title: e.tool_name.clone(),
                subtitle,
                trailing: relative_time(cx.now, e.at),
                icon: t.category.icon(),
                category: t.category,
                matched: Vec::new(),
                chips: Vec::new(),
                estimate: None,
            })
        })
        .collect();
    if !recent.is_empty() {
        sections.push(Section {
            title: "Recent".into(),
            rows: recent,
        });
    }
    Results {
        sections,
        footer: Some("Everything runs on this computer".into()),
        understood: None,
    }
}

/// Files on the bar, no query: every tool that takes them, best first.
fn for_files(cx: &Context) -> Results {
    let recent: Vec<String> = cx.history.iter().map(|e| e.tool_id.clone()).collect();
    let matches = tools_for_files(cx.catalog, cx.files, &recent);
    let noun = files_noun(cx.files);
    let rows: Vec<Row> = matches
        .iter()
        .map(|m| tool_row(cx, m.tool, cx.catalog.tool(m.tool).summary.clone()))
        .collect();
    Results {
        footer: Some(format!(
            "{} of {} tools fit {} {noun}",
            rows.len(),
            cx.catalog.file_tool_count(),
            article(&noun)
        )),
        sections: vec![Section {
            title: format!("For this {noun} \u{00B7} best first"),
            rows,
        }],
        understood: None,
    }
}

fn article(noun: &str) -> &'static str {
    match noun.chars().next() {
        Some(c) if "AEIOUaeiou".contains(c) && !noun.starts_with("US") => "an",
        _ => "a",
    }
}

/// Words of a tool that a query word can match: name, summary, category.
fn word_hit(cx: &Context, tool: usize, word: &str) -> bool {
    let t = cx.catalog.tool(tool);
    // The services a preset names (WhatsApp, Discord, Gmail), so "whatsapp" finds Compress video.
    let presets = t
        .manifest
        .options
        .get("preset")
        .map(|o| o.values.join(" ").replace('_', " "))
        .unwrap_or_default();
    let hay = format!(
        "{} {} {} {} {}",
        t.name.to_lowercase(),
        t.summary.to_lowercase(),
        t.category.label().to_lowercase(),
        t.id.replace(['.', '_'], " "),
        presets
    );
    // "photo" and "picture" are searched as "image" (see `intent`), so a tool that says photo
    // must still answer to it.
    let alts: &[&str] = if word == "image" {
        &["image", "photo", "photos", "picture", "pictures"]
    } else {
        std::slice::from_ref(&word)
    };
    hay.split(|c: char| !c.is_alphanumeric()).any(|w| {
        alts.iter()
            .any(|a| w == *a || (a.len() >= 3 && w.starts_with(a)))
    })
}

fn from_query(cx: &Context, q: &str) -> Results {
    let intent = intent::parse(q);
    let q_chars: Vec<char> = fuzzy::lower_chars(q)
        .into_iter()
        .filter(|c| !c.is_whitespace())
        .collect();
    let recent: Vec<String> = cx.history.iter().map(|e| e.tool_id.clone()).collect();

    // The pool: with files, only tools that take them (ranked); without, every tool on the bar.
    let ranked = tools_for_files(cx.catalog, cx.files, &recent);
    let pool: Vec<(usize, i32)> = if cx.files.is_empty() {
        (0..cx.catalog.tools().len())
            .filter(|&i| cx.catalog.tool(i).on_bar())
            .map(|i| (i, 0))
            .collect()
    } else {
        ranked.iter().map(|m| (m.tool, m.score / 100)).collect()
    };

    let mut best_rows: Vec<Row> = Vec::new();
    let mut understood: Option<String> = None;

    // A conversion: "heic to jpg", or "to jpg" / "jpg" with a file on the bar.
    let convert = intent.convert.clone().or_else(|| {
        let word = match intent.words.as_slice() {
            [w] if !cx.files.is_empty() => w.as_str(),
            _ => return None,
        };
        Graph::shared().format_by_ext(word)?;
        Some(intent::Convert {
            from: None,
            to: word.to_string(),
        })
    });
    if let Some(c) = &convert {
        let from = c
            .from
            .clone()
            .or_else(|| cx.files.first().map(|f| f.sniffed.ext.clone()));
        if let Some(row) = convert_row(cx, from.as_deref(), &c.to) {
            best_rows.push(row);
        }
    }

    // Tool rows by score.
    let mut scored: Vec<(i32, usize, Vec<usize>)> = Vec::new();
    let mut size_boosted = false;
    for &(tool, rank) in &pool {
        let t = cx.catalog.tool(tool);
        let entry = IndexEntry::new(&t.name);
        let mut score = 0;
        let mut matched = Vec::new();
        if !q_chars.is_empty()
            && let Some((s, idx)) = fuzzy::matches(&q_chars, &entry)
        {
            score = s * 4 - entry.name_lower.len() as i32;
            matched = idx;
        }
        if score == 0 && !intent.words.is_empty() {
            let hits = intent
                .words
                .iter()
                .filter(|w| word_hit(cx, tool, w))
                .count();
            if hits == intent.words.len() {
                score = 20 + hits as i32 * 6;
            }
        }
        if intent.size.is_some() && crate::estimate::bound_option(&t.manifest).is_some() {
            // A size request is for the tools that can hit a size, and it also matches with
            // no other words ("make this under 50 kb").
            if intent.words.is_empty() || score > 0 {
                score += 60;
                size_boosted = true;
            }
        }
        if score > 0 {
            scored.push((score * 10 + rank.min(9), tool, matched));
        }
    }
    // Nothing matched every word. People type "passport photo india", "resize to 800px" or
    // "loudness -16": a country, a size or a level next to the name of what they want. Match on the
    // words that name something (not numbers, not "to"), when more than half of them are found.
    if scored.is_empty() && best_rows.is_empty() {
        let content: Vec<&String> = intent
            .words
            .iter()
            .filter(|w| w.chars().count() >= 3 && !w.chars().any(|c| c.is_ascii_digit()))
            .collect();
        if !content.is_empty() {
            for &(tool, rank) in &pool {
                let hits = content.iter().filter(|w| word_hit(cx, tool, w)).count();
                if hits > 0 && hits * 2 > content.len() {
                    scored.push(((10 + hits as i32 * 6) * 10 + rank.min(9), tool, Vec::new()));
                }
            }
        }
    }
    if q_chars.len() < 3 && intent.size.is_none() {
        // One or two letters match many names, and the best of them changes with every keystroke.
        // Keep the catalog's own order, so each letter only removes rows and never reorders them.
        scored.sort_by_key(|s| s.1);
    } else {
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| cx.catalog.tool(a.1).name.cmp(&cx.catalog.tool(b.1).name))
        });
    }

    let mut tool_rows: Vec<Row> = Vec::new();
    for (n, (_, tool, matched)) in scored.iter().enumerate() {
        let t = cx.catalog.tool(*tool);
        let mut row = tool_row(cx, *tool, t.summary.clone());
        row.matched = matched.clone();
        if n == 0
            && size_boosted
            && let Some(size) = &intent.size
            && let Some(filled) = fill_size(cx, *tool, size.bytes)
        {
            row.action = Action::OpenTool {
                tool: *tool,
                preset: filled.preset,
            };
            row.chips = filled.chips;
            row.estimate = filled.estimate;
            understood = Some(format!(
                "Understood \u{201C}{}\u{201D} as a target size",
                size.phrase
            ));
        }
        tool_rows.push(row);
    }

    // Sections. The first row is the "Best match" when it carries chips or is a conversion.
    let mut sections = Vec::new();
    let lead_is_special =
        !best_rows.is_empty() || tool_rows.first().is_some_and(|r| !r.chips.is_empty());
    if lead_is_special {
        let lead = if best_rows.is_empty() {
            vec![tool_rows.remove(0)]
        } else {
            best_rows
        };
        sections.push(Section {
            title: "Best match".into(),
            rows: lead,
        });
        if !tool_rows.is_empty() {
            sections.push(Section {
                title: "Also matches".into(),
                rows: tool_rows,
            });
        }
    } else if !tool_rows.is_empty() {
        sections.push(Section {
            title: "Best matches".into(),
            rows: tool_rows,
        });
    }
    let footer = if sections.is_empty() {
        None
    } else if !cx.files.is_empty() {
        Some(format!(
            "{} of {} tools fit {} {}",
            sections.iter().map(|s| s.rows.len()).sum::<usize>(),
            cx.catalog.file_tool_count(),
            article(&files_noun(cx.files)),
            files_noun(cx.files)
        ))
    } else {
        None
    };
    Results {
        sections,
        footer,
        understood,
    }
}

struct Filled {
    preset: Vec<(String, Value)>,
    chips: Vec<(String, String)>,
    estimate: Option<String>,
}

/// Fill a tool's size target from a request, the way the bar shows it as chips.
fn fill_size(cx: &Context, tool: usize, bytes: u64) -> Option<Filled> {
    let t = cx.catalog.tool(tool);
    let (key, value) = crate::estimate::bound_preset(&t.manifest, bytes)?;
    let mut form = Form::from_manifest(&t.manifest);
    form.set_value(&key, value.clone());
    let mut preset = vec![(key, value)];
    // A "target" style choice that has an under-size mode must be switched to it.
    for f in form.fields().to_vec() {
        if let crate::form::Control::Choice(values) = &f.control
            && let Some(v) = values
                .iter()
                .find(|v| v.contains("under") || v.contains("size"))
            && !form.is_set(&f.key)
        {
            form.set_value(&f.key, Value::String(v.clone()));
            preset.push((f.key.clone(), Value::String(v.clone())));
        }
    }
    let input_bytes: u64 = cx.files.iter().map(|f| f.bytes).sum();
    let estimate = (!cx.files.is_empty())
        .then(|| {
            form.to_options()
                .ok()
                .and_then(|o| crate::estimate::for_job(&t.manifest, input_bytes, &o))
        })
        .flatten()
        .map(|e| e.text());
    Some(Filled {
        chips: form.chips_with_defaults(3),
        preset,
        estimate,
    })
}

/// A conversion row for `from` to `to`, found through the router, run by whichever module takes
/// the source and produces the target.
fn convert_row(cx: &Context, from: Option<&str>, to: &str) -> Option<Row> {
    let graph = Graph::shared();
    let to_fmt = graph.format_by_ext(to)?;
    let to_ext = to_fmt.ext.clone();
    let from_ext = from.map(|f| {
        graph
            .format_by_ext(f)
            .map_or(f.to_string(), |x| x.ext.clone())
    });
    let title = match &from_ext {
        Some(f) => format!("{} to {}", f.to_uppercase(), to_ext.to_uppercase()),
        None => format!("Convert to {}", to_ext.to_uppercase()),
    };
    if from_ext.as_deref() == Some(to_ext.as_str()) {
        return None;
    }
    let route = from_ext.as_deref().and_then(|f| graph.route(f, &to_ext));
    if from_ext.is_some() && route.is_none() {
        return None;
    }
    let (action, subtitle) = match executor(cx, from_ext.as_deref(), &to_ext) {
        Some((tool, preset)) => (
            Action::OpenTool { tool, preset },
            match &route {
                Some(r) if r.hops() > 1 => format!(
                    "{} steps: {}",
                    r.hops(),
                    r.chain().replace(" -> ", " \u{2192} ")
                ),
                _ => format!("Runs with {}", cx.catalog.tool(tool).name),
            },
        ),
        None => {
            let why = match &route {
                Some(r) => format!(
                    "Route {}, but no installed module runs it yet",
                    r.chain().replace(" -> ", " \u{2192} ")
                ),
                None => "No installed module converts to this yet".to_string(),
            };
            (Action::Unavailable(why.clone()), why)
        }
    };
    let category = match &action {
        Action::OpenTool { tool, .. } => cx.catalog.tool(*tool).category,
        _ => Category::Files,
    };
    Some(Row {
        action,
        title,
        subtitle,
        trailing: category.short().to_string(),
        icon: "refresh-cw",
        category,
        matched: Vec::new(),
        chips: Vec::new(),
        estimate: None,
    })
}

/// Score, tool index and option preset of a conversion module.
type Executor = (i32, usize, Vec<(String, Value)>);

/// The module that runs a conversion, found from manifests alone: it must take the source and
/// either have a choice option offering the target, or produce the target's type.
fn executor(cx: &Context, from: Option<&str>, to: &str) -> Option<(usize, Vec<(String, Value)>)> {
    let to_mime = panoply_core::sniff_bytes(&[], Some(to)).mime;
    let from_ty = from.map(|f| panoply_core::sniff_bytes(&[], Some(f)));
    let mut best: Option<Executor> = None;
    for (i, t) in cx.catalog.tools().iter().enumerate() {
        if !t.takes_files() || !t.on_bar() {
            continue;
        }
        if t.manifest.accepts.iter().all(|a| a == "*/*") {
            continue;
        }
        if let Some(ft) = &from_ty
            && !t.manifest.accepts_file(&ft.mime, &ft.ext)
        {
            continue;
        }
        let option = t.manifest.options.iter().find_map(|(k, s)| {
            (s.kind == panoply_manifest::OptionType::Choice
                && s.values
                    .iter()
                    .any(|v| v == to || (to == "jpg" && v == "jpeg")))
            .then(|| (k.clone(), Value::String(to.to_string())))
        });
        let makes = t.manifest.outputs.contains(&to_mime);
        let mut score = 0;
        if option.is_some() {
            score += 2;
        }
        if makes {
            score += 1;
        }
        if score == 0 {
            continue;
        }
        if t.id.contains("convert") {
            score += 2;
        }
        let preset = option.into_iter().collect::<Vec<_>>();
        if best.as_ref().is_none_or(|b| score > b.0) {
            best = Some((score, i, preset));
        }
    }
    best.map(|(_, i, p)| (i, p))
}

/// The intent a query carries, for tests and the footer.
pub fn understand(query: &str) -> Intent {
    intent::parse(query)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn cat() -> Catalog {
        Catalog::with_dirs(&[])
    }

    fn cx<'a>(c: &'a Catalog, files: &'a [FileProfile], history: &'a [Entry]) -> Context<'a> {
        Context {
            catalog: c,
            files,
            history,
            now: 1_000_000,
        }
    }

    const PNG_HEAD: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

    fn png(name: &str, bytes: u64) -> FileProfile {
        FileProfile {
            path: PathBuf::from(name),
            name: name.into(),
            bytes,
            sniffed: panoply_core::sniff_bytes(PNG_HEAD, Some("png")),
        }
    }

    fn titles(r: &Results) -> Vec<String> {
        r.rows().map(|r| r.title.clone()).collect()
    }

    #[test]
    fn a_sum_is_the_top_row_and_copies_its_answer() {
        let c = cat();
        for (q, shown, copy) in [
            ("15% of 48000", "7,200", "7200"),
            ("500 kb in mb", "0.5 MB", "0.5 MB"),
        ] {
            let r = search(&cx(&c, &[], &[]), q);
            let top = r.get(0).unwrap();
            assert_eq!(r.sections[0].title, "Answer");
            assert_eq!(top.title, shown);
            assert_eq!(
                top.action,
                Action::CopyText {
                    text: copy.to_string()
                }
            );
        }
    }

    #[test]
    fn a_query_for_a_tool_or_a_file_never_gets_an_answer_row() {
        let c = cat();
        let files = [png("sig.png", 200_000)];
        for q in [
            "under 50 kb",
            "merge pdf",
            "heic to jpg",
            "compress",
            "2024",
        ] {
            for files in [&[][..], &files[..]] {
                let r = search(&cx(&c, files, &[]), q);
                assert!(
                    r.rows()
                        .all(|r| !matches!(r.action, Action::CopyText { .. })),
                    "query: {q}"
                );
            }
        }
    }

    #[test]
    fn the_bar_at_rest_suggests_and_lists_recent_jobs() {
        let c = cat();
        let hist = vec![Entry {
            at: 1_000_000 - 120,
            tool_id: "image.resize_to_kb".into(),
            tool_name: "Resize to exact KB".into(),
            inputs: vec![PathBuf::from("/x/signature.jpg")],
            options: Value::Null,
            outputs: vec![],
            headline: String::new(),
        }];
        let r = search(&cx(&c, &[], &hist), "");
        assert_eq!(r.sections[0].title, "Suggested");
        assert_eq!(
            r.sections[0].rows[0].title, "Resize to exact KB",
            "recent first"
        );
        assert!(r.sections[0].rows.len() >= 3);
        assert_eq!(r.sections[1].title, "Recent");
        assert_eq!(r.sections[1].rows[0].subtitle, "signature.jpg");
        assert_eq!(r.sections[1].rows[0].trailing, "2 min ago");
    }

    #[test]
    fn fuzzy_search_finds_real_modules_with_highlights() {
        let c = cat();
        let r = search(&cx(&c, &[], &[]), "cmpr pdf");
        assert_eq!(r.rows().next().unwrap().title, "Compress PDF");
        let r = search(&cx(&c, &[], &[]), "merge");
        let first = r.rows().next().unwrap();
        assert!(first.title.to_lowercase().contains("merge"));
        assert!(!first.matched.is_empty());
    }

    #[test]
    fn community_modules_are_searchable_too() {
        let examples =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/examples-wasm");
        let c = Catalog::with_dirs(&[examples]);
        if c.index_of("text.word_count").is_none() {
            return; // wasm examples not built on this machine
        }
        let r = search(&cx(&c, &[], &[]), "word count");
        assert_eq!(r.rows().next().unwrap().title, "Word count");
    }

    #[test]
    fn the_community_plugins_are_found_by_what_people_type() {
        let examples =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/examples-wasm");
        let c = Catalog::with_dirs(&[examples]);
        if c.index_of("image.qr_code").is_none() {
            return; // wasm examples not built on this machine
        }
        for (typed, title) in [
            ("qr code", "QR code maker"),
            ("markdown", "Markdown to HTML"),
            ("format json", "Format JSON, YAML and TOML"),
            ("yaml", "Format JSON, YAML and TOML"),
        ] {
            let r = search(&cx(&c, &[], &[]), typed);
            assert!(
                titles(&r).iter().take(3).any(|t| t == title),
                "{typed:?} should find {title}: {:?}",
                titles(&r)
            );
        }
        // Dropping a Markdown file offers the converter, a JSON file offers the formatter.
        let md = FileProfile {
            path: PathBuf::from("notes.md"),
            name: "notes.md".into(),
            bytes: 900,
            sniffed: panoply_core::sniff_bytes(b"# Notes\n", Some("md")),
        };
        let r = search(&cx(&c, std::slice::from_ref(&md), &[]), "");
        assert!(
            titles(&r).contains(&"Markdown to HTML".to_string()),
            "{:?}",
            titles(&r)
        );
        let json = FileProfile {
            path: PathBuf::from("data.json"),
            name: "data.json".into(),
            bytes: 90,
            sniffed: panoply_core::sniff_bytes(b"{\"a\":1}", Some("json")),
        };
        let r = search(&cx(&c, std::slice::from_ref(&json), &[]), "");
        assert!(
            titles(&r).contains(&"Format JSON, YAML and TOML".to_string()),
            "{:?}",
            titles(&r)
        );
    }

    #[test]
    fn plain_words_find_a_tool_by_what_it_does() {
        let c = cat();
        let r = search(&cx(&c, &[], &[]), "shrink");
        let t = titles(&r);
        assert!(t.iter().any(|t| t == "Compress PDF"), "{t:?}");
        assert!(t.iter().any(|t| t == "Compress image"), "{t:?}");
    }

    #[test]
    fn a_file_filters_tools_and_the_footer_counts_them() {
        let c = cat();
        let files = [png("shot.png", 312_000)];
        let r = search(&cx(&c, &files, &[]), "");
        assert!(r.sections[0].title.starts_with("For this PNG image"));
        assert!(!titles(&r).contains(&"Compress PDF".to_string()));
        assert!(titles(&r).contains(&"Compress image".to_string()));
        let footer = r.footer.unwrap();
        assert!(footer.contains("tools fit a PNG image"), "{footer}");
    }

    #[test]
    fn typing_under_50_kb_never_flashes_no_tools_and_never_reorders() {
        let c = cat();
        for files in [Vec::new(), vec![png("signature.png", 312_000)]] {
            for phrase in ["under 50 kb", "make this under 50 kb"] {
                let mut typing = Typing::default();
                let mut prev: Option<Vec<String>> = None;
                let mut seen_rows = false;
                for end in 1..=phrase.len() {
                    let q = &phrase[..end];
                    let r = typing.search(&cx(&c, &files, &[]), q);
                    let now = titles(&r);
                    if seen_rows && intent::is_unfinished(q) {
                        assert!(!now.is_empty(), "{q:?} flashed the empty state");
                    }
                    // Up to two letters, rows only drop out; the ones that stay keep their order.
                    if q.trim().len() <= 2
                        && let Some(p) = &prev
                    {
                        let kept: Vec<&String> = p.iter().filter(|t| now.contains(t)).collect();
                        let order: Vec<&String> = now.iter().filter(|t| p.contains(t)).collect();
                        assert_eq!(kept, order, "{q:?} reordered {p:?} into {now:?}");
                    }
                    seen_rows |= !now.is_empty();
                    prev = Some(now);
                }
                let end = typing.search(&cx(&c, &files, &[]), phrase);
                assert_eq!(titles(&end), titles(&search(&cx(&c, &files, &[]), phrase)));
                // A query that is not a way to finish gets the empty state straight away.
                let q = "make this under 50 kbx";
                let typo = typing.search(&cx(&c, &files, &[]), q);
                assert_eq!(titles(&typo), titles(&search(&cx(&c, &files, &[]), q)));
                let junk = typing.search(&cx(&c, &files, &[]), "zzzqx");
                assert!(junk.is_empty());
            }
        }
    }

    #[test]
    fn a_name_with_a_country_a_size_or_a_level_next_to_it_still_finds_the_tool() {
        let c = cat();
        let photo = [png("me.jpg", 400_000)];
        let r = search(&cx(&c, &photo, &[]), "passport photo india");
        assert_eq!(titles(&r)[0], "Passport photo", "{:?}", titles(&r));
        let r = search(&cx(&c, &photo, &[]), "resize to 800px");
        assert!(
            titles(&r).iter().any(|t| t.starts_with("Resize")),
            "{:?}",
            titles(&r)
        );
        let r = search(&cx(&c, &[], &[]), "whatsapp");
        assert!(
            titles(&r).contains(&"Compress video".to_string()),
            "{:?}",
            titles(&r)
        );
        // A request for something Panoply does not do stays empty rather than guessing.
        let r = search(&cx(&c, &[], &[]), "pdf to word");
        assert!(r.is_empty(), "{:?}", titles(&r));
        let r = search(&cx(&c, &[], &[]), "zzzqx");
        assert!(r.is_empty());
    }

    #[test]
    fn make_this_under_50kb_picks_a_size_tool_with_chips_and_an_estimate() {
        let c = cat();
        let files = [png("signature.png", 312_000)];
        let r = search(&cx(&c, &files, &[]), "make this under 50 kb");
        assert_eq!(r.sections[0].title, "Best match");
        let best = &r.sections[0].rows[0];
        assert!(
            ["Resize to exact KB", "Compress image"].contains(&best.title.as_str()),
            "{}",
            best.title
        );
        assert!(
            best.chips
                .iter()
                .any(|(_, v)| v.to_lowercase().contains("50")),
            "{:?}",
            best.chips
        );
        assert_eq!(best.estimate.as_deref(), Some("312 KB to at most 50 KB"));
        assert_eq!(
            r.understood.as_deref(),
            Some("Understood \u{201C}under 50 kb\u{201D} as a target size")
        );
        assert_eq!(r.sections[1].title, "Also matches");
        match &best.action {
            Action::OpenTool { preset, .. } => assert!(!preset.is_empty()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn x_to_y_goes_through_the_router_to_a_real_module() {
        let c = cat();
        let r = search(&cx(&c, &[], &[]), "png to jpg");
        let best = r.rows().next().unwrap();
        assert_eq!(best.title, "PNG to JPG");
        match &best.action {
            Action::OpenTool { tool, preset } => {
                assert_eq!(c.tool(*tool).id, "image.convert");
                assert_eq!(
                    preset,
                    &vec![("to".to_string(), Value::String("jpg".into()))]
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_file_plus_a_bare_format_converts() {
        let c = cat();
        let files = [png("shot.png", 10_000)];
        let r = search(&cx(&c, &files, &[]), "jpg");
        assert_eq!(r.rows().next().unwrap().title, "PNG to JPG");
        let r = search(&cx(&c, &files, &[]), "convert to webp");
        assert_eq!(r.rows().next().unwrap().title, "PNG to WEBP");
    }

    #[test]
    fn multi_step_routes_say_so_and_unroutable_ones_do_not_appear() {
        let c = cat();
        let r = search(&cx(&c, &[], &[]), "docx to jpg");
        let row = r.rows().next().unwrap();
        assert_eq!(row.title, "DOCX to JPG");
        assert!(
            matches!(row.action, Action::Unavailable(_)),
            "{:?}",
            row.action
        );
        assert!(
            row.subtitle.contains("docx \u{2192} pdf \u{2192} jpg"),
            "{}",
            row.subtitle
        );
        let r = search(&cx(&c, &[], &[]), "heic to banana");
        assert!(r.rows().all(|r| !r.title.contains("BANANA")));
    }

    #[test]
    fn nothing_matches_gracefully() {
        let c = cat();
        let r = search(&cx(&c, &[], &[]), "zzzzqqq");
        assert!(r.is_empty());
        assert!(r.get(0).is_none());
    }

    #[test]
    fn flat_indexing_crosses_sections() {
        let c = cat();
        let files = [png("signature.png", 312_000)];
        let r = search(&cx(&c, &files, &[]), "under 50 kb");
        assert_eq!(r.len(), r.rows().count());
        assert_eq!(r.get(0).unwrap().title, r.sections[0].rows[0].title);
        let last = r.len() - 1;
        assert_eq!(r.get(last).unwrap().title, r.rows().last().unwrap().title);
    }
}
