//! QR code maker: the links or text in a file become QR codes, as PNG or SVG.

mod make;

wit_bindgen::generate!({ path: "wit", world: "module" });
use panoply::module::host;

struct Module;

/// A phone-friendly cap so one stray huge file cannot write thousands of codes.
const MAX_CODES: usize = 500;

fn num(v: &serde_json::Value, key: &str, default: usize) -> usize {
    v[key].as_f64().map_or(default, |n| n.max(0.0) as usize)
}

impl Guest for Module {
    fn run(inputs: Vec<InputFile>, options: String) -> Result<Vec<Output>, String> {
        let opts: serde_json::Value = serde_json::from_str(&options).map_err(|e| e.to_string())?;
        let svg = opts["format"].as_str() == Some("svg");
        let whole = opts["lines"].as_str() == Some("whole_file");
        let ec = make::level(opts["error_correction"].as_str().unwrap_or("M"));
        let scale = num(&opts, "scale", 10).clamp(2, 40);
        let border = num(&opts, "border", 4).min(16);
        let dark = make::parse_colour(opts["dark"].as_str().unwrap_or("#000000"))?;
        let light = make::parse_colour(opts["light"].as_str().unwrap_or("#ffffff"))?;
        make::check_contrast(dark, light)?;

        let mut out = Vec::new();
        let total = inputs.len().max(1) as f32;
        for (i, file) in inputs.iter().enumerate() {
            let bytes = host::read_all(file.handle)?;
            let text = String::from_utf8(bytes)
                .map_err(|_| format!("{} is not text (it is not valid UTF-8). Pick a text file with the link on a line.", file.name))?;
            let text = text.trim_start_matches('\u{feff}');
            let items: Vec<String> = if whole {
                vec![text.trim_end_matches(['\r', '\n']).to_string()]
            } else {
                text.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect()
            };
            if items.iter().all(|s| s.trim().is_empty()) {
                return Err(format!(
                    "{} is empty. Put a link or some text in it, one per line.",
                    file.name
                ));
            }
            if items.len() > MAX_CODES {
                return Err(format!(
                    "{} has {} lines, and one run makes at most {MAX_CODES} codes. Split the file.",
                    file.name,
                    items.len()
                ));
            }
            let many = items.len() > 1;
            for (n, item) in items.iter().enumerate() {
                if host::cancelled() {
                    return Err("Cancelled.".into());
                }
                host::progress(
                    (i as f32 + n as f32 / items.len() as f32) / total,
                    &file.name,
                );
                let m = make::encode(item, ec).map_err(|e| {
                    if many {
                        format!("Line {}: {e}", n + 1)
                    } else {
                        e
                    }
                })?;
                let (data, ext) = if svg {
                    (make::svg(&m, border, dark, light).into_bytes(), "svg")
                } else {
                    (make::png(&m, border, scale, dark, light)?, "png")
                };
                let label = if many {
                    format!("qr {}", n + 1)
                } else {
                    "qr".to_string()
                };
                out.push(Output::File(host::save(
                    Some(file.handle),
                    &label,
                    ext,
                    &data,
                )?));
            }
        }
        Ok(out)
    }
}

export!(Module);
