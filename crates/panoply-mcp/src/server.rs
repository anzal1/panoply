//! A small MCP server over the stdio transport: one JSON-RPC 2.0 message per line, in and out.
//!
//! It speaks `initialize`, `ping`, `tools/list` and `tools/call`, answers progress for a call
//! when the client sent a progress token, and honours `notifications/cancelled` for the call that
//! is running. What a tool does is up to a [`Backend`], so this file holds the protocol and
//! nothing else. Nothing is ever written to the output except protocol messages: the host
//! process keeps its own logging on stderr.
//!
//! The protocol layer is written by hand on purpose. The official Rust SDK pulls in an async
//! runtime and a schema generator, which the idle-memory and start-time budgets do not allow for
//! a server that has four methods.

use std::io::{BufRead, Write};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};

/// Protocol versions this server can speak, newest first. The client's version is used when it is
/// in the list, otherwise the newest.
pub const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// What the server needs from the program around it.
pub trait Backend {
    /// The tool definitions to list.
    fn tools(&self) -> Vec<Value>;

    /// Run one tool. `Err` is for a name that is not a tool; a tool that ran and failed is an
    /// `Ok` reply with `is_error` set, so the model can read what went wrong.
    fn call(
        &self,
        name: &str,
        arguments: &Map<String, Value>,
        ctx: &CallContext,
    ) -> Result<ToolReply, UnknownTool>;
}

/// The tool name given to `tools/call` is not one of the listed tools.
#[derive(Debug)]
pub struct UnknownTool;

/// The result of one tool call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolReply {
    /// True when the tool failed. The text then says what happened and what to do next.
    pub is_error: bool,
    /// A short summary for the model to read.
    pub summary: String,
    /// Absolute paths of the files written.
    pub outputs: Vec<String>,
    /// Report lines from tools that only read.
    pub notes: Vec<String>,
}

impl ToolReply {
    /// A failed call with a plain message.
    pub fn error(message: impl Into<String>) -> Self {
        ToolReply {
            is_error: true,
            summary: message.into(),
            ..Self::default()
        }
    }
}

type ProgressFn = Arc<dyn Fn(f32, &str) + Send + Sync>;
type CancelHook = Arc<Mutex<Option<Box<dyn Fn() + Send + Sync>>>>;

/// Handed to a tool while it runs: report progress, learn about a cancel.
pub struct CallContext {
    progress: Option<ProgressFn>,
    cancel: CancelHook,
}

impl CallContext {
    /// Say how far along the call is (0.0 to 1.0). Does nothing when the client did not ask for
    /// progress.
    pub fn progress(&self, fraction: f32, label: &str) {
        if let Some(p) = &self.progress {
            p(fraction, label);
        }
    }

    /// The same as [`CallContext::progress`], as a value that can move into a job's event sink.
    pub fn progress_sink(&self) -> impl Fn(f32, &str) + Send + Sync + Clone + 'static {
        let p = self.progress.clone();
        move |fraction, label| {
            if let Some(p) = &p {
                p(fraction, label);
            }
        }
    }

    /// Run `f` if the client cancels this call. It runs on another thread.
    pub fn on_cancel(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.cancel.lock().unwrap_or_else(|p| p.into_inner()) = Some(Box::new(f));
    }
}

type InFlight = Arc<Mutex<Option<(Value, CancelHook)>>>;

/// Serve until `input` ends. `name` and `version` identify the program to the client.
pub fn serve<R, W, B>(
    input: R,
    output: W,
    backend: &B,
    name: &str,
    version: &str,
) -> std::io::Result<()>
where
    R: BufRead + Send + 'static,
    W: Write + Send + 'static,
    B: Backend,
{
    let out = Arc::new(Mutex::new(output));
    let in_flight: InFlight = Arc::new(Mutex::new(None));
    let (tx, rx) = mpsc::channel::<Value>();

    // A reader thread, so a cancel can arrive while a tool is running on this one.
    let reader_out = out.clone();
    let reader_flight = in_flight.clone();
    let reader = std::thread::spawn(move || {
        for line in input.lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(msg) => {
                    if msg.get("method").and_then(Value::as_str) == Some("notifications/cancelled")
                    {
                        cancel_in_flight(&reader_flight, &msg);
                    } else if tx.send(msg).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = send(
                        &reader_out,
                        &error_reply(Value::Null, PARSE_ERROR, "The message was not valid JSON."),
                    );
                }
            }
        }
    });

    let info = json!({ "name": name, "title": "Panoply", "version": version });
    while let Ok(msg) = rx.recv() {
        if let Some(reply) = handle(&msg, backend, &info, &out, &in_flight) {
            send(&out, &reply)?;
        }
    }
    let _ = reader.join();
    Ok(())
}

fn cancel_in_flight(flight: &InFlight, msg: &Value) {
    let wanted = msg.pointer("/params/requestId");
    let guard = flight.lock().unwrap_or_else(|p| p.into_inner());
    if let (Some((id, hook)), Some(wanted)) = (guard.as_ref(), wanted)
        && id == wanted
        && let Some(f) = hook.lock().unwrap_or_else(|p| p.into_inner()).as_ref()
    {
        f();
    }
}

fn send<W: Write>(out: &Arc<Mutex<W>>, msg: &Value) -> std::io::Result<()> {
    let mut w = out.lock().unwrap_or_else(|p| p.into_inner());
    let mut line = serde_json::to_vec(msg).map_err(std::io::Error::other)?;
    line.push(b'\n');
    w.write_all(&line)?;
    w.flush()
}

fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn result_reply(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// One incoming message. A request gets a reply; a notification or a stray response gets none.
fn handle<W, B>(
    msg: &Value,
    backend: &B,
    info: &Value,
    out: &Arc<Mutex<W>>,
    in_flight: &InFlight,
) -> Option<Value>
where
    W: Write + Send + 'static,
    B: Backend,
{
    let Some(obj) = msg.as_object() else {
        return Some(error_reply(
            Value::Null,
            INVALID_REQUEST,
            "Expected one JSON object per line.",
        ));
    };
    let method = obj.get("method").and_then(Value::as_str);
    let id = obj.get("id").cloned();
    let (Some(method), Some(id)) = (method, id) else {
        return None;
    };
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = asked
                .filter(|v| SUPPORTED_VERSIONS.contains(v))
                .unwrap_or(SUPPORTED_VERSIONS[0]);
            result_reply(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": info,
                    "instructions": "Panoply runs on this computer and works on local files. \
                        Give files as absolute paths. Results are new files; originals are \
                        never changed or overwritten.",
                }),
            )
        }
        "ping" => result_reply(id, json!({})),
        "tools/list" => result_reply(id, json!({ "tools": backend.tools() })),
        "tools/call" => call(id, &params, backend, out, in_flight),
        _ => error_reply(id, METHOD_NOT_FOUND, &format!("Unknown method: {method}")),
    })
}

fn call<W, B>(
    id: Value,
    params: &Value,
    backend: &B,
    out: &Arc<Mutex<W>>,
    in_flight: &InFlight,
) -> Value
where
    W: Write + Send + 'static,
    B: Backend,
{
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return error_reply(id, INVALID_PARAMS, "tools/call needs a tool name.");
    };
    let args = match params.get("arguments") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => {
            return error_reply(id, INVALID_PARAMS, "arguments must be an object.");
        }
    };

    // Progress notifications, only when the client sent a token, and only when the number goes
    // up (the protocol requires that), in whole percent steps.
    let progress = params
        .pointer("/_meta/progressToken")
        .filter(|t| t.is_string() || t.is_number())
        .cloned()
        .map(|token| {
            let out = out.clone();
            let last = Mutex::new(-1.0_f64);
            Arc::new(move |fraction: f32, label: &str| {
                let pct = (f64::from(fraction.clamp(0.0, 1.0)) * 100.0).round();
                let mut last = last.lock().unwrap_or_else(|p| p.into_inner());
                if pct <= *last {
                    return;
                }
                *last = pct;
                let _ = send(
                    &out,
                    &json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/progress",
                        "params": {
                            "progressToken": token,
                            "progress": pct,
                            "total": 100,
                            "message": label,
                        },
                    }),
                );
            }) as ProgressFn
        });

    let hook = Arc::new(Mutex::new(None));
    *in_flight.lock().unwrap_or_else(|p| p.into_inner()) = Some((id.clone(), hook.clone()));
    let ctx = CallContext {
        progress,
        cancel: hook,
    };
    let reply = backend.call(name, &args, &ctx);
    *in_flight.lock().unwrap_or_else(|p| p.into_inner()) = None;

    match reply {
        Err(UnknownTool) => error_reply(id, INVALID_PARAMS, &format!("Unknown tool: {name}")),
        Ok(r) => {
            let mut text = r.summary.clone();
            if text.is_empty() {
                text = "Done.".into();
            }
            let mut result = json!({
                "content": [{ "type": "text", "text": text }],
                "isError": r.is_error,
            });
            if !r.is_error {
                result["structuredContent"] = json!({ "outputs": r.outputs, "notes": r.notes });
            }
            result_reply(id, result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct Echo;

    impl Backend for Echo {
        fn tools(&self) -> Vec<Value> {
            vec![json!({ "name": "echo", "inputSchema": { "type": "object" } })]
        }

        fn call(
            &self,
            name: &str,
            args: &Map<String, Value>,
            ctx: &CallContext,
        ) -> Result<ToolReply, UnknownTool> {
            if name != "echo" {
                return Err(UnknownTool);
            }
            ctx.progress(0.5, "Halfway");
            ctx.progress(0.5, "Still halfway");
            ctx.progress(1.0, "Done");
            Ok(ToolReply {
                summary: format!("got {}", Value::Object(args.clone())),
                outputs: vec!["/tmp/a".into()],
                ..ToolReply::default()
            })
        }
    }

    /// A writer the test can read after `serve` has returned.
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn run(lines: &[Value]) -> Vec<Value> {
        let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
        let sink = Sink::default();
        serve(Cursor::new(input), sink.clone(), &Echo, "panoply", "0.0.0").unwrap();
        let bytes = sink.0.lock().unwrap().clone();
        String::from_utf8(bytes)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn initialize_echoes_a_known_version_and_falls_back_to_the_newest() {
        let out = run(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}),
            json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        ]);
        assert_eq!(out.len(), 2, "a notification gets no reply: {out:?}");
        assert_eq!(out[0]["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(out[1]["result"]["protocolVersion"], SUPPORTED_VERSIONS[0]);
        assert_eq!(out[0]["result"]["serverInfo"]["name"], "panoply");
        assert!(out[0]["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn lists_tools_and_answers_ping() {
        let out = run(&[
            json!({"jsonrpc":"2.0","id":"a","method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":"b","method":"ping"}),
        ]);
        assert_eq!(out[0]["id"], "a");
        assert_eq!(out[0]["result"]["tools"][0]["name"], "echo");
        assert_eq!(out[1]["result"], json!({}));
    }

    #[test]
    fn a_call_returns_text_and_structured_content_and_progress_goes_up() {
        let out = run(&[json!({
            "jsonrpc":"2.0","id":7,"method":"tools/call",
            "params":{"name":"echo","arguments":{"x":1},"_meta":{"progressToken":"t1"}}
        })]);
        let progress: Vec<&Value> = out
            .iter()
            .filter(|m| m["method"] == "notifications/progress")
            .collect();
        // 0.5 twice is one notification, then 1.0.
        assert_eq!(progress.len(), 2, "{out:?}");
        assert_eq!(progress[0]["params"]["progressToken"], "t1");
        assert_eq!(progress[0]["params"]["progress"], 50.0);
        assert_eq!(progress[1]["params"]["progress"], 100.0);
        let reply = out.last().unwrap();
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["result"]["isError"], false);
        assert!(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("\"x\":1")
        );
        assert_eq!(
            reply["result"]["structuredContent"]["outputs"],
            json!(["/tmp/a"])
        );
    }

    #[test]
    fn no_progress_without_a_token() {
        let out = run(&[json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"echo"}
        })]);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn unknown_tool_unknown_method_and_bad_input_are_protocol_errors() {
        let sink = Sink::default();
        let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"nope\"}}\n\
                     {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"resources/list\"}\n\
                     not json\n\
                     [1,2]\n\
                     {\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"echo\",\"arguments\":5}}\n";
        serve(Cursor::new(input), sink.clone(), &Echo, "panoply", "0").unwrap();
        let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        let mut codes: Vec<i64> = text
            .lines()
            .map(|l| {
                serde_json::from_str::<Value>(l).unwrap()["error"]["code"]
                    .as_i64()
                    .unwrap()
            })
            .collect();
        codes.sort_unstable();
        assert_eq!(
            codes,
            [
                PARSE_ERROR,
                INVALID_PARAMS,
                INVALID_PARAMS,
                METHOD_NOT_FOUND,
                INVALID_REQUEST
            ]
        );
    }
}
