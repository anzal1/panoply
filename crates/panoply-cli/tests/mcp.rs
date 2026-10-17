//! End to end checks of `panoply mcp`: the real binary, spoken to over stdio the way Claude Code,
//! Claude Desktop and Cursor speak to it.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{Value, json};

/// A running `panoply mcp` and the messages it has sent.
struct Server {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: i64,
    /// Notifications that arrived while waiting for a reply.
    notifications: Vec<Value>,
}

impl Server {
    fn start(data: &Path, modules: Option<&Path>) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_panoply"));
        cmd.arg("mcp")
            .env("PANOPLY_DATA_DIR", data)
            // The examples in the source tree are not compiled here. Community modules come in
            // only through PANOPLY_MODULES, where a test asks for them.
            .env("PANOPLY_NO_SOURCE_TREE", "1")
            .env_remove("PANOPLY_MODULES")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(m) = modules {
            cmd.env("PANOPLY_MODULES", m);
        }
        let mut child = cmd.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Server {
            child,
            stdin,
            lines,
            next_id: 1,
            notifications: Vec::new(),
        }
    }

    fn send(&mut self, msg: &Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method }));
    }

    /// Send a request and wait for its reply (the whole message, `result` or `error`).
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(120))
                .expect("the server did not answer in time");
            let msg: Value = serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("stdout must hold only JSON messages: {e}: {line}"));
            assert_eq!(msg["jsonrpc"], "2.0", "{msg}");
            if msg.get("id") == Some(&json!(id)) {
                return msg;
            }
            assert!(msg.get("method").is_some(), "unexpected message: {msg}");
            self.notifications.push(msg);
        }
    }

    fn initialize(&mut self) -> Value {
        let reply = self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "panoply-test", "version": "0" },
            }),
        );
        self.notify("notifications/initialized");
        reply
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments, "_meta": { "progressToken": "job" } }),
        )
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // Closing stdin ends the server. Kill only as a backstop, and only our own child.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("panoply-mcp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A PDF with `pages` blank pages, classic cross-reference table.
fn blank_pdf(pages: usize) -> Vec<u8> {
    let mut objs: Vec<String> = vec!["<< /Type /Catalog /Pages 2 0 R >>".into()];
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 3 + i)).collect();
    objs.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {pages} >>",
        kids.join(" ")
    ));
    for _ in 0..pages {
        objs.push("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] >>".into());
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn page_count(path: &Path) -> usize {
    let file = std::fs::File::open(path).unwrap();
    panoply_pdf::open(&file).unwrap().doc.pages().unwrap().len()
}

/// A structural check of a tool's input schema: the parts of JSON Schema the generator uses, and
/// that every default satisfies its own schema.
fn check_schema(tool: &Value) {
    let name = tool["name"].as_str().unwrap();
    assert!(
        !name.is_empty()
            && name.len() <= 64
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
        "bad tool name {name}"
    );
    assert!(
        tool["description"].as_str().is_some_and(|d| !d.is_empty()),
        "{name} needs a description"
    );
    let schema = &tool["inputSchema"];
    assert_eq!(schema["type"], "object", "{name}");
    let props = schema["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("{name}: properties must be an object"));
    for req in schema
        .get("required")
        .map(|r| r.as_array().unwrap().clone())
        .unwrap_or_default()
    {
        assert!(props.contains_key(req.as_str().unwrap()), "{name}: {req}");
    }
    for (key, p) in props {
        let types: Vec<&str> = match &p["type"] {
            Value::String(t) => vec![t.as_str()],
            Value::Array(a) => a.iter().map(|t| t.as_str().unwrap()).collect(),
            other => panic!("{name}.{key}: type must be text or a list, got {other}"),
        };
        assert!(
            types
                .iter()
                .all(|t| ["string", "number", "integer", "boolean", "array", "object"].contains(t)),
            "{name}.{key}: {types:?}"
        );
        if let Some(e) = p.get("enum") {
            assert!(
                !e.as_array().unwrap().is_empty(),
                "{name}.{key}: empty enum"
            );
        }
        if let Some(d) = p.get("default") {
            let ok = types.iter().any(|t| match *t {
                "string" => d.is_string(),
                "number" | "integer" => d.is_number(),
                "boolean" => d.is_boolean(),
                _ => false,
            });
            assert!(ok, "{name}.{key}: default {d} does not match {types:?}");
            if let Some(e) = p.get("enum") {
                assert!(e.as_array().unwrap().contains(d), "{name}.{key}: {d}");
            }
            if let (Some(n), Some(min)) = (d.as_f64(), p.get("minimum").and_then(Value::as_f64)) {
                assert!(n >= min, "{name}.{key}: default under minimum");
            }
            if let (Some(n), Some(max)) = (d.as_f64(), p.get("maximum").and_then(Value::as_f64)) {
                assert!(n <= max, "{name}.{key}: default over maximum");
            }
        }
    }
}

/// One server test at a time: each starts a real process and some build a module.
fn one_at_a_time() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

fn text_of(reply: &Value) -> String {
    reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn initialize_lists_every_builtin_and_merges_two_pdfs() {
    let _g = one_at_a_time();
    let d = scratch("merge");
    let mut server = Server::start(&d.join("data"), None);

    // initialize
    let init = server.initialize();
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "{init}");
    assert_eq!(init["result"]["serverInfo"]["name"], "panoply");
    assert!(init["result"]["capabilities"]["tools"].is_object());

    // tools/list: every built-in module, each with a valid schema.
    let list = server.request("tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for t in tools {
        check_schema(t);
    }
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len(), "tool names must be unique");
    let builtins = panoply_host::Registry::with_builtins();
    assert!(builtins.len() >= 30, "expected the full built-in set");
    for m in builtins.list() {
        let id = &m.manifest().module.id;
        let tool = panoply_mcp::tool_name(id);
        assert!(
            names.contains(&tool.as_str()),
            "{id} is not listed as {tool}"
        );
    }
    println!("tools/list: {} tools, all with valid schemas", tools.len());

    // tools/call: merge two PDFs of 2 and 3 pages.
    let a = d.join("a.pdf");
    let b = d.join("b.pdf");
    std::fs::write(&a, blank_pdf(2)).unwrap();
    std::fs::write(&b, blank_pdf(3)).unwrap();
    let (a_bytes, b_bytes) = (std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
    let out_dir = d.join("out");
    let reply = server.call(
        "pdf_merge",
        json!({ "files": [a, b], "output_dir": out_dir, "order": "given" }),
    );
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let outputs = reply["result"]["structuredContent"]["outputs"]
        .as_array()
        .unwrap();
    assert_eq!(outputs.len(), 1, "{reply}");
    let merged = PathBuf::from(outputs[0].as_str().unwrap());
    // macOS reports /private/var for /var: compare real paths.
    let real_out = out_dir.canonicalize().unwrap();
    assert!(
        merged.is_absolute() && merged.starts_with(&real_out),
        "{merged:?}"
    );
    assert!(text_of(&reply).contains(merged.to_str().unwrap()));
    assert_eq!(page_count(&merged), 5);
    println!(
        "pdf_merge: {} pages + {} pages -> {} pages at {}",
        2,
        3,
        page_count(&merged),
        merged.display()
    );

    // The inputs are untouched and are not the output.
    assert_eq!(std::fs::read(&a).unwrap(), a_bytes);
    assert_eq!(std::fs::read(&b).unwrap(), b_bytes);

    // A second call into the same folder makes a new file instead of overwriting the first.
    let again = server.call(
        "pdf_merge",
        json!({ "files": [a, b], "output_dir": out_dir }),
    );
    assert_eq!(again["result"]["isError"], false, "{again}");
    let second = PathBuf::from(
        again["result"]["structuredContent"]["outputs"][0]
            .as_str()
            .unwrap(),
    );
    assert_ne!(second, merged);
    assert!(merged.is_file() && second.is_file());

    drop(server);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn bad_calls_are_refused_in_plain_words() {
    let _g = one_at_a_time();
    let d = scratch("bad");
    let mut server = Server::start(&d.join("data"), None);
    server.initialize();
    let real = d.join("real.pdf");
    std::fs::write(&real, blank_pdf(1)).unwrap();

    let missing = d.join("missing.pdf");
    let r = server.call("pdf_merge", json!({ "files": [real, missing] }));
    assert_eq!(r["result"]["isError"], true, "{r}");
    assert!(
        text_of(&r).contains("does not exist") && text_of(&r).contains("missing.pdf"),
        "{r}"
    );

    let r = server.call("pdf_merge", json!({ "files": ["relative/a.pdf"] }));
    assert_eq!(r["result"]["isError"], true);
    assert!(text_of(&r).contains("not absolute"), "{r}");

    let r = server.call("pdf_merge", json!({ "files": [d] }));
    assert_eq!(r["result"]["isError"], true);
    assert!(text_of(&r).contains("folder"), "{r}");

    let r = server.call("pdf_merge", json!({}));
    assert_eq!(r["result"]["isError"], true);
    assert!(text_of(&r).contains("at least one file"), "{r}");

    let r = server.call("pdf_merge", json!({ "files": [real], "colour": "red" }));
    assert_eq!(r["result"]["isError"], true);
    assert!(text_of(&r).contains("no option called colour"), "{r}");

    let r = server.call("pdf_merge", json!({ "files": [real], "order": "sideways" }));
    assert_eq!(r["result"]["isError"], true, "{r}");

    let r = server.call("no_such_tool", json!({}));
    assert_eq!(r["error"]["code"], -32602, "{r}");

    // Nothing was written next to the input by any of the refused calls.
    let entries: Vec<String> = std::fs::read_dir(&d)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "data" && n != "real.pdf")
        .collect();
    assert!(entries.is_empty(), "refused calls wrote {entries:?}");

    drop(server);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_read_only_tool_returns_its_report() {
    let _g = one_at_a_time();
    let d = scratch("hash");
    let mut server = Server::start(&d.join("data"), None);
    server.initialize();
    let f = d.join("abc.txt");
    std::fs::write(&f, "abc").unwrap();
    let r = server.call("files_hash", json!({ "files": [f] }));
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert!(text_of(&r).contains("ba7816bf8f01cfea41414"), "{r}");
    assert_eq!(r["result"]["structuredContent"]["outputs"], json!([]));
    drop(server);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn community_plugins_are_listed_and_run() {
    let _g = one_at_a_time();
    let module_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/examples-wasm/word-count");
    let status = {
        #[cfg(unix)]
        let mut c = {
            let mut c = Command::new("nice");
            c.args(["-n", "15"]).arg(env!("CARGO"));
            c
        };
        #[cfg(not(unix))]
        let mut c = Command::new(env!("CARGO"));
        c.args([
            "build",
            "--release",
            "--target",
            "wasm32-wasip2",
            "-j3",
            "--quiet",
        ])
        .arg("--manifest-path")
        .arg(module_dir.join("Cargo.toml"))
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .unwrap()
    };
    assert!(
        status.success(),
        "build failed (rustup target add wasm32-wasip2?)"
    );

    let d = scratch("community");
    let mut server = Server::start(&d.join("data"), Some(&module_dir));
    server.initialize();
    let list = server.request("tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    let tool = tools
        .iter()
        .find(|t| t["name"] == "text_word_count")
        .unwrap_or_else(|| panic!("the community module is not listed: {list}"));
    check_schema(tool);
    let f = d.join("poem.txt");
    std::fs::write(&f, "roses are red\nviolets are blue\n").unwrap();
    let r = server.call("text_word_count", json!({ "files": [f] }));
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert!(text_of(&r).contains("2 lines, 6 words"), "{r}");
    drop(server);
    std::fs::remove_dir_all(&d).unwrap();
}
