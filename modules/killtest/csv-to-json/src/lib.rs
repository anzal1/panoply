wit_bindgen::generate!({ path: "wit", world: "module" });
use panoply::module::host;

struct Module;

impl Guest for Module {
    fn run(inputs: Vec<InputFile>, _options: String) -> Result<Vec<Output>, String> {
        let mut outputs = Vec::new();
        for (i, file) in inputs.iter().enumerate() {
            if host::cancelled() {
                return Err("Cancelled.".into());
            }
            host::progress(i as f32 / inputs.len() as f32, &format!("Converting {}", file.name));
            let bytes = host::read_all(file.handle)?;
            let json = csv_to_json(&bytes).map_err(|e| {
                format!(
                    "{} is not a readable CSV file: {e}. Check that every row has the same number of columns as the first row.",
                    file.name
                )
            })?;
            let name = host::save(Some(file.handle), "json", "json", json.as_bytes())?;
            outputs.push(Output::File(name));
        }
        Ok(outputs)
    }
}

/// First row is the header. Each later row becomes one object, keyed by the headers.
/// Values are kept as text, exactly as they appear in the file.
fn csv_to_json(bytes: &[u8]) -> Result<String, String> {
    let mut reader = csv::Reader::from_reader(bytes);
    let headers = reader.headers().map_err(|e| e.to_string())?.clone();
    let mut rows: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| e.to_string())?;
        let mut object = serde_json::Map::new();
        for (header, value) in headers.iter().zip(record.iter()) {
            object.insert(header.to_string(), serde_json::Value::String(value.to_string()));
        }
        rows.push(object);
    }
    serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())
}

export!(Module);
