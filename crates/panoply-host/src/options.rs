//! Option handling: fill defaults from the manifest, check types, and turn `key=value`
//! pairs from the command line into typed JSON. The module only ever sees the finished
//! object, so it never has to re-check what the manifest already promised.

use crate::ModuleError;
use panoply_manifest::{Manifest, OptionSpec, OptionType};
use serde_json::{Map, Number, Value};

fn bad(msg: impl Into<String>) -> ModuleError {
    ModuleError::BadOption(msg.into())
}

/// Turn `key=value` pairs (from `--opt`) into a JSON object typed by the manifest.
///
/// Numbers become numbers, `true` and `false` become booleans for `bool` options, and
/// everything else stays text. Unknown keys are refused with the list of real ones.
pub fn from_pairs(manifest: &Manifest, pairs: &[String]) -> Result<Value, ModuleError> {
    let mut map = Map::new();
    for pair in pairs {
        let (key, raw) = pair
            .split_once('=')
            .ok_or_else(|| bad(format!("\"{pair}\" should look like key=value")))?;
        let spec = manifest
            .options
            .get(key)
            .ok_or_else(|| unknown(manifest, key))?;
        map.insert(key.to_string(), coerce(key, spec, raw)?);
    }
    Ok(Value::Object(map))
}

fn unknown(manifest: &Manifest, key: &str) -> ModuleError {
    let known: Vec<&str> = manifest.options.keys().map(String::as_str).collect();
    if known.is_empty() {
        bad(format!("\"{key}\" is not an option, this module has none"))
    } else {
        bad(format!(
            "\"{key}\" is not an option, try one of: {}",
            known.join(", ")
        ))
    }
}

fn coerce(key: &str, spec: &OptionSpec, raw: &str) -> Result<Value, ModuleError> {
    Ok(match spec.kind {
        OptionType::Number => {
            let n: f64 = raw
                .trim()
                .parse()
                .map_err(|_| bad(format!("{key} needs a number")))?;
            number(n).ok_or_else(|| bad(format!("{key} needs a finite number")))?
        }
        OptionType::Bool => match raw.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Value::Bool(true),
            "false" | "no" | "off" | "0" => Value::Bool(false),
            _ => return Err(bad(format!("{key} needs true or false"))),
        },
        OptionType::Size => match raw.trim().parse::<f64>() {
            Ok(n) => number(n).ok_or_else(|| bad(format!("{key} needs a size")))?,
            Err(_) => Value::String(raw.to_string()),
        },
        OptionType::Choice | OptionType::Text | OptionType::File => Value::String(raw.to_string()),
    })
}

fn number(n: f64) -> Option<Value> {
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        Some(Value::Number(Number::from(n as i64)))
    } else {
        Number::from_f64(n).map(Value::Number)
    }
}

/// Merge `supplied` over the manifest defaults and check every value against its declared
/// type, range and choices. Returns the JSON object handed to the module.
pub fn resolve(manifest: &Manifest, supplied: &Value) -> Result<Value, ModuleError> {
    let given = match supplied {
        Value::Null => Map::new(),
        Value::Object(m) => m.clone(),
        _ => return Err(bad("options must be an object of name and value")),
    };
    for key in given.keys() {
        if !manifest.options.contains_key(key) {
            return Err(unknown(manifest, key));
        }
    }
    let mut out = Map::new();
    for (key, spec) in &manifest.options {
        let value = given.get(key).cloned().or_else(|| spec.default.clone());
        if let Some(v) = value {
            check(key, spec, &v)?;
            out.insert(key.clone(), v);
        }
    }
    Ok(Value::Object(out))
}

fn check(key: &str, spec: &OptionSpec, v: &Value) -> Result<(), ModuleError> {
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
                return Err(bad(format!("{key} needs a size like 2mb")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest() -> Manifest {
        Manifest::from_toml_str(
            r#"
accepts = ["text/*"]
[module]
id = "text.demo"
name = "Demo"
version = "0.1.0"
pattern = "A"
summary = "Demo."
[options.width]
type = "number"
default = 80
min = 10
max = 200
[options.mode]
type = "choice"
values = ["fast", "slow"]
default = "fast"
[options.loud]
type = "bool"
default = false
[options.note]
type = "text"
"#,
        )
        .unwrap()
    }

    #[test]
    fn defaults_fill_in_and_optional_stays_out() {
        let v = resolve(&manifest(), &Value::Null).unwrap();
        assert_eq!(v, json!({"width": 80, "mode": "fast", "loud": false}));
    }

    #[test]
    fn pairs_are_typed_by_the_manifest() {
        let m = manifest();
        let pairs = [
            "width=120".to_string(),
            "loud=yes".to_string(),
            "note=hi there".to_string(),
        ];
        let v = resolve(&m, &from_pairs(&m, &pairs).unwrap()).unwrap();
        assert_eq!(v["width"], 120);
        assert_eq!(v["loud"], true);
        assert_eq!(v["note"], "hi there");
    }

    #[test]
    fn unknown_wrong_and_out_of_range_are_refused_plainly() {
        let m = manifest();
        let err = from_pairs(&m, &["colour=red".into()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("not an option") && err.contains("width"));
        assert!(from_pairs(&m, &["width=wide".into()]).is_err());
        assert!(from_pairs(&m, &["nonsense".into()]).is_err());
        let low = resolve(&m, &json!({"width": 3})).unwrap_err().to_string();
        assert!(low.contains("between 10 and 200"));
        assert!(resolve(&m, &json!({"mode": "medium"})).is_err());
        assert!(resolve(&m, &json!([1])).is_err());
    }
}
