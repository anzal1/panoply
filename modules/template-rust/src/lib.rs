wit_bindgen::generate!({ path: "wit", world: "module" });
use panoply::module::host;

struct Module;

impl Guest for Module {
    fn run(inputs: Vec<InputFile>, options: String) -> Result<Vec<Output>, String> {
        let options: serde_json::Value = serde_json::from_str(&options).map_err(|e| e.to_string())?;
        let end = options["separator"].as_str().unwrap_or("");
        let mut outputs = Vec::new();
        for (i, file) in inputs.iter().enumerate() {
            if host::cancelled() {
                return Err("Cancelled.".into());
            }
            host::progress(i as f32 / inputs.len() as f32, &format!("Reading {}", file.name));
            let text = String::from_utf8_lossy(&host::read_all(file.handle)?).to_uppercase();
            let text: String = text.lines().map(|l| format!("{l}{end}\n")).collect();
            let name = host::save(Some(file.handle), "uppercase", "txt", text.as_bytes())?;
            outputs.push(Output::File(name));
        }
        Ok(outputs)
    }
}

export!(Module);
