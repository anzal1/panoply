//! Estimate before running (FLOWS.md, shared capability 3).
//!
//! The module trait has no estimate hook yet, so the host reads what the manifest already says:
//! a size-target option (`under_mb`, `max_kb`, a `size` option called "Stay under") bounds the
//! result. When the host grows `Module::estimate` it takes precedence here. A module with no size
//! target shows no estimate, and the panel says nothing rather than guessing.

use panoply_manifest::{Manifest, OptionSpec, OptionType};
use serde_json::Value;

use crate::units::{format_bytes, parse_size, unit_in};

/// What a run is expected to produce.
#[derive(Debug, Clone, PartialEq)]
pub struct Estimate {
    /// Total size of the inputs, in bytes.
    pub before_bytes: u64,
    /// The most the output can weigh, in bytes.
    pub at_most_bytes: u64,
}

impl Estimate {
    /// "312 KB to at most 50 KB".
    pub fn text(&self) -> String {
        if self.at_most_bytes >= self.before_bytes {
            format!(
                "{} now, already under the limit",
                format_bytes(self.before_bytes)
            )
        } else {
            format!(
                "{} to at most {}",
                format_bytes(self.before_bytes),
                format_bytes(self.at_most_bytes)
            )
        }
    }
}

/// True for an option that names an upper bound on the output size.
fn is_size_bound(key: &str, spec: &OptionSpec) -> bool {
    let k = key.to_ascii_lowercase();
    let label = spec.label.clone().unwrap_or_default().to_ascii_lowercase();
    let bound_word = |s: &str| {
        s.starts_with("under")
            || s.starts_with("max")
            || s.starts_with("limit")
            || s.starts_with("largest")
            || s.starts_with("stay under")
    };
    match spec.kind {
        OptionType::Size => bound_word(&k) || bound_word(&label) || k.contains("size"),
        OptionType::Number => {
            (bound_word(&k) || bound_word(&label)) && unit_in(&format!("{k} {label}")).is_some()
        }
        _ => false,
    }
}

/// The option that carries a size target, as `(key, spec)`, when the manifest has one.
pub fn bound_option(manifest: &Manifest) -> Option<(&String, &OptionSpec)> {
    manifest.options.iter().find(|(k, s)| is_size_bound(k, s))
}

/// The option value that sets a size target of `bytes`: text for a `size` option, a number in the
/// option's own unit for a number option. `None` when the manifest has no size target.
pub fn bound_preset(manifest: &Manifest, bytes: u64) -> Option<(String, Value)> {
    let (key, spec) = bound_option(manifest)?;
    match spec.kind {
        OptionType::Size => Some((key.clone(), Value::String(format!("{bytes}")))),
        _ => {
            let unit = unit_in(&format!("{key} {}", spec.label.clone().unwrap_or_default()))?;
            Some((key.clone(), crate::form::number(bytes as f64 / unit)))
        }
    }
}

/// The byte limit in the options, when one of them is a size target and is set.
pub fn size_bound(manifest: &Manifest, options: &Value) -> Option<u64> {
    let obj = options.as_object()?;
    for (key, spec) in &manifest.options {
        if !is_size_bound(key, spec) {
            continue;
        }
        let Some(v) = obj.get(key) else { continue };
        let bytes = match (spec.kind, v) {
            (OptionType::Size, Value::String(s)) => parse_size(s),
            (OptionType::Size, Value::Number(n)) => n.as_f64().map(|n| n as u64),
            (OptionType::Number, Value::Number(n)) => {
                let unit = unit_in(&format!("{key} {}", spec.label.clone().unwrap_or_default()))?;
                n.as_f64().map(|n| (n * unit) as u64)
            }
            _ => None,
        };
        if let Some(b) = bytes.filter(|b| *b > 0) {
            return Some(b);
        }
    }
    None
}

/// The estimate for a run, when the options give one.
pub fn for_job(manifest: &Manifest, input_bytes: u64, options: &Value) -> Option<Estimate> {
    size_bound(manifest, options).map(|at_most_bytes| Estimate {
        before_bytes: input_bytes,
        at_most_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn catalog_manifest(id: &str) -> Manifest {
        let c = crate::catalog::Catalog::with_dirs(&[]);
        c.tool(c.index_of(id).unwrap()).manifest.clone()
    }

    #[test]
    fn a_pdf_under_option_bounds_the_result() {
        let m = catalog_manifest("pdf.compress");
        // under_mb is a number whose label says (MB).
        let e = for_job(&m, 4_200_000, &json!({"under_mb": 2})).unwrap();
        assert_eq!(e.at_most_bytes, 2_000_000);
        assert_eq!(e.text(), "4.2 MB to at most 2.0 MB");
    }

    #[test]
    fn a_size_option_is_parsed() {
        let m = catalog_manifest("image.compress");
        let e = for_job(&m, 312_000, &json!({"max_size": "50kb"})).unwrap();
        assert_eq!(e.text(), "312 KB to at most 50 KB");
        let small = for_job(&m, 30_000, &json!({"max_size": "50kb"})).unwrap();
        assert!(small.text().contains("already under"));
    }

    #[test]
    fn no_target_means_no_estimate() {
        let m = catalog_manifest("pdf.merge");
        assert!(for_job(&m, 1_000_000, &json!({})).is_none());
        let m = catalog_manifest("image.compress");
        assert!(for_job(&m, 1_000_000, &json!({"quality": 80})).is_none());
    }

    #[test]
    fn a_target_becomes_the_right_option_value() {
        let m = catalog_manifest("pdf.compress");
        assert_eq!(
            bound_preset(&m, 2_000_000),
            Some(("under_mb".to_string(), json!(2)))
        );
        let m = catalog_manifest("image.resize_to_kb");
        assert_eq!(
            bound_preset(&m, 50_000),
            Some(("max_kb".to_string(), json!(50)))
        );
        let m = catalog_manifest("pdf.merge");
        assert_eq!(bound_preset(&m, 50_000), None);
    }

    #[test]
    fn kb_windows_count_too() {
        let m = catalog_manifest("image.resize_to_kb");
        // max_kb is "Largest size (KB)".
        let e = for_job(&m, 312_000, &json!({"max_kb": 50})).unwrap();
        assert_eq!(e.at_most_bytes, 50_000);
    }
}
