//! The pure part: Markdown text in, HTML text out.

use pulldown_cmark::{CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd, html};

pub struct Settings {
    pub theme: String,
    pub paper: String,
    pub title: String,
    pub toc: bool,
    pub page: bool,
}

struct Heading {
    level: u8,
    id: String,
    text: String,
}

fn level_num(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}

/// A heading's anchor: lowercase words joined by hyphens, the same way GitHub does it.
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if (c == ' ' || c == '-' || c == '_') && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Convert Markdown to an HTML body, and report the headings found.
fn body(md: &str) -> (String, Vec<(u8, String, String)>) {
    let md = md.trim_start_matches('\u{feff}');
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SMART_PUNCTUATION
        | Options::ENABLE_HEADING_ATTRIBUTES;
    let events: Vec<Event> = Parser::new_ext(md, opts).collect();

    // Pass one: find each heading's text, and give it an id unless the author set one.
    let mut headings: Vec<Heading> = Vec::new();
    let mut used: Vec<String> = Vec::new();
    let mut open: Option<usize> = None;
    let mut ids: Vec<Option<String>> = Vec::new();
    for ev in &events {
        match ev {
            Event::Start(Tag::Heading { level, id, .. }) => {
                ids.push(id.as_ref().map(|s| s.to_string()));
                headings.push(Heading {
                    level: level_num(*level),
                    id: id.as_ref().map(|s| s.to_string()).unwrap_or_default(),
                    text: String::new(),
                });
                open = Some(headings.len() - 1);
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some(i) = open {
                    headings[i].text.push_str(t);
                }
            }
            Event::End(TagEnd::Heading(_)) => open = None,
            _ => {}
        }
    }
    for h in headings.iter_mut() {
        if h.id.is_empty() {
            let base = slug(&h.text);
            let base = if base.is_empty() {
                "section".to_string()
            } else {
                base
            };
            let mut candidate = base.clone();
            let mut n = 1;
            while used.contains(&candidate) {
                candidate = format!("{base}-{n}");
                n += 1;
            }
            h.id = candidate;
        }
        used.push(h.id.clone());
    }

    // Pass two: write the ids into the events.
    let mut next = 0;
    let events: Vec<Event> = events
        .into_iter()
        .map(|ev| match ev {
            Event::Start(Tag::Heading {
                level,
                classes,
                attrs,
                ..
            }) => {
                let id = headings[next].id.clone();
                next += 1;
                Event::Start(Tag::Heading {
                    level,
                    id: Some(CowStr::from(id)),
                    classes,
                    attrs,
                })
            }
            other => other,
        })
        .collect();

    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    (
        out,
        headings
            .into_iter()
            .map(|h| (h.level, h.id, h.text))
            .collect(),
    )
}

fn toc_html(headings: &[(u8, String, String)]) -> String {
    let shown: Vec<_> = headings
        .iter()
        .filter(|(l, _, _)| *l >= 2 && *l <= 4)
        .collect();
    if shown.len() < 2 {
        return String::new();
    }
    let mut out = String::from("<nav class=\"toc\" aria-label=\"Contents\"><p>Contents</p><ul>\n");
    for (l, id, text) in shown {
        out.push_str(&format!(
            "<li class=\"l{l}\"><a href=\"#{}\">{}</a></li>\n",
            escape(id),
            escape(text)
        ));
    }
    out.push_str("</ul></nav>\n");
    out
}

pub fn convert(md: &str, file_stem: &str, s: &Settings) -> String {
    let (html_body, headings) = body(md);
    if !s.page {
        return html_body;
    }
    let title = if !s.title.trim().is_empty() {
        s.title.trim().to_string()
    } else if let Some((_, _, t)) = headings.iter().find(|(l, _, _)| *l == 1) {
        t.clone()
    } else if let Some((_, _, t)) = headings.first() {
        t.clone()
    } else {
        file_stem.to_string()
    };
    let toc = if s.toc {
        toc_html(&headings)
    } else {
        String::new()
    };
    let scheme = match s.theme.as_str() {
        "dark" => "dark",
        "light" => "light",
        _ => "light dark",
    };
    let paper = if s.paper == "Letter" { "Letter" } else { "A4" };
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data: https: http: file:; media-src data: https: http: file:; style-src 'unsafe-inline'; font-src data:\">\n\
<title>{title}</title>\n<style>\n:root {{ color-scheme: {scheme}; }}\n{css}@page {{ size: {paper}; margin: 18mm; }}\n</style>\n</head>\n<body>\n<main>\n{toc}{body}</main>\n</body>\n</html>\n",
        title = escape(&title),
        css = CSS,
        body = html_body,
    )
}

const CSS: &str = r#"
html { -webkit-text-size-adjust: 100%; }
body { margin: 0; background: Canvas; color: CanvasText; font: 17px/1.65 ui-sans-serif, -apple-system, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; }
main { max-width: 46rem; margin: 0 auto; padding: 3rem 1.25rem 5rem; }
h1, h2, h3, h4, h5, h6 { line-height: 1.25; margin: 2em 0 .5em; font-weight: 650; }
h1 { font-size: 2.1rem; margin-top: 0; }
h2 { font-size: 1.5rem; padding-bottom: .3em; border-bottom: 1px solid color-mix(in srgb, CanvasText 18%, transparent); }
h3 { font-size: 1.2rem; }
p, ul, ol, blockquote, pre, table, hr { margin: 0 0 1.1em; }
a { color: LinkText; }
img, video { max-width: 100%; height: auto; }
code, pre { font: .88em/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
code { padding: .12em .35em; border-radius: 4px; background: color-mix(in srgb, CanvasText 8%, transparent); }
pre { padding: .9rem 1rem; overflow-x: auto; border-radius: 8px; background: color-mix(in srgb, CanvasText 7%, transparent); }
pre code { padding: 0; background: none; font-size: 1em; }
blockquote { padding: .1em 1rem; border-left: 3px solid color-mix(in srgb, CanvasText 30%, transparent); color: color-mix(in srgb, CanvasText 75%, transparent); }
table { border-collapse: collapse; display: block; overflow-x: auto; }
th, td { padding: .4em .8em; border: 1px solid color-mix(in srgb, CanvasText 20%, transparent); }
th { background: color-mix(in srgb, CanvasText 6%, transparent); text-align: left; }
hr { border: 0; border-top: 1px solid color-mix(in srgb, CanvasText 20%, transparent); }
li:has(> input[type=checkbox]) { list-style: none; margin-left: -1.3em; }
li > input[type=checkbox] { margin-right: .5em; }
.footnote-definition { font-size: .9em; }
.footnote-definition p { display: inline; }
.toc { margin: 0 0 2.5rem; padding: 1rem 1.25rem; border-radius: 8px; background: color-mix(in srgb, CanvasText 5%, transparent); }
.toc p { margin: 0 0 .4rem; font-weight: 650; }
.toc ul { list-style: none; margin: 0; padding: 0; }
.toc li { margin: .15rem 0; }
.toc .l3 { padding-left: 1.2rem; }
.toc .l4 { padding-left: 2.4rem; }
.toc a { text-decoration: none; }
@media print {
  :root { color-scheme: light; }
  body { background: #fff; color: #000; font-size: 11pt; }
  main { max-width: none; padding: 0; }
  a { color: #000; text-decoration: underline; }
  pre, blockquote, table, img, h2, h3 { break-inside: avoid; }
  h1, h2, h3, h4 { break-after: avoid; }
  pre { white-space: pre-wrap; border: 1px solid #ccc; background: #f6f6f6; }
  code { background: #f1f1f1; }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn s() -> Settings {
        Settings {
            theme: "auto".into(),
            paper: "A4".into(),
            title: String::new(),
            toc: false,
            page: true,
        }
    }

    const README: &str = "# Panoply\n\nEvery tool, **one click**. See [the docs](https://example.com/docs).\n\n## Install\n\n```sh\ncargo install panoply\n```\n\n## Usage\n\n| Command | What it does |\n|---|---|\n| `run` | Run a module |\n| `modules` | List them |\n\n- [x] Fast\n- [ ] Done\n\n> Nothing leaves your device.\n\n## Usage\n\nText with a footnote.[^1]\n\n[^1]: Like this.\n";

    #[test]
    fn a_readme_converts() {
        let h = convert(README, "README", &s());
        assert!(h.starts_with("<!doctype html>"));
        assert!(h.contains("<title>Panoply</title>"));
        assert!(h.contains("<strong>one click</strong>"));
        assert!(h.contains("<a href=\"https://example.com/docs\">the docs</a>"));
        assert!(h.contains("<table>") && h.contains("<th>Command</th>"));
        assert!(h.contains("type=\"checkbox\""));
        assert!(h.contains("<pre><code class=\"language-sh\">cargo install panoply"));
        assert!(h.contains("class=\"footnote-definition\""));
    }

    #[test]
    fn headings_get_unique_anchors() {
        let h = convert(README, "README", &s());
        assert!(h.contains("<h2 id=\"usage\">Usage</h2>"), "{h}");
        assert!(h.contains("<h2 id=\"usage-1\">Usage</h2>"), "{h}");
    }

    #[test]
    fn table_of_contents_links_to_the_anchors() {
        let h = convert(README, "README", &Settings { toc: true, ..s() });
        assert!(h.contains("<a href=\"#install\">Install</a>"));
        assert!(h.contains("class=\"toc\""));
    }

    #[test]
    fn fragment_mode_has_no_page() {
        let h = convert("Hello *world*", "x", &Settings { page: false, ..s() });
        assert_eq!(h.trim(), "<p>Hello <em>world</em></p>");
    }

    #[test]
    fn title_option_and_escaping() {
        let h = convert(
            "# a",
            "x",
            &Settings {
                title: "Q&A <1>".into(),
                ..s()
            },
        );
        assert!(h.contains("<title>Q&amp;A &lt;1&gt;</title>"));
    }

    #[test]
    fn untitled_file_uses_its_name() {
        assert!(
            convert("just text", "meeting notes", &s()).contains("<title>meeting notes</title>")
        );
    }

    #[test]
    fn scripts_are_blocked_by_the_page_policy() {
        let h = convert("<script>alert(1)</script>\n\nhi", "x", &s());
        assert!(h.contains("default-src 'none'"));
        assert!(!h.contains("script-src"));
    }

    #[test]
    fn paper_and_theme() {
        let h = convert(
            "x",
            "x",
            &Settings {
                paper: "Letter".into(),
                theme: "dark".into(),
                ..s()
            },
        );
        assert!(h.contains("size: Letter"));
        assert!(h.contains("color-scheme: dark;"));
    }

    #[test]
    fn smart_punctuation_and_bom() {
        let h = convert(
            "\u{feff}\"Quoted\" -- yes...",
            "x",
            &Settings { page: false, ..s() },
        );
        assert!(
            h.contains('\u{201c}') && h.contains('\u{2013}') && h.contains('\u{2026}'),
            "{h}"
        );
    }
}
