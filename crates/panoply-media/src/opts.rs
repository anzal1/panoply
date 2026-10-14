//! Option handling: fill defaults from the manifest, check every value against its declared
//! type, range and choices, then read them back through typed getters.

use panoply_manifest::{Manifest, OptionSpec, OptionType};
use serde_json::{Map, Value};

use crate::error::{MediaError, Result};

fn bad(msg: impl Into<String>) -> MediaError {
    MediaError::BadOption(format!(
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

    /// Options built directly (tests and tools that skip a manifest).
    pub fn from_json(v: Value) -> Opts {
        Opts {
            map: v.as_object().cloned().unwrap_or_default(),
        }
    }

    /// A text or choice option.
    pub fn str(&self, key: &str) -> Option<&str> {
        self.map.get(key).and_then(Value::as_str)
    }

    /// A text option with a fallback.
    pub fn text<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.str(key).unwrap_or(default)
    }

    /// A number option.
    pub fn f64(&self, key: &str) -> Option<f64> {
        self.map.get(key).and_then(Value::as_f64)
    }

    /// A bool option with a fallback.
    pub fn flag(&self, key: &str, default: bool) -> bool {
        self.map
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(default)
    }

    /// A size option in bytes: a number of bytes, or text such as `25mb`.
    pub fn bytes(&self, key: &str) -> Result<Option<u64>> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Number(n)) => Ok(n.as_f64().map(|v| v.max(0.0) as u64)),
            Some(Value::String(s)) => crate::units::parse_size(s).map(Some),
            Some(_) => Err(bad(format!("{key} is not a size"))),
        }
    }
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
            if !v.is_string() && !v.is_number() {
                return Err(bad(format!("{key} needs a size like 25mb")));
            }
        }
    }
    Ok(())
}
