//! Markdown to one self-contained HTML page. Opens in any browser and prints cleanly to PDF.

mod render;

wit_bindgen::generate!({ path: "wit", world: "module" });
use panoply::module::host;

use render::Settings;

struct Module;

/// "notes.md" becomes "notes".
fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

impl Guest for Module {
    fn run(inputs: Vec<InputFile>, options: String) -> Result<Vec<Output>, String> {
        let opts: serde_json::Value = serde_json::from_str(&options).map_err(|e| e.to_string())?;
        let settings = Settings {
            theme: opts["theme"].as_str().unwrap_or("auto").to_string(),
            paper: opts["paper"].as_str().unwrap_or("A4").to_string(),
            title: opts["title"].as_str().unwrap_or("").to_string(),
            toc: opts["toc"].as_bool().unwrap_or(false),
            page: opts["page"].as_bool().unwrap_or(true),
        };
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
                    "{} is not text (it is not valid UTF-8). Save it as UTF-8 and try again.",
                    file.name
                )
            })?;
            let html = render::convert(&text, stem(&file.name), &settings);
            out.push(Output::File(host::save(
                Some(file.handle),
                "html",
                "html",
                html.as_bytes(),
            )?));
        }
        Ok(out)
    }
}

export!(Module);
