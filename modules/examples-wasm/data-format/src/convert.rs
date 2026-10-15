//! The pure part of the module: parse one of three formats, write one of three formats.
//! Nothing here touches the host, so it runs under plain `cargo test`.

use serde_json::{Map, Number, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Json,
    Yaml,
    Toml,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::Yaml => "yaml",
            Format::Toml => "toml",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Json => "JSON",
            Format::Yaml => "YAML",
            Format::Toml => "TOML",
        }
    }

    pub fn parse_name(name: &str) -> Option<Format> {
        match name {
            "json" => Some(Format::Json),
            "yaml" => Some(Format::Yaml),
            "toml" => Some(Format::Toml),
            _ => None,
        }
    }
}

/// The format a file is in: its extension first, then what the text looks like.
pub fn detect(name: &str, text: &str) -> Format {
    let lower = name.to_lowercase();
    if lower.ends_with(".json") {
        return Format::Json;
    }
    if lower.ends_with(".yaml") || lower.ends_with(".yml") {
        return Format::Yaml;
    }
    if lower.ends_with(".toml") {
        return Format::Toml;
    }
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if t.starts_with('{') || t.starts_with('[') && !t.starts_with("[[") && t.contains(',') {
        Format::Json
    } else {
        Format::Yaml
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indent {
    Spaces(usize),
    Tab,
    Compact,
}

impl Indent {
    pub fn parse(s: &str) -> Indent {
        match s {
            "4" => Indent::Spaces(4),
            "tab" => Indent::Tab,
            "compact" => Indent::Compact,
            _ => Indent::Spaces(2),
        }
    }
}

/// Parse text. The error says where the mistake is, in the file's own words.
pub fn parse(format: Format, text: &str) -> Result<Value, String> {
    let text = text.trim_start_matches('\u{feff}');
    match format {
        Format::Json => serde_json::from_str::<Value>(text).map_err(|e| {
            format!(
                "Line {}, column {}: {}",
                e.line(),
                e.column(),
                plain_json_error(&e.to_string())
            )
        }),
        Format::Yaml => serde_yaml_ng::from_str::<Value>(text).map_err(|e| {
            if e.to_string().contains("more than one document") {
                return "It holds several documents (separated by ---), and this tool handles one at a time. Split it into one file per document first.".to_string();
            }
            match e.location() {
                Some(l) => format!("Line {}, column {}: {}", l.line(), l.column(), strip_at(&e.to_string())),
                None => e.to_string(),
            }
        }),
        Format::Toml => {
            let table: toml::Table = text.parse().map_err(|e: toml::de::Error| match e.span() {
                Some(span) => {
                    let (line, col) = line_col(text, span.start);
                    format!("Line {line}, column {col}: {}", e.message())
                }
                None => e.message().to_string(),
            })?;
            Ok(from_toml(toml::Value::Table(table)))
        }
    }
}

fn plain_json_error(msg: &str) -> String {
    // serde_json ends its messages with " at line N column M"; the position is already shown.
    match msg.find(" at line ") {
        Some(i) => msg[..i].to_string(),
        None => msg.to_string(),
    }
}

fn strip_at(msg: &str) -> String {
    match msg.rfind(" at line ") {
        Some(i) => msg[..i].to_string(),
        None => msg.to_string(),
    }
}

fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

fn from_toml(v: toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::Number(i.into()),
        toml::Value::Float(f) => {
            Number::from_f64(f).map_or_else(|| Value::String(f.to_string()), Value::Number)
        }
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(a) => Value::Array(a.into_iter().map(from_toml).collect()),
        toml::Value::Table(t) => Value::Object(
            t.into_iter()
                .map(|(k, v)| (k, from_toml(v)))
                .collect::<Map<_, _>>(),
        ),
    }
}

fn to_toml(v: &Value, path: &str) -> Result<toml::Value, String> {
    Ok(match v {
        Value::Null => {
            return Err(format!(
                "TOML has no null, and {} is null. Pick JSON or YAML, or give it a value first.",
                if path.is_empty() { "the file" } else { path }
            ));
        }
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                toml::Value::Float(f)
            } else {
                return Err(format!("{path} is a number too large for TOML."));
            }
        }
        Value::String(s) => toml::Value::String(s.clone()),
        Value::Array(a) => {
            let mut out = Vec::new();
            for (i, x) in a.iter().enumerate() {
                out.push(to_toml(x, &format!("{path}[{i}]"))?);
            }
            toml::Value::Array(out)
        }
        Value::Object(o) => {
            let mut t = toml::Table::new();
            for (k, x) in o {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                t.insert(k.clone(), to_toml(x, &p)?);
            }
            toml::Value::Table(t)
        }
    })
}

pub fn sort_keys(v: &mut Value) {
    match v {
        Value::Object(o) => {
            let mut entries: Vec<(String, Value)> = std::mem::take(o).into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, mut x) in entries {
                sort_keys(&mut x);
                o.insert(k, x);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(sort_keys),
        _ => {}
    }
}

/// Write a value in a format. The text always ends with one newline.
pub fn write(format: Format, v: &Value, indent: Indent) -> Result<String, String> {
    let mut out = match format {
        Format::Json => match indent {
            Indent::Compact => serde_json::to_string(v).map_err(|e| e.to_string())?,
            other => {
                let unit: &[u8] = match other {
                    Indent::Tab => b"\t",
                    Indent::Spaces(4) => b"    ",
                    _ => b"  ",
                };
                let mut buf = Vec::new();
                let fmt = serde_json::ser::PrettyFormatter::with_indent(unit);
                let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
                serde::Serialize::serialize(v, &mut ser).map_err(|e| e.to_string())?;
                String::from_utf8(buf).map_err(|e| e.to_string())?
            }
        },
        Format::Yaml => serde_yaml_ng::to_string(v).map_err(|e| e.to_string())?,
        Format::Toml => {
            if !v.is_object() {
                return Err("TOML needs a table (key = value pairs) at the top, and this file's top is a list or a single value. Pick JSON or YAML.".into());
            }
            let t = to_toml(v, "")?;
            toml::to_string_pretty(&t).map_err(|e| e.to_string())?
        }
    };
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKAGE_JSON: &str = r#"{"name":"left-pad","version":"1.3.0","scripts":{"test":"node test.js","build":"tsc"},"files":["index.js"],"private":false}"#;

    #[test]
    fn json_pretty_keeps_key_order() {
        let v = parse(Format::Json, PACKAGE_JSON).unwrap();
        let out = write(Format::Json, &v, Indent::Spaces(2)).unwrap();
        assert!(
            out.starts_with("{\n  \"name\": \"left-pad\",\n  \"version\""),
            "{out}"
        );
        assert!(out.find("\"test\"").unwrap() < out.find("\"build\"").unwrap());
    }

    #[test]
    fn json_compact_is_one_line() {
        let v = parse(Format::Json, "{\n  \"a\": [1, 2],\n  \"b\": null\n}").unwrap();
        assert_eq!(
            write(Format::Json, &v, Indent::Compact).unwrap(),
            "{\"a\":[1,2],\"b\":null}\n"
        );
    }

    #[test]
    fn tab_indent() {
        let v = parse(Format::Json, "{\"a\":{\"b\":1}}").unwrap();
        assert_eq!(
            write(Format::Json, &v, Indent::Tab).unwrap(),
            "{\n\t\"a\": {\n\t\t\"b\": 1\n\t}\n}\n"
        );
    }

    #[test]
    fn sorting_is_deep() {
        let mut v = parse(
            Format::Json,
            "{\"b\":{\"z\":1,\"a\":2},\"a\":[{\"y\":1,\"x\":2}]}",
        )
        .unwrap();
        sort_keys(&mut v);
        assert_eq!(
            write(Format::Json, &v, Indent::Compact).unwrap(),
            "{\"a\":[{\"x\":2,\"y\":1}],\"b\":{\"a\":2,\"z\":1}}\n"
        );
    }

    #[test]
    fn json_error_has_line_and_column() {
        let e = parse(Format::Json, "{\n  \"a\": 1,\n  \"b\": ,\n}").unwrap_err();
        assert!(e.starts_with("Line 3, column"), "{e}");
        assert!(!e.contains(" at line "), "{e}");
    }

    #[test]
    fn yaml_error_has_line_and_column() {
        let e = parse(Format::Yaml, "a: 1\nb: [1, 2\nc: 3\n").unwrap_err();
        assert!(e.starts_with("Line "), "{e}");
    }

    #[test]
    fn several_yaml_documents_are_explained() {
        let e = parse(Format::Yaml, "a: 1\n---\nb: 2\n").unwrap_err();
        assert!(e.contains("several documents"), "{e}");
    }

    #[test]
    fn toml_error_has_line_and_column() {
        let e = parse(Format::Toml, "[package]\nname = \"x\"\nversion = \n").unwrap_err();
        assert!(e.starts_with("Line 3, column"), "{e}");
    }

    #[test]
    fn cargo_toml_to_json_and_back() {
        let src = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\n\n[[bin]]\nname = \"a\"\n\n[[bin]]\nname = \"b\"\n";
        let v = parse(Format::Toml, src).unwrap();
        let json = write(Format::Json, &v, Indent::Spaces(2)).unwrap();
        let back = parse(Format::Json, &json).unwrap();
        let toml_again = write(Format::Toml, &back, Indent::Spaces(2)).unwrap();
        assert_eq!(parse(Format::Toml, &toml_again).unwrap(), v);
        assert_eq!(v["bin"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn toml_datetime_becomes_text() {
        let v = parse(Format::Toml, "when = 2026-10-16T09:30:00Z\n").unwrap();
        assert_eq!(v["when"], "2026-10-16T09:30:00Z");
    }

    #[test]
    fn compose_yaml_to_json() {
        let src = "services:\n  web:\n    image: nginx:1.27\n    ports:\n      - \"8080:80\"\n    environment:\n      DEBUG: false\n  db:\n    image: postgres:16\n";
        let v = parse(Format::Yaml, src).unwrap();
        assert_eq!(v["services"]["web"]["ports"][0], "8080:80");
        assert_eq!(v["services"]["web"]["environment"]["DEBUG"], false);
        let keys: Vec<_> = v["services"].as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["web", "db"]);
    }

    #[test]
    fn null_into_toml_says_where() {
        let v = parse(Format::Json, "{\"a\":{\"b\":null}}").unwrap();
        let e = write(Format::Toml, &v, Indent::Spaces(2)).unwrap_err();
        assert!(e.contains("a.b"), "{e}");
    }

    #[test]
    fn list_at_top_into_toml_is_refused_kindly() {
        let v = parse(Format::Json, "[1,2,3]").unwrap();
        assert!(
            write(Format::Toml, &v, Indent::Spaces(2))
                .unwrap_err()
                .contains("table")
        );
    }

    #[test]
    fn byte_order_mark_is_ignored() {
        assert!(parse(Format::Json, "\u{feff}{\"a\":1}").is_ok());
    }

    #[test]
    fn detection() {
        assert_eq!(detect("x.YML", ""), Format::Yaml);
        assert_eq!(detect("data", "{\"a\":1}"), Format::Json);
        assert_eq!(detect("data", "a: 1"), Format::Yaml);
    }
}
