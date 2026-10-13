//! MCP tool definitions generated from module manifests.
//!
//! Every module is an MCP tool from day one, with no extra work for the module author. This
//! crate only builds the definitions. There is no transport yet.
//!
//! The tool name is the module id with dots turned into underscores (`pdf.compress` becomes
//! `pdf_compress`). The description is the manifest summary. The input schema has a `files`
//! array, an optional `output_dir` for modules that write files, and one property per option.

use panoply_manifest::{Manifest, OptionSpec, OptionType};
use serde_json::{Map, Value, json};

/// The MCP tool name for a module id.
pub fn tool_name(module_id: &str) -> String {
    module_id.replace(['.', '-'], "_")
}

/// Build the MCP tool definition for one module.
pub fn tool_for(manifest: &Manifest) -> Value {
    let mut props = Map::new();
    let mut required = Vec::new();

    if !manifest.accepts.is_empty() {
        props.insert(
            "files".into(),
            json!({
                "type": "array",
                "items": { "type": "string" },
                "minItems": 1,
                "description": format!("Paths of the files to process. Accepts: {}.", manifest.accepts.join(", ")),
            }),
        );
        required.push("files");
    }
    if !manifest.outputs.is_empty() {
        props.insert(
            "output_dir".into(),
            json!({
                "type": "string",
                "description": "Folder for the results. Defaults to next to each original. Originals are never overwritten.",
            }),
        );
    }
    for (key, opt) in &manifest.options {
        props.insert(key.clone(), option_schema(opt));
    }

    json!({
        "name": tool_name(&manifest.module.id),
        "description": manifest.module.summary,
        "inputSchema": {
            "type": "object",
            "properties": props,
            "required": required,
        },
    })
}

/// Tool definitions for every manifest that has the MCP surface on, in the order given.
pub fn tools<'a>(manifests: impl IntoIterator<Item = &'a Manifest>) -> Vec<Value> {
    manifests
        .into_iter()
        .filter(|m| m.surfaces.mcp)
        .map(tool_for)
        .collect()
}

fn option_schema(opt: &OptionSpec) -> Value {
    let mut s = Map::new();
    match opt.kind {
        OptionType::Number => {
            s.insert("type".into(), json!("number"));
            if let Some(min) = opt.min {
                s.insert("minimum".into(), json!(min));
            }
            if let Some(max) = opt.max {
                s.insert("maximum".into(), json!(max));
            }
        }
        OptionType::Choice => {
            s.insert("type".into(), json!("string"));
            s.insert("enum".into(), json!(opt.values));
        }
        OptionType::Bool => {
            s.insert("type".into(), json!("boolean"));
        }
        OptionType::Text => {
            s.insert("type".into(), json!("string"));
        }
        OptionType::Size => {
            s.insert("type".into(), json!(["string", "number"]));
        }
        OptionType::File => {
            s.insert("type".into(), json!("string"));
        }
    }
    if let Some(d) = &opt.default {
        s.insert("default".into(), d.clone());
    }
    let description = match (&opt.label, &opt.help, opt.kind) {
        (_, Some(h), OptionType::Size) => format!("{h} A size like \"2mb\" or a number of bytes."),
        (_, Some(h), _) => h.clone(),
        (Some(l), None, _) => l.clone(),
        (None, None, OptionType::Size) => "A size like \"2mb\" or a number of bytes.".into(),
        (None, None, _) => String::new(),
    };
    if !description.is_empty() {
        s.insert("description".into(), json!(description));
    }
    Value::Object(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn example(dir: &str) -> Manifest {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../modules/examples")
            .join(dir)
            .join("module.toml");
        Manifest::from_file(&p).unwrap()
    }

    #[test]
    fn name_swaps_dots_for_underscores() {
        assert_eq!(tool_name("pdf.compress"), "pdf_compress");
        assert_eq!(tool_name("image.resize_to_kb"), "image_resize_to_kb");
    }

    #[test]
    fn compress_pdf_tool_shape() {
        let t = tool_for(&example("compress-pdf"));
        assert_eq!(t["name"], "pdf_compress");
        assert_eq!(
            t["description"],
            "Make this PDF small enough to email or upload."
        );
        let schema = &t["inputSchema"];
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["files"]));
        let p = &schema["properties"];
        assert_eq!(p["target"]["enum"], json!(["smallest", "size", "quality"]));
        assert_eq!(p["target"]["default"], "smallest");
        assert_eq!(p["keep_text_selectable"]["type"], "boolean");
        assert_eq!(p["keep_text_selectable"]["default"], true);
        assert_eq!(p["grayscale"]["default"], false);
        assert_eq!(p["under_mb"]["type"], json!(["string", "number"]));
        assert!(p["output_dir"].is_object());
    }

    #[test]
    fn resize_tool_carries_number_limits() {
        let t = tool_for(&example("resize-to-kb"));
        assert_eq!(t["name"], "image_resize_to_kb");
        let p = &t["inputSchema"]["properties"];
        assert_eq!(p["target_kb"]["type"], "number");
        assert_eq!(p["target_kb"]["minimum"], 5.0);
        assert_eq!(p["target_kb"]["default"], 50);
        assert_eq!(p["kb_basis"]["enum"], json!(["1000", "1024", "both"]));
        assert!(p["width"].get("default").is_none());
    }

    #[test]
    fn read_only_module_has_no_output_dir() {
        let t = tool_for(&example("hash"));
        assert_eq!(t["name"], "files_hash");
        assert!(t["inputSchema"]["properties"].get("output_dir").is_none());
    }

    #[test]
    fn tools_skips_modules_with_mcp_off() {
        let mut off = example("hash");
        off.surfaces.mcp = false;
        let on = example("compress-pdf");
        let list = tools([&off, &on]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["name"], "pdf_compress");
    }
}
