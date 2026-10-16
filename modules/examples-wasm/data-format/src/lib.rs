//! Format JSON, YAML and TOML: tidy, convert, or check. One run takes any number of files.

mod convert;

wit_bindgen::generate!({ path: "wit", world: "module" });
use panoply::module::host;

use convert::{Format, Indent};

struct Module;

impl Guest for Module {
    fn run(inputs: Vec<InputFile>, options: String) -> Result<Vec<Output>, String> {
        let opts: serde_json::Value = serde_json::from_str(&options).map_err(|e| e.to_string())?;
        let to = opts["to"].as_str().unwrap_or("same");
        let indent = Indent::parse(opts["indent"].as_str().unwrap_or("2"));
        let sort = opts["sort_keys"].as_bool().unwrap_or(false);
        let check_only = opts["check_only"].as_bool().unwrap_or(false);

        let mut out = Vec::new();
        let total = inputs.len().max(1) as f32;
        for (i, file) in inputs.iter().enumerate() {
            if host::cancelled() {
                return Err("Cancelled.".into());
            }
            host::progress(i as f32 / total, &file.name);
            let bytes = host::read_all(file.handle)?;
            let text = String::from_utf8(bytes).map_err(|_| {
                format!(
                    "{} is not text (it is not valid UTF-8). Pick a JSON, YAML or TOML file.",
                    file.name
                )
            })?;
            let from = convert::detect(&file.name, &text);
            let mut value = convert::parse(from, &text)
                .map_err(|e| format!("{} is not valid {}. {e}", file.name, from.label()))?;
            if check_only {
                out.push(Output::Note(format!(
                    "{}: valid {}",
                    file.name,
                    from.label()
                )));
                continue;
            }
            if sort {
                convert::sort_keys(&mut value);
            }
            let target = Format::parse_name(to).unwrap_or(from);
            let written = convert::write(target, &value, indent)
                .map_err(|e| format!("{}: {e}", file.name))?;
            let label = if target == from {
                "formatted"
            } else {
                target.ext()
            };
            out.push(Output::File(host::save(
                Some(file.handle),
                label,
                target.ext(),
                written.as_bytes(),
            )?));
        }
        Ok(out)
    }
}

export!(Module);
