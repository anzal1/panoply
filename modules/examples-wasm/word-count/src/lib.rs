wit_bindgen::generate!({ path: "wit", world: "module" });
use panoply::module::host;

struct Module;

impl Guest for Module {
    fn run(inputs: Vec<InputFile>, _options: String) -> Result<Vec<Output>, String> {
        let mut report = Vec::new();
        for file in &inputs {
            let bytes = host::read_all(file.handle)?;
            let text = String::from_utf8_lossy(&bytes);
            let (lines, words, chars) = (
                text.lines().count(),
                text.split_whitespace().count(),
                text.chars().count(),
            );
            report.push(Output::Note(format!(
                "{}: {lines} lines, {words} words, {chars} characters, {} bytes",
                file.name,
                bytes.len()
            )));
        }
        Ok(report)
    }
}

export!(Module);
