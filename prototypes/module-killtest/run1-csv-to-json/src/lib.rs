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
            let text = String::from_utf8_lossy(&host::read_all(file.handle)?).into_owned();
            let json = csv_to_json(&text).map_err(|e| format!("{}: {e}", file.name))?;
            let name = host::save(Some(file.handle), "json", "json", json.as_bytes())?;
            outputs.push(Output::File(name));
        }
        Ok(outputs)
    }
}

/// Converts CSV text to a pretty-printed JSON array. The first record is the headers.
fn csv_to_json(text: &str) -> Result<String, String> {
    // Keep each record's number (the header is record 1) so errors can name the row.
    let mut records = parse_csv(text)?
        .into_iter()
        .enumerate()
        .map(|(i, fields)| (i + 1, fields))
        .filter(|(_, fields)| !is_blank(fields));

    let (_, headers) = records
        .next()
        .ok_or("The file is empty. The first line must be the column headers.")?;
    for (i, header) in headers.iter().enumerate() {
        if header.is_empty() {
            return Err(format!("Column {} has no name in the header row. Name it or remove it.", i + 1));
        }
        if headers[..i].contains(header) {
            return Err(format!("The header \"{header}\" appears twice. Rename one of the columns."));
        }
    }

    let mut objects = Vec::new();
    for (number, fields) in records {
        if fields.len() > headers.len() {
            return Err(format!(
                "Row {number} has {} values but the header has {}. Fix that row so it matches the header.",
                fields.len(),
                headers.len()
            ));
        }
        let mut object = serde_json::Map::new();
        for (i, header) in headers.iter().enumerate() {
            // A short row leaves its missing columns as empty text.
            let value = fields.get(i).cloned().unwrap_or_default();
            object.insert(header.clone(), serde_json::Value::String(value));
        }
        objects.push(serde_json::Value::Object(object));
    }
    serde_json::to_string_pretty(&serde_json::Value::Array(objects)).map_err(|e| e.to_string())
}

/// A blank line parses as one empty field. Skip it.
fn is_blank(fields: &[String]) -> bool {
    fields.len() == 1 && fields[0].is_empty()
}

/// Splits CSV text into records of fields. Handles quoted fields, "" for a literal quote,
/// commas and line breaks inside quotes, and LF or CRLF line endings.
fn parse_csv(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else {
            match c {
                '"' if field.is_empty() => in_quotes = true,
                ',' => record.push(std::mem::take(&mut field)),
                '\r' => {}
                '\n' => {
                    record.push(std::mem::take(&mut field));
                    records.push(std::mem::take(&mut record));
                }
                _ => field.push(c),
            }
        }
    }

    if in_quotes {
        return Err("A quoted value is never closed. Add the missing closing \" quote.".into());
    }
    // The last line may have no line break after it.
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    Ok(records)
}

export!(Module);
