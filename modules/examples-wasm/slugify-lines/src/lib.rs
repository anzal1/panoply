wit_bindgen::generate!({ path: "wit", world: "module" });
use panoply::module::host;

struct Module;

/// "Hello, World!" becomes "hello-world".
fn slug(line: &str, sep: char) -> String {
    let words: Vec<String> = line
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    words.join(&sep.to_string())
}

impl Guest for Module {
    fn run(inputs: Vec<InputFile>, options: String) -> Result<Vec<Output>, String> {
        let options: serde_json::Value = serde_json::from_str(&options).map_err(|e| e.to_string())?;
        let sep = options["separator"].as_str().unwrap_or("-").chars().next().unwrap_or('-');
        let mut outputs = Vec::new();
        for file in &inputs {
            let text = String::from_utf8_lossy(&host::read_all(file.handle)?).into_owned();
            let slugs: Vec<String> = text.lines().map(|l| slug(l, sep)).collect();
            let name = host::save(Some(file.handle), "slugified", "txt", slugs.join("\n").as_bytes())?;
            outputs.push(Output::File(name));
        }
        Ok(outputs)
    }
}

export!(Module);
