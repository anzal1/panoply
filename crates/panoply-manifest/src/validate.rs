//! Plain-language validation of a parsed manifest.

use crate::schema::{FilesAccess, Manifest, OptionSpec, OptionType};

/// Licenses a model may carry. Mirrors the clean-license rule in DECISIONS.md: permissive
/// only, no research-only, non-commercial or unclear terms.
pub const ALLOWED_MODEL_LICENSES: &[&str] = &[
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "CC0-1.0",
    "Unlicense",
];

impl Manifest {
    /// Check the manifest against the v0 rules. Returns every problem found, each written for
    /// a module author rather than a parser.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        self.check_module(&mut errs);
        self.check_io(&mut errs);
        for (key, opt) in &self.options {
            check_option(key, opt, &mut errs);
        }
        self.check_capabilities(&mut errs);
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }

    fn check_module(&self, errs: &mut Vec<String>) {
        let m = &self.module;
        if !valid_id(&m.id) {
            errs.push(format!(
                "The module id \"{}\" is not valid. Use lowercase words joined by dots, like \
                 \"pdf.compress\" (letters, digits and underscores only).",
                m.id
            ));
        }
        if m.name.trim().is_empty() {
            errs.push("The module needs a name.".into());
        }
        if !valid_version(&m.version) {
            errs.push(format!(
                "The version \"{}\" is not valid. Use three numbers like \"0.1.0\".",
                m.version
            ));
        }
        if m.summary.trim().is_empty() {
            errs.push("The module needs a one-sentence summary.".into());
        } else if m.summary.chars().count() > 140 {
            errs.push(
                "The summary is too long. Keep it to one sentence under 140 characters.".into(),
            );
        }
    }

    fn check_io(&self, errs: &mut Vec<String>) {
        if self.accepts.is_empty() && self.capabilities.files == FilesAccess::Chosen {
            errs.push(
                "The module has file access but accepts no files. List what it accepts, like \
                 accepts = [\"image/*\"], or set files = \"none\" in [capabilities]."
                    .into(),
            );
        }
        for (field, list) in [("accepts", &self.accepts), ("outputs", &self.outputs)] {
            for entry in list {
                if !valid_type_pattern(entry) {
                    errs.push(format!(
                        "\"{entry}\" in {field} is not a MIME type (image/png, image/*) or an \
                         extension glob (*.heic)."
                    ));
                }
            }
        }
    }

    fn check_capabilities(&self, errs: &mut Vec<String>) {
        let c = &self.capabilities;
        if c.network
            && c.network_reason
                .as_deref()
                .is_none_or(|r| r.trim().is_empty())
        {
            errs.push(
                "This module asks for network access but gives no reason. Add network_reason, \
                 because the user sees it before the module runs."
                    .into(),
            );
        }
        for model in &c.models {
            if model.id.trim().is_empty() {
                errs.push("A model has no id.".into());
            }
            if model.size_mb == 0 {
                errs.push(format!(
                    "Model \"{}\" needs its download size in megabytes, shown before the first \
                     download.",
                    model.id
                ));
            }
            if !ALLOWED_MODEL_LICENSES.contains(&model.license.as_str()) {
                errs.push(format!(
                    "Model \"{}\" has the license \"{}\". Panoply ships only models under clean \
                     licenses: {}.",
                    model.id,
                    model.license,
                    ALLOWED_MODEL_LICENSES.join(", ")
                ));
            }
        }
    }
}

fn check_option(key: &str, opt: &OptionSpec, errs: &mut Vec<String>) {
    if !valid_key(key) {
        errs.push(format!(
            "The option name \"{key}\" is not valid. Use lowercase letters, digits and underscores."
        ));
    }
    if opt.kind != OptionType::Choice && !opt.values.is_empty() {
        errs.push(format!(
            "Option \"{key}\" lists values but is not a choice."
        ));
    }
    if opt.kind != OptionType::Number && (opt.min.is_some() || opt.max.is_some()) {
        errs.push(format!(
            "Option \"{key}\" has min or max but is not a number."
        ));
    }
    match opt.kind {
        OptionType::Choice => {
            if opt.values.is_empty() {
                errs.push(format!("Option \"{key}\" is a choice but lists no values."));
            }
            let mut seen = std::collections::HashSet::new();
            for v in &opt.values {
                if !seen.insert(v) {
                    errs.push(format!("Option \"{key}\" lists the value \"{v}\" twice."));
                }
            }
            if let Some(d) = &opt.default {
                match d.as_str() {
                    Some(s) if opt.values.iter().any(|v| v == s) => {}
                    _ => errs.push(format!(
                        "The default for \"{key}\" ({d}) is not one of its values: {}.",
                        opt.values.join(", ")
                    )),
                }
            }
        }
        OptionType::Number => {
            if let (Some(lo), Some(hi)) = (opt.min, opt.max)
                && lo > hi
            {
                errs.push(format!("Option \"{key}\" has min {lo} above max {hi}."));
            }
            if let Some(d) = &opt.default {
                match d.as_f64() {
                    None => errs.push(format!("The default for \"{key}\" must be a number.")),
                    Some(n) => {
                        if opt.min.is_some_and(|lo| n < lo) || opt.max.is_some_and(|hi| n > hi) {
                            errs.push(format!(
                                "The default for \"{key}\" ({n}) is outside its min and max."
                            ));
                        }
                    }
                }
            }
        }
        OptionType::Bool => {
            if opt.default.as_ref().is_some_and(|d| !d.is_boolean()) {
                errs.push(format!("The default for \"{key}\" must be true or false."));
            }
        }
        OptionType::Text | OptionType::File => {
            if opt.default.as_ref().is_some_and(|d| !d.is_string()) {
                errs.push(format!("The default for \"{key}\" must be text."));
            }
        }
        OptionType::Size => {
            if opt
                .default
                .as_ref()
                .is_some_and(|d| !d.is_string() && !d.is_number())
            {
                errs.push(format!(
                    "The default for \"{key}\" must be a size like \"2mb\" or a number of bytes."
                ));
            }
        }
    }
}

fn valid_id(id: &str) -> bool {
    id.contains('.')
        && id.split('.').all(|seg| {
            !seg.is_empty()
                && seg
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn valid_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

fn valid_type_pattern(p: &str) -> bool {
    if p.chars().any(char::is_whitespace) {
        return false;
    }
    if p == "*/*" {
        return true;
    }
    if let Some(ext) = p.strip_prefix("*.") {
        return !ext.is_empty() && !ext.contains(['*', '/', '.']);
    }
    match p.split_once('/') {
        Some((kind, sub)) => !kind.is_empty() && !sub.is_empty() && !kind.contains('*'),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Manifest;
    use std::path::PathBuf;

    fn base() -> Manifest {
        Manifest::from_toml_str(
            r#"
accepts = ["image/*"]
[module]
id = "image.demo"
name = "Demo"
version = "0.1.0"
pattern = "A"
summary = "Does a demo."
"#,
        )
        .unwrap()
    }

    fn errors(m: &Manifest) -> String {
        m.validate().err().map(|e| e.join("\n")).unwrap_or_default()
    }

    #[test]
    fn minimal_is_valid() {
        assert!(base().validate().is_ok());
    }

    #[test]
    fn network_needs_a_reason() {
        let mut m = base();
        m.capabilities.network = true;
        assert!(errors(&m).contains("gives no reason"));
        m.capabilities.network_reason = Some("Downloads exchange rates".into());
        assert!(m.validate().is_ok());
    }

    #[test]
    fn model_license_must_be_clean() {
        let mut m = base();
        m.capabilities.models.push(crate::ModelSpec {
            id: "birefnet".into(),
            size_mb: 42,
            license: "CC-BY-NC-4.0".into(),
        });
        let e = errors(&m);
        assert!(e.contains("clean licenses"), "{e}");
        m.capabilities.models[0].license = "MIT".into();
        assert!(m.validate().is_ok());
    }

    #[test]
    fn bad_id_and_version_are_explained() {
        let mut m = base();
        m.module.id = "Demo".into();
        m.module.version = "1".into();
        let e = errors(&m);
        assert!(e.contains("module id"));
        assert!(e.contains("three numbers"));
    }

    #[test]
    fn choice_default_must_be_a_value() {
        let m = Manifest::from_toml_str(
            r#"
accepts = ["image/*"]
[module]
id = "image.demo"
name = "Demo"
version = "0.1.0"
pattern = "B"
summary = "Does a demo."
[options.format]
type = "choice"
values = ["jpg", "png"]
default = "gif"
"#,
        )
        .unwrap();
        assert!(errors(&m).contains("not one of its values"));
    }

    #[test]
    fn number_default_outside_range_is_flagged() {
        let m = Manifest::from_toml_str(
            r#"
accepts = ["image/*"]
[module]
id = "image.demo"
name = "Demo"
version = "0.1.0"
pattern = "B"
summary = "Does a demo."
[options.kb]
type = "number"
default = 1
min = 5
"#,
        )
        .unwrap();
        assert!(errors(&m).contains("outside its min and max"));
    }

    #[test]
    fn files_without_accepts_is_flagged_unless_none() {
        let mut m = base();
        m.accepts.clear();
        assert!(errors(&m).contains("accepts no files"));
        m.capabilities.files = FilesAccess::None;
        assert!(m.validate().is_ok());
    }

    #[test]
    fn bad_accept_pattern_is_flagged() {
        let mut m = base();
        m.accepts = vec!["pdf".into()];
        assert!(errors(&m).contains("not a MIME type"));
    }

    #[test]
    fn every_example_manifest_parses_and_validates() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/examples");
        let mut count = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path().join("module.toml");
            if !path.exists() {
                continue;
            }
            let m =
                Manifest::from_file(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            if let Err(errs) = m.validate() {
                panic!("{}: {}", path.display(), errs.join("; "));
            }
            count += 1;
        }
        assert!(
            count >= 3,
            "expected at least 3 example manifests, found {count}"
        );
    }
}
