//! A small MCP server over the stdio transport: one JSON-RPC 2.0 message per line, in and out.
//!
//! It speaks `initialize`, `ping`, `tools/list` and `tools/call`, answers progress for a call
//! when the client sent a progress token, and honours `notifications/cancelled`. What a tool does
//! is up to a [`Backend`], so this file holds the protocol and nothing else. Nothing is ever
//! written to the output except protocol messages: the host process keeps its own logging on
//! stderr.
//!
//! Threads: the calling thread reads input and answers `initialize`, `ping` and `tools/list` at
//! once. Every `tools/call` goes to a small pool of worker threads, so a long job never holds up a
//! ping, a listing or another call. All output goes through one locked writer, so messages never
//! interleave. A tool that panics becomes an error result and the server keeps running.
//!
//! The protocol layer is written by hand on purpose. The official Rust SDK pulls in an async
//! runtime and a schema generator, which the idle-memory and start-time budgets do not allow for
//! a server that has four methods.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use serde_json::{Map, Value, json};

/// Protocol versions this server can speak, newest first. The client's version is used when it is
/// in the list, otherwise the newest. Versions before 2025-06-18 allow JSON-RPC batches, which
/// this server does not read, so they are not offered.
pub const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18"];

/// Tool calls that can run at the same time.
const WORKERS: usize = 4;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// What the server needs from the program around it. Calls run on several threads at once.
pub trait Backend: Sync {
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

/// One accepted `tools/call`, from the moment it is queued until its reply is sent.
struct Slot {
    cancelled: AtomicBool,
    hook: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

impl Slot {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(f) = lock(&self.hook).as_ref() {
            f();
        }
    }
}

/// Handed to a tool while it runs: report progress, learn about a cancel.
pub struct CallContext {
    progress: Option<ProgressFn>,
    slot: Arc<Slot>,
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

    /// Run `f` when the client cancels this call. It runs on another thread, and right away when
    /// the cancel came in before this was called.
    pub fn on_cancel(&self, f: impl Fn() + Send + Sync + 'static) {
        let mut hook = lock(&self.slot.hook);
        *hook = Some(Box::new(f));
        if self.slot.cancelled.load(Ordering::Acquire)
            && let Some(f) = hook.as_ref()
        {
            f();
        }
    }
}

/// A call waiting for a worker.
struct Job {
    id: Value,
    name: String,
    args: Map<String, Value>,
    token: Option<Value>,
    slot: Arc<Slot>,
}

type Writer<W> = Arc<Mutex<W>>;
/// Calls that are queued or running, by request id.
type Pending = Mutex<HashMap<String, Arc<Slot>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Serve until `input` ends. `name` and `version` identify the program to the client.
pub fn serve<R, W, B>(
    input: R,
    output: W,
    backend: &B,
    name: &str,
    version: &str,
) -> std::io::Result<()>
where
    R: BufRead,
    W: Write + Send + 'static,
    B: Backend,
{
    serve_with_workers(input, output, backend, name, version, WORKERS)
}

fn serve_with_workers<R, W, B>(
    input: R,
    output: W,
    backend: &B,
    name: &str,
    version: &str,
    workers: usize,
) -> std::io::Result<()>
where
    R: BufRead,
    W: Write + Send + 'static,
    B: Backend,
{
    let out: Writer<W> = Arc::new(Mutex::new(output));
    let pending: Pending = Mutex::new(HashMap::new());
    let info = json!({ "name": name, "title": "Panoply", "version": version });
    let (tx, rx) = mpsc::channel::<Job>();
    let rx = Mutex::new(rx);
    std::thread::scope(|scope| {
        for _ in 0..workers.max(1) {
            scope.spawn(|| {
                // Hold the lock only while waiting for the next job, never while running it.
                loop {
                    let next = lock(&rx).recv();
                    let Ok(job) = next else { break };
                    run_job(job, backend, &out, &pending);
                }
            });
        }
        // The queue closes when `tx` drops at the end of the read loop, and the workers finish
        // what is queued, then stop.
        read_loop(input, backend, &info, &out, &pending, tx)
    })
}

fn read_loop<R, W, B>(
    mut input: R,
    backend: &B,
    info: &Value,
    out: &Writer<W>,
    pending: &Pending,
    jobs: mpsc::Sender<Job>,
) -> std::io::Result<()>
where
    R: BufRead,
    W: Write + Send + 'static,
    B: Backend,
{
    let mut buf = Vec::new();
    loop {
        buf.clear();
        if input.read_until(b'\n', &mut buf)? == 0 {
            return Ok(());
        }
        let Ok(line) = std::str::from_utf8(&buf) else {
            send(
                out,
                &error_reply(Value::Null, PARSE_ERROR, "The message was not valid UTF-8."),
            )?;
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        let msg = match serde_json::from_str::<Value>(line) {
            Ok(m) => m,
            Err(_) => {
                send(
                    out,
                    &error_reply(Value::Null, PARSE_ERROR, "The message was not valid JSON."),
                )?;
                continue;
            }
        };
        if let Some(reply) = handle(&msg, backend, info, out, pending, &jobs) {
            send(out, &reply)?;
        }
    }
}

fn send<W: Write>(out: &Writer<W>, msg: &Value) -> std::io::Result<()> {
    let mut w = lock(out);
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

/// One incoming message. A request gets a reply (a `tools/call` gets it later, from a worker); a
/// notification, a request with a null id or a stray response gets none.
fn handle<W, B>(
    msg: &Value,
    backend: &B,
    info: &Value,
    out: &Writer<W>,
    pending: &Pending,
    jobs: &mpsc::Sender<Job>,
) -> Option<Value>
where
    W: Write + Send + 'static,
    B: Backend,
{
    let Some(obj) = msg.as_object() else {
        return Some(error_reply(
            Value::Null,
            INVALID_REQUEST,
            "Expected one JSON object per line. Batches are not supported.",
        ));
    };
    let method = obj.get("method").and_then(Value::as_str);
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    // An id of null is not a request id: treat the message as a notification.
    let id = obj.get("id").filter(|id| !id.is_null()).cloned();
    let (Some(method), Some(id)) = (method, id) else {
        if method == Some("notifications/cancelled") {
            cancel(&params, pending);
        }
        return None;
    };
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
        "tools/call" => return queue_call(id, &params, pending, jobs, out),
        _ => error_reply(id, METHOD_NOT_FOUND, &format!("Unknown method: {method}")),
    })
}

/// `notifications/cancelled`: ask the call to stop. It may still be queued, in which case a worker
/// skips it when its turn comes.
fn cancel(params: &Value, pending: &Pending) {
    if let Some(id) = params.get("requestId")
        && let Some(slot) = lock(pending).get(&id.to_string()).cloned()
    {
        slot.cancel();
    }
}

/// Check a `tools/call` and hand it to the workers. Returns a reply only for a malformed call.
fn queue_call<W: Write + Send + 'static>(
    id: Value,
    params: &Value,
    pending: &Pending,
    jobs: &mpsc::Sender<Job>,
    out: &Writer<W>,
) -> Option<Value> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Some(error_reply(
            id,
            INVALID_PARAMS,
            "tools/call needs a tool name.",
        ));
    };
    let args = match params.get("arguments") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => {
            return Some(error_reply(
                id,
                INVALID_PARAMS,
                "arguments must be an object.",
            ));
        }
    };
    let token = params
        .pointer("/_meta/progressToken")
        .filter(|t| t.is_string() || t.is_number())
        .cloned();
    let slot = Arc::new(Slot {
        cancelled: AtomicBool::new(false),
        hook: Mutex::new(None),
    });
    lock(pending).insert(id.to_string(), slot.clone());
    let job = Job {
        id: id.clone(),
        name: name.to_string(),
        args,
        token,
        slot,
    };
    if jobs.send(job).is_err() {
        lock(pending).remove(&id.to_string());
        let _ = send(
            out,
            &error_reply(id, INVALID_REQUEST, "The server is shutting down."),
        );
    }
    None
}

/// Progress notifications, only when the client sent a token, and only when the number goes up
/// (the protocol requires that), in whole percent steps.
fn progress_for<W: Write + Send + 'static>(
    token: Option<Value>,
    out: &Writer<W>,
) -> Option<ProgressFn> {
    let token = token?;
    let out = out.clone();
    let last = Mutex::new(-1.0_f64);
    Some(Arc::new(move |fraction: f32, label: &str| {
        let pct = (f64::from(fraction.clamp(0.0, 1.0)) * 100.0).round();
        let mut last = lock(&last);
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
    }))
}

fn run_job<W, B>(job: Job, backend: &B, out: &Writer<W>, pending: &Pending)
where
    W: Write + Send + 'static,
    B: Backend,
{
    let Job {
        id,
        name,
        args,
        token,
        slot,
    } = job;
    let reply = if slot.cancelled.load(Ordering::Acquire) {
        Ok(ToolReply::error(
            "The call was cancelled before it started.",
        ))
    } else {
        let ctx = CallContext {
            progress: progress_for(token, out),
            slot,
        };
        // A module that panics must not take the server and every other call with it.
        match catch_unwind(AssertUnwindSafe(|| backend.call(&name, &args, &ctx))) {
            Ok(r) => r,
            Err(_) => Ok(ToolReply::error(format!(
                "{name} stopped unexpectedly. Nothing was changed. Try again, and if it keeps \
                 happening, report the file type and the tool name."
            ))),
        }
    };
    lock(pending).remove(&id.to_string());
    let msg = match reply {
        Err(UnknownTool) => error_reply(id, INVALID_PARAMS, &format!("Unknown tool: {name}")),
        Ok(r) => {
            let text = if r.summary.is_empty() {
                "Done.".to_string()
            } else {
                r.summary.clone()
            };
            let mut result = json!({
                "content": [{ "type": "text", "text": text }],
                "isError": r.is_error,
            });
            if !r.is_error {
                result["structuredContent"] = json!({ "outputs": r.outputs, "notes": r.notes });
            }
            result_reply(id, result)
        }
    };
    let _ = send(out, &msg);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    /// A backend with one tool per behaviour the server has to survive.
    #[derive(Default)]
    struct Lab {
        running: AtomicUsize,
        most_at_once: AtomicUsize,
        started: Mutex<Vec<String>>,
    }

    impl Backend for Lab {
        fn tools(&self) -> Vec<Value> {
            vec![json!({ "name": "echo", "inputSchema": { "type": "object" } })]
        }

        fn call(
            &self,
            name: &str,
            args: &Map<String, Value>,
            ctx: &CallContext,
        ) -> Result<ToolReply, UnknownTool> {
            self.started.lock().unwrap().push(name.to_string());
            match name {
                "echo" => {
                    ctx.progress(0.5, "Halfway");
                    ctx.progress(0.5, "Still halfway");
                    ctx.progress(1.0, "Done");
                    Ok(ToolReply {
                        summary: format!("got {}", Value::Object(args.clone())),
                        outputs: vec!["/tmp/a".into()],
                        ..ToolReply::default()
                    })
                }
                "slow" => {
                    let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
                    self.most_at_once.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(300));
                    self.running.fetch_sub(1, Ordering::SeqCst);
                    Ok(ToolReply {
                        summary: "slow done".into(),
                        ..ToolReply::default()
                    })
                }
                // Runs until the client cancels it, as a long job that polls its token does.
                "wait" => {
                    let stop = Arc::new(AtomicBool::new(false));
                    let flag = stop.clone();
                    ctx.on_cancel(move || flag.store(true, Ordering::SeqCst));
                    let t = std::time::Instant::now();
                    while !stop.load(Ordering::SeqCst) && t.elapsed() < Duration::from_secs(10) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Ok(if stop.load(Ordering::SeqCst) {
                        ToolReply::error("stopped at the next check")
                    } else {
                        ToolReply::default()
                    })
                }
                "boom" => panic!("a module bug"),
                _ => Err(UnknownTool),
            }
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
        run_with(&Lab::default(), 4, lines)
    }

    fn run_with(lab: &Lab, workers: usize, lines: &[Value]) -> Vec<Value> {
        let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
        run_bytes(lab, workers, input.into_bytes())
    }

    fn run_bytes(lab: &Lab, workers: usize, input: Vec<u8>) -> Vec<Value> {
        let sink = Sink::default();
        serve_with_workers(
            Cursor::new(input),
            sink.clone(),
            lab,
            "panoply",
            "0.0.0",
            workers,
        )
        .unwrap();
        let bytes = sink.0.lock().unwrap().clone();
        String::from_utf8(bytes)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn call_msg(id: i64, name: &str) -> Value {
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name}})
    }

    fn position_of(out: &[Value], id: i64) -> usize {
        out.iter()
            .position(|m| m["id"] == id)
            .unwrap_or_else(|| panic!("no reply for {id}: {out:?}"))
    }

    #[test]
    fn initialize_echoes_a_known_version_and_falls_back_to_the_newest() {
        let out = run(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}),
            json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}),
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
        serve(
            Cursor::new(input),
            sink.clone(),
            &Lab::default(),
            "panoply",
            "0",
        )
        .unwrap();
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

    #[test]
    fn older_versions_with_batches_are_not_offered() {
        let out = run(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}),
        ]);
        assert_eq!(out[0]["result"]["protocolVersion"], SUPPORTED_VERSIONS[0]);
        assert!(!SUPPORTED_VERSIONS.contains(&"2025-03-26"));
    }

    #[test]
    fn a_ping_and_a_listing_are_answered_while_a_job_runs() {
        let out = run(&[
            call_msg(1, "slow"),
            json!({"jsonrpc":"2.0","id":2,"method":"ping"}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}),
        ]);
        assert!(position_of(&out, 2) < position_of(&out, 1), "{out:?}");
        assert!(position_of(&out, 3) < position_of(&out, 1), "{out:?}");
    }

    #[test]
    fn calls_run_in_parallel() {
        let lab = Lab::default();
        let out = run_with(
            &lab,
            4,
            &[
                call_msg(1, "slow"),
                call_msg(2, "slow"),
                call_msg(3, "slow"),
            ],
        );
        assert_eq!(out.len(), 3);
        assert_eq!(lab.most_at_once.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn a_cancel_for_a_queued_call_skips_it() {
        let lab = Lab::default();
        // One worker: call 2 is still queued behind call 1 when its cancel arrives.
        let out = run_with(
            &lab,
            1,
            &[
                call_msg(1, "slow"),
                call_msg(2, "echo"),
                json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2}}),
            ],
        );
        let skipped = &out[position_of(&out, 2)];
        assert_eq!(skipped["result"]["isError"], true, "{skipped}");
        assert!(
            skipped["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("cancelled")
        );
        assert_eq!(
            *lab.started.lock().unwrap(),
            ["slow"],
            "call 2 must never start"
        );
        assert_eq!(out[position_of(&out, 1)]["result"]["isError"], false);
    }

    #[test]
    fn a_cancel_reaches_the_running_call() {
        let t = std::time::Instant::now();
        let out = run(&[
            call_msg(7, "wait"),
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":7}}),
        ]);
        let reply = &out[position_of(&out, 7)];
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "the call was not stopped"
        );
    }

    #[test]
    fn a_cancel_for_an_unknown_or_finished_call_does_nothing() {
        let out = run(&[
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":99}}),
            json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
        ]);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_panicking_tool_becomes_an_error_and_the_server_keeps_going() {
        let out = run(&[
            call_msg(1, "boom"),
            call_msg(2, "echo"),
            json!({"jsonrpc":"2.0","id":3,"method":"ping"}),
        ]);
        let boom = &out[position_of(&out, 1)];
        assert_eq!(boom["result"]["isError"], true, "{boom}");
        assert!(
            boom["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("stopped unexpectedly")
        );
        assert_eq!(out[position_of(&out, 2)]["result"]["isError"], false);
        assert!(out.iter().any(|m| m["id"] == 3));
    }

    #[test]
    fn invalid_utf8_gets_a_parse_error_and_the_server_keeps_going() {
        let mut input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n".to_vec();
        input.extend_from_slice(b"\xff\xfe not utf8\n");
        input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n");
        let out = run_bytes(&Lab::default(), 4, input);
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(out.iter().any(|m| m["error"]["code"] == PARSE_ERROR));
        assert!(out.iter().any(|m| m["id"] == 2 && m["result"].is_object()));
    }

    #[test]
    fn a_request_with_a_null_id_is_a_notification() {
        let out = run(&[
            json!({"jsonrpc":"2.0","id":null,"method":"ping"}),
            json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], 1);
    }
}
