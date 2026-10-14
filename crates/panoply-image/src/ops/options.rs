//! Option handling: fill defaults from the manifest, check every value against its declared
//! type, range and choices, then read them back through typed getters.

use panoply_manifest::{Manifest, OptionSpec, OptionType};
use serde_json::{Map, Value};

use crate::error::{ImageError, Result};

fn bad(msg: impl Into<String>) -> ImageError {
    ImageError::BadOption(format!(
        "One of the options is not right: {}. Change the option and run again.",
        msg.into()
    ))
}

/// Options after defaults and checks.
#[derive(Debug, Clone)]
pub struct Opts {
    map: Map<String, Value>,
}

impl Opts {
    /// Merge `supplied` over the manifest defaults and check the result.
    pub fn resolve(manifest: &Manifest, supplied: &Value) -> Result<Opts> {
        let given = match supplied {
            Value::Null => Map::new(),
            Value::Object(m) => m.clone(),
            _ => return Err(bad("options must be an object of name and value")),
        };
        for key in given.keys() {
            if !manifest.options.contains_key(key) {
                let known: Vec<&str> = manifest.options.keys().map(String::as_str).collect();
                return Err(bad(format!(
                    "\"{key}\" is not an option, try one of: {}",
                    known.join(", ")
                )));
            }
        }
        let mut out = Map::new();
        for (key, spec) in &manifest.options {
            // Empty text and null count as "not given".
            let value = given
                .get(key)
                .filter(|v| !v.is_null() && v.as_str() != Some(""))
                .cloned()
                .or_else(|| spec.default.clone());
            if let Some(v) = value {
                check(key, spec, &v)?;
                out.insert(key.clone(), v);
            }
        }
        Ok(Opts { map: out })
    }

    /// A text or choice option.
    pub fn str(&self, key: &str) -> Option<&str> {
        self.map.get(key).and_then(Value::as_str)
    }

    /// A number option.
    pub fn f64(&self, key: &str) -> Option<f64> {
        self.map.get(key).and_then(Value::as_f64)
    }

    /// A number option that must be a whole number of at least 1.
    pub fn u32(&self, key: &str) -> Option<u32> {
        self.f64(key).map(|v| v.round().max(0.0) as u32)
    }

    /// A bool option.
    pub fn bool(&self, key: &str) -> Option<bool> {
        self.map.get(key).and_then(Value::as_bool)
    }

    /// A bool option with a fallback.
    pub fn flag(&self, key: &str, default: bool) -> bool {
        self.bool(key).unwrap_or(default)
    }

    /// A size option in bytes: a number, or text such as `200kb`, `1.5mb`.
    pub fn bytes(&self, key: &str) -> Result<Option<u64>> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Number(n)) => Ok(n.as_f64().map(|v| v.max(0.0) as u64)),
            Some(Value::String(s)) => parse_size(s).map(Some).ok_or_else(|| {
                bad(format!(
                    "{key} is not a size, try something like 200kb or 1.5mb"
                ))
            }),
            Some(_) => Err(bad(format!("{key} is not a size"))),
        }
    }
}

/// Parse `200kb`, `1.5 MB`, `64k`, `2048` (bytes). `kb` is 1000 bytes, `kib` is 1024.
pub fn parse_size(s: &str) -> Option<u64> {
    let t = s.trim().to_ascii_lowercase().replace([' ', '_'], "");
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
        .unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let n: f64 = num.replace(',', "").parse().ok()?;
    let mult = match unit {
        "" | "b" | "bytes" => 1.0,
        "k" | "kb" => 1000.0,
        "kib" => 1024.0,
        "m" | "mb" => 1e6,
        "mib" => 1048576.0,
        _ => return None,
    };
    (n >= 0.0).then(|| (n * mult).round() as u64)
}

fn check(key: &str, spec: &OptionSpec, v: &Value) -> Result<()> {
    match spec.kind {
        OptionType::Number => {
            let n = v
                .as_f64()
                .ok_or_else(|| bad(format!("{key} needs a number")))?;
            if spec.min.is_some_and(|lo| n < lo) || spec.max.is_some_and(|hi| n > hi) {
                let range = match (spec.min, spec.max) {
                    (Some(lo), Some(hi)) => format!("between {lo} and {hi}"),
                    (Some(lo), None) => format!("at least {lo}"),
                    (None, Some(hi)) => format!("at most {hi}"),
                    (None, None) => String::new(),
                };
                return Err(bad(format!("{key} must be {range}")));
            }
        }
        OptionType::Bool => {
            if !v.is_boolean() {
                return Err(bad(format!("{key} needs true or false")));
            }
        }
        OptionType::Choice => match v.as_str() {
            Some(s) if spec.values.iter().any(|c| c == s) => {}
            _ => {
                return Err(bad(format!(
                    "{key} must be one of: {}",
                    spec.values.join(", ")
                )));
            }
        },
        OptionType::Text | OptionType::File => {
            if !v.is_string() {
                return Err(bad(format!("{key} needs text")));
            }
        }
        OptionType::Size => {
            let ok = v.is_number() || v.as_str().is_some_and(|s| parse_size(s).is_some());
            if !ok {
                return Err(bad(format!("{key} needs a size, like 200kb or 1.5mb")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: &str = r#"
accepts = ["image/*"]
[module]
id = "image.t"
name = "T"
version = "0.1.0"
pattern = "B"
summary = "x"
[options.quality]
type = "number"
default = 85
min = 1
max = 100
[options.to]
type = "choice"
values = ["jpg", "png"]
default = "jpg"
[options.keep]
type = "bool"
default = true
[options.under]
type = "size"
"#;

    fn manifest() -> Manifest {
        Manifest::from_toml_str(M).unwrap()
    }

    #[test]
    fn defaults_fill_in_and_checks_apply() {
        let o = Opts::resolve(&manifest(), &Value::Null).unwrap();
        assert_eq!(o.u32("quality"), Some(85));
        assert_eq!(o.str("to"), Some("jpg"));
        assert!(o.flag("keep", false));
        let o = Opts::resolve(
            &manifest(),
            &serde_json::json!({"quality": 40, "to": "png"}),
        )
        .unwrap();
        assert_eq!(o.u32("quality"), Some(40));
        assert!(Opts::resolve(&manifest(), &serde_json::json!({"quality": 400})).is_err());
        assert!(Opts::resolve(&manifest(), &serde_json::json!({"to": "gif"})).is_err());
        assert!(Opts::resolve(&manifest(), &serde_json::json!({"nope": 1})).is_err());
    }

    #[test]
    fn sizes_parse() {
        assert_eq!(parse_size("200kb"), Some(200_000));
        assert_eq!(parse_size("1.5 MB"), Some(1_500_000));
        assert_eq!(parse_size("64KiB"), Some(65_536));
        assert_eq!(parse_size("2048"), Some(2048));
        assert_eq!(parse_size("abc"), None);
        let o = Opts::resolve(&manifest(), &serde_json::json!({"under": "200kb"})).unwrap();
        assert_eq!(o.bytes("under").unwrap(), Some(200_000));
    }
}
