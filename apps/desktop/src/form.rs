//! The options form, built from a manifest. There is no module-specific code here: every option
//! type has one control, and the same code renders `pdf.compress`, a community word counter and a
//! module that does not exist yet.

use std::collections::BTreeMap;

use panoply_manifest::{Manifest, OptionSpec, OptionType};
use serde_json::{Map, Number, Value};

use crate::units::parse_size;

/// The native control an option renders as.
#[derive(Debug, Clone, PartialEq)]
pub enum Control {
    /// A number with a stepper and optional limits.
    Number {
        /// Lowest value.
        min: Option<f64>,
        /// Highest value.
        max: Option<f64>,
    },
    /// A segmented control, one of these.
    Choice(Vec<String>),
    /// A switch.
    Toggle,
    /// A size such as `2mb`.
    Size,
    /// Free text.
    Text,
    /// A file path chosen by the user.
    File,
}

/// One option of a module.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// The option name in the manifest.
    pub key: String,
    /// Label, from the manifest or the key.
    pub label: String,
    /// One line of help.
    pub help: Option<String>,
    /// The control.
    pub control: Control,
    /// Default from the manifest.
    pub default: Option<Value>,
}

/// How many fields show before the rest fold under "More options".
pub const PRIMARY_FIELDS: usize = 4;

/// Option values the user set, over the manifest defaults.
#[derive(Debug, Clone)]
pub struct Form {
    manifest: Manifest,
    fields: Vec<Field>,
    set: BTreeMap<String, Value>,
    errors: BTreeMap<String, String>,
}

fn humanize(key: &str) -> String {
    let mut s = key.replace('_', " ");
    if let Some(c) = s.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    s
}

fn control_for(spec: &OptionSpec) -> Control {
    match spec.kind {
        OptionType::Number => Control::Number {
            min: spec.min,
            max: spec.max,
        },
        OptionType::Choice => Control::Choice(spec.values.clone()),
        OptionType::Bool => Control::Toggle,
        OptionType::Size => Control::Size,
        OptionType::Text => Control::Text,
        OptionType::File => Control::File,
    }
}

/// The option that decides what a tool does, or that a tool cannot run without, in the order they
/// should be asked. They are always shown, ahead of format, quality and the rest, so a person never
/// has to open "More options" to say which country, which area, how big or which pages.
const PINNED: &[&str] = &[
    "country",
    "own_content",
    "rect",
    "mask",
    "to",
    "result",
    "target",
    "mode",
    "scale",
    "longest",
    "width",
    "height",
    "percent",
    "under",
    "under_mb",
    "max_size",
    "max_kb",
    "keep",
    "remove",
    "every",
    "ranges",
    "drop",
    "extract",
    "action",
    "preset",
    "languages",
];

fn pin_index(key: &str) -> usize {
    PINNED.iter().position(|k| *k == key).unwrap_or(usize::MAX)
}

/// Options people reach for first, and options that are rarely touched. The manifest keeps
/// options in a sorted map, so the order has to come from what the names say.
fn priority(key: &str) -> u8 {
    const FIRST: &[&str] = &[
        "target", "to", "format", "preset", "mode", "quality", "under", "max_", "min_", "width",
        "height", "dpi", "size", "pages", "every", "ranges", "keep", "delete", "rotate",
    ];
    const LAST: &[&str] = &[
        "animation",
        "color_profile",
        "metadata",
        "strip",
        "lossless",
        "flatten",
        "baseline",
        "pad_to",
        "password",
        "zip",
        "order",
        "name_pattern",
        "interleave",
        "reverse",
        "bookmarks",
    ];
    if FIRST.iter().any(|w| key.starts_with(w)) {
        0
    } else if LAST.iter().any(|w| key.contains(w)) {
        2
    } else {
        1
    }
}

fn rank(c: &Control) -> u8 {
    match c {
        Control::Choice(_) => 0,
        Control::Size | Control::Number { .. } => 1,
        Control::Text | Control::File => 2,
        Control::Toggle => 3,
    }
}

impl Form {
    /// The form for a module. Fields are ordered choices first, then numbers and sizes, then text,
    /// then switches, because the manifest keeps options in a sorted map, not in file order.
    pub fn from_manifest(manifest: &Manifest) -> Form {
        let mut fields: Vec<Field> = manifest
            .options
            .iter()
            .map(|(key, spec)| Field {
                key: key.clone(),
                label: spec.label.clone().unwrap_or_else(|| humanize(key)),
                help: spec.help.clone(),
                control: control_for(spec),
                default: spec.default.clone(),
            })
            .collect();
        fields.sort_by_key(|f| (pin_index(&f.key), priority(&f.key), rank(&f.control)));
        Form {
            manifest: manifest.clone(),
            fields,
            set: BTreeMap::new(),
            errors: BTreeMap::new(),
        }
    }

    /// Every field, in display order.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The fields shown up front.
    pub fn primary(&self) -> &[Field] {
        &self.fields[..self.fields.len().min(PRIMARY_FIELDS)]
    }

    /// The fields folded under "More options".
    pub fn more(&self) -> &[Field] {
        self.fields.get(PRIMARY_FIELDS..).unwrap_or(&[])
    }

    /// A field by key.
    pub fn field(&self, key: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.key == key)
    }

    /// The value in effect: what the user set, else the default.
    pub fn value(&self, key: &str) -> Option<Value> {
        self.set
            .get(key)
            .cloned()
            .or_else(|| self.field(key).and_then(|f| f.default.clone()))
    }

    /// True when the user (or a request such as "under 50 kb") set this field.
    pub fn is_set(&self, key: &str) -> bool {
        self.set.contains_key(key)
    }

    /// Set a field from typed text. Empty text clears it back to the default. Returns the plain
    /// error for this field, which is also kept for [`Form::error`].
    pub fn set_text(&mut self, key: &str, raw: &str) -> Result<(), String> {
        let Some(field) = self.field(key).cloned() else {
            return Err(format!("There is no option called {key}."));
        };
        let raw = raw.trim();
        if raw.is_empty() {
            self.set.remove(key);
            self.errors.remove(key);
            return Ok(());
        }
        let parsed = match &field.control {
            Control::Number { min, max } => {
                let n: f64 = raw.parse().unwrap_or(f64::NAN);
                if !n.is_finite() {
                    Err(format!("{} needs a number.", field.label))
                } else if min.is_some_and(|lo| n < lo) || max.is_some_and(|hi| n > hi) {
                    Err(match (min, max) {
                        (Some(lo), Some(hi)) => {
                            format!("{} must be between {lo} and {hi}.", field.label)
                        }
                        (Some(lo), None) => format!("{} must be at least {lo}.", field.label),
                        (None, Some(hi)) => format!("{} must be at most {hi}.", field.label),
                        (None, None) => String::new(),
                    })
                } else {
                    Ok(number(n))
                }
            }
            Control::Size => {
                if parse_size(raw).is_none() {
                    Err(format!(
                        "{} needs a size such as 200kb or 1.5mb.",
                        field.label
                    ))
                } else if raw.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    Ok(number(raw.parse().unwrap_or(0.0)))
                } else {
                    Ok(Value::String(
                        raw.chars()
                            .filter(|c| !c.is_whitespace())
                            .collect::<String>()
                            .to_ascii_lowercase(),
                    ))
                }
            }
            Control::Text | Control::File => Ok(Value::String(raw.to_string())),
            Control::Choice(values) => {
                if values.iter().any(|v| v == raw) {
                    Ok(Value::String(raw.to_string()))
                } else {
                    Err(format!(
                        "{} must be one of: {}.",
                        field.label,
                        values.join(", ")
                    ))
                }
            }
            Control::Toggle => match raw.to_ascii_lowercase().as_str() {
                "true" | "on" | "yes" => Ok(Value::Bool(true)),
                "false" | "off" | "no" => Ok(Value::Bool(false)),
                _ => Err(format!("{} is on or off.", field.label)),
            },
        };
        match parsed {
            Ok(v) => {
                self.set.insert(key.to_string(), v);
                self.errors.remove(key);
                Ok(())
            }
            Err(e) => {
                self.errors.insert(key.to_string(), e.clone());
                Err(e)
            }
        }
    }

    /// Put the fields a request already set first, so they are on screen when the panel opens.
    pub fn float_set_fields(&mut self) {
        let set = self.set.keys().cloned().collect::<Vec<_>>();
        self.fields.sort_by_key(|f| !set.contains(&f.key));
    }

    /// Set a field to a ready value (a segmented choice, a switch, a request preset).
    pub fn set_value(&mut self, key: &str, v: Value) {
        self.errors.remove(key);
        self.set.insert(key.to_string(), v);
    }

    /// Back to the default.
    pub fn clear(&mut self, key: &str) {
        self.set.remove(key);
        self.errors.remove(key);
    }

    /// The error for one field, when its text does not parse.
    pub fn error(&self, key: &str) -> Option<&str> {
        self.errors.get(key).map(String::as_str)
    }

    /// True when any field holds text that does not parse.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// The options object to run with: the values the user set, checked and completed by the
    /// host against the manifest. The error is a plain sentence.
    pub fn to_options(&self) -> Result<Value, String> {
        if let Some(e) = self.errors.values().next() {
            return Err(e.clone());
        }
        let given: Map<String, Value> = self
            .set
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        panoply_host::options::resolve(&self.manifest, &Value::Object(given))
            .map_err(|e| e.plain_message())
    }

    /// Fields the user set, as short label and value pairs, for the chips on a bar row.
    pub fn chips(&self) -> Vec<(String, String)> {
        self.fields
            .iter()
            .filter(|f| self.is_set(&f.key))
            .map(|f| (f.label.clone(), display(&f.control, self.set.get(&f.key))))
            .collect()
    }

    /// Chips for the bar row: what the request set first, then the first few defaults.
    pub fn chips_with_defaults(&self, max: usize) -> Vec<(String, String)> {
        let mut out = self.chips();
        for f in &self.fields {
            if out.len() >= max {
                break;
            }
            if self.is_set(&f.key) {
                continue;
            }
            if let Some(d) = &f.default {
                out.push((f.label.clone(), display(&f.control, Some(d))));
            }
        }
        out.truncate(max);
        out
    }
}

/// How a value reads on a chip or a control.
pub fn display(control: &Control, v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "Auto".to_string(),
        Some(Value::Bool(true)) => "On".to_string(),
        Some(Value::Bool(false)) => "Off".to_string(),
        Some(Value::String(s)) => match control {
            Control::Size => match s.parse::<u64>() {
                Ok(bytes) => crate::units::format_bytes(bytes),
                Err(_) => s.to_ascii_uppercase(),
            },
            Control::Choice(_) => humanize_value(s),
            _ => s.clone(),
        },
        Some(Value::Number(n)) => match control {
            Control::Size => crate::units::format_bytes(n.as_f64().unwrap_or(0.0) as u64),
            _ => n.to_string(),
        },
        Some(other) => other.to_string(),
    }
}

/// "under_size" reads as "under size"; everything else stays as the manifest wrote it.
pub fn humanize_value(v: &str) -> String {
    match v {
        // "Os" in a segmented control is not a word. The engine the system itself provides.
        "os" => "System".to_string(),
        _ => v.replace('_', " "),
    }
}

/// A JSON number that prints as an integer when it is one.
pub fn number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        Value::Number(Number::from(n as i64))
    } else {
        Number::from_f64(n).map_or(Value::Null, Value::Number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest(extra: &str) -> Manifest {
        Manifest::from_toml_str(&format!(
            r#"
accepts = ["image/*"]
[module]
id = "image.demo"
name = "Demo"
version = "0.1.0"
pattern = "B"
summary = "Demo."
[options.mode]
type = "choice"
values = ["fast", "slow"]
default = "fast"
[options.width]
type = "number"
min = 10
max = 200
label = "Width (px)"
[options.limit]
type = "size"
[options.note]
type = "text"
[options.loud]
type = "bool"
default = false
{extra}
"#
        ))
        .unwrap()
    }

    #[test]
    fn every_option_type_gets_its_control() {
        let f = Form::from_manifest(&manifest("[options.pick]\ntype = \"file\"\n"));
        let kinds: Vec<&str> = f
            .fields()
            .iter()
            .map(|f| match f.control {
                Control::Number { .. } => "number",
                Control::Choice(_) => "choice",
                Control::Toggle => "toggle",
                Control::Size => "size",
                Control::Text => "text",
                Control::File => "file",
            })
            .collect();
        assert_eq!(kinds.len(), 6);
        assert_eq!(
            kinds[0], "choice",
            "mode is a priority name and choices come first"
        );
        assert_eq!(f.field("width").unwrap().label, "Width (px)");
        assert_eq!(f.field("note").unwrap().label, "Note");
    }

    #[test]
    fn primary_and_folded_fields_split() {
        let f = Form::from_manifest(&manifest(""));
        assert_eq!(f.primary().len(), PRIMARY_FIELDS);
        assert_eq!(f.more().len(), f.fields().len() - PRIMARY_FIELDS);
    }

    #[test]
    fn typed_values_are_checked_where_they_are_typed() {
        let mut f = Form::from_manifest(&manifest(""));
        assert!(f.set_text("width", "abc").is_err());
        assert!(f.error("width").unwrap().contains("needs a number"));
        assert!(
            f.set_text("width", "5")
                .unwrap_err()
                .contains("between 10 and 200")
        );
        assert!(f.has_errors());
        assert!(f.to_options().is_err());
        f.set_text("width", "120").unwrap();
        assert!(!f.has_errors());
        f.set_text("limit", "1.5 MB").unwrap();
        assert!(f.set_text("limit", "lots").is_err());
        f.set_text("limit", "200").unwrap();
        assert_eq!(f.value("limit"), Some(json!(200)));
        f.set_text("limit", "").unwrap();
        assert!(!f.is_set("limit"));
    }

    #[test]
    fn options_come_out_resolved_by_the_host() {
        let mut f = Form::from_manifest(&manifest(""));
        f.set_value("loud", json!(true));
        f.set_text("width", "80").unwrap();
        f.set_text("limit", "2mb").unwrap();
        let o = f.to_options().unwrap();
        assert_eq!(o["mode"], "fast");
        assert_eq!(o["width"], 80);
        assert_eq!(o["limit"], "2mb");
        assert_eq!(o["loud"], true);
        assert!(o.get("note").is_none());
    }

    #[test]
    fn a_bad_choice_is_refused() {
        let mut f = Form::from_manifest(&manifest(""));
        assert!(f.set_text("mode", "medium").is_err());
        f.set_value("mode", json!("slow"));
        assert!(!f.has_errors());
        assert_eq!(f.to_options().unwrap()["mode"], "slow");
    }

    #[test]
    fn chips_show_what_was_set_then_defaults() {
        let mut f = Form::from_manifest(&manifest(""));
        f.set_text("limit", "50kb").unwrap();
        let chips = f.chips_with_defaults(3);
        assert_eq!(chips[0], ("Limit".to_string(), "50KB".to_string()));
        assert_eq!(humanize_value("under_size"), "under size");
        assert_eq!(humanize_value("os"), "System");
        assert_eq!(chips.len(), 3);
        assert!(chips.iter().any(|(l, v)| l == "Mode" && v == "fast"));
    }

    #[test]
    fn the_option_a_tool_cannot_run_without_is_never_folded_away() {
        let c = crate::catalog::Catalog::with_dirs(&[]);
        let must_show: &[(&str, &[&str])] = &[
            ("id.passport_photo", &["country"]),
            ("image.erase", &["rect"]),
            ("image.remove_watermark", &["own_content", "rect"]),
            ("image.remove_background", &["result"]),
            ("image.resize", &["mode", "longest"]),
            ("image.upscale", &["scale"]),
            ("image.compress", &["target", "max_size"]),
            ("image.convert", &["to"]),
            ("media.trim", &["keep"]),
            ("media.compress", &["under"]),
            ("pdf.compress", &["target", "under_mb"]),
            ("pdf.split", &["mode", "every"]),
            ("pdf.organize", &["keep", "delete"]),
        ];
        for (id, keys) in must_show {
            let t = c
                .tools()
                .iter()
                .find(|t| t.id == *id)
                .unwrap_or_else(|| panic!("no tool {id}"));
            let f = Form::from_manifest(&t.manifest);
            for k in *keys {
                assert!(
                    f.primary().iter().any(|p| p.key == *k),
                    "{id}: {k} is folded under More options, primary is {:?}",
                    f.primary()
                        .iter()
                        .map(|p| p.key.as_str())
                        .collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn real_manifests_build_forms() {
        let c = crate::catalog::Catalog::with_dirs(&[]);
        for t in c.tools() {
            let f = Form::from_manifest(&t.manifest);
            assert_eq!(f.fields().len(), t.manifest.options.len(), "{}", t.id);
            // With nothing touched, every manifest still resolves to a valid options object.
            let _ = f.to_options().unwrap_or_else(|e| panic!("{}: {e}", t.id));
        }
    }
}
