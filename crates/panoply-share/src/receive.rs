//! The receiving side: the LocalSend v2 endpoints a Panoply device serves.
//!
//! `GET /info`, `POST /register`, `POST /prepare-upload`, `POST /upload`, `POST /cancel`.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::{Method, Request, Response};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncWriteExt, BufWriter};

use crate::error::Error;
use crate::fsx;
use crate::http::{self, Body, query_get, query_pairs};
use crate::node::{Canceller, Direction, Shared, TransferKind};
use crate::proto::{
    API_PREFIX, DeviceInfo, FileMeta, PrepareUploadRequest, PrepareUploadResponse, Scheme, hex,
    is_fingerprint, parse_iso8601, random_hex,
};
use crate::trust::check_code;

/// A boxed future, the return type of the prompt callback.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// Largest `prepare-upload` body accepted (a very large folder tree).
const MAX_PREPARE_BODY: usize = 32 * 1024 * 1024;
/// A text message larger than this is treated as a file.
const MAX_TEXT_BYTES: usize = 1024 * 1024;

/// One file in an incoming request, as shown in a prompt.
#[derive(Debug, Clone)]
pub struct IncomingFile {
    /// Id used in the accept list.
    pub id: String,
    /// Name as the sender gave it (may include folders).
    pub name: String,
    /// Size in bytes.
    pub size: u64,
    /// MIME type the sender declared.
    pub mime: String,
    /// True for a file type that runs code when opened.
    pub risky: bool,
}

/// What the receiver shows when someone wants to send files.
#[derive(Debug, Clone)]
pub struct IncomingRequest {
    /// The sender as it describes itself.
    pub sender: DeviceInfo,
    /// Where the request came from.
    pub remote: IpAddr,
    /// True when the sender's server presented the certificate it claims. Unverified senders
    /// are never auto-accepted and must not be trusted from the prompt.
    pub verified: bool,
    /// True when the sender's fingerprint is in the trust list.
    pub trusted: bool,
    /// True when a trusted device with this alias exists under another fingerprint.
    pub alias_conflict: bool,
    /// Four words both screens show for visual comparison.
    pub check_code: String,
    /// The files offered.
    pub files: Vec<IncomingFile>,
    /// Total size of the files.
    pub total_size: u64,
    /// The text, when this is a message.
    pub text: Option<String>,
}

/// The answer to an [`IncomingRequest`].
#[derive(Debug, Clone)]
pub enum Decision {
    /// Refuse (the sender sees HTTP 403).
    Decline,
    /// Accept. `files` limits which file ids are taken (`None` takes all); `trust` remembers
    /// the sender so it can be accepted without a prompt next time.
    Accept {
        /// File ids to take, or all.
        files: Option<Vec<String>>,
        /// Add the sender to the trust list (ignored for unverified senders).
        trust: bool,
    },
}

impl Decision {
    /// Accept everything, without trusting the sender.
    pub fn accept_all() -> Self {
        Decision::Accept {
            files: None,
            trust: false,
        }
    }
}

/// Called for each request that is not auto-accepted. Implement it for a UI, or pass a
/// closure through [`handler_fn`].
pub trait ReceiveHandler: Send + Sync + 'static {
    /// Decide on an incoming request. Not answering within the config's `request_timeout`
    /// counts as a decline.
    fn decide(&self, request: IncomingRequest) -> BoxFuture<Decision>;
}

struct FnHandler<F>(F);

impl<F> ReceiveHandler for FnHandler<F>
where
    F: Fn(IncomingRequest) -> BoxFuture<Decision> + Send + Sync + 'static,
{
    fn decide(&self, request: IncomingRequest) -> BoxFuture<Decision> {
        (self.0)(request)
    }
}

/// Wrap an async closure as a handler.
pub fn handler_fn<F>(f: F) -> Arc<dyn ReceiveHandler>
where
    F: Fn(IncomingRequest) -> BoxFuture<Decision> + Send + Sync + 'static,
{
    Arc::new(FnHandler(f))
}

/// Accepts every request. For tests and for "quick save" setups.
pub fn accept_all_handler() -> Arc<dyn ReceiveHandler> {
    handler_fn(|_| Box::pin(async { Decision::accept_all() }))
}

/// Declines every request.
pub fn decline_all_handler() -> Arc<dyn ReceiveHandler> {
    handler_fn(|_| Box::pin(async { Decision::Decline }))
}

/// When the prompt is skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReceiveMode {
    /// Always call the handler.
    #[default]
    AskEveryTime,
    /// Accept verified, trusted devices without calling the handler.
    AutoAcceptTrusted,
}

/// Receiving settings.
#[derive(Clone)]
pub struct ReceiveConfig {
    /// Folder files are saved into.
    pub dir: PathBuf,
    /// Require this PIN (`?pin=` on `prepare-upload`).
    pub pin: Option<String>,
    /// When to skip the prompt.
    pub mode: ReceiveMode,
    /// Put each sender's files in a folder named after its alias.
    pub subfolder_per_sender: bool,
    /// Connect back to the sender to check its certificate against the claimed fingerprint.
    pub verify_sender: bool,
    /// How long a request may wait for the handler (default 60 s).
    pub request_timeout: Duration,
    /// A session with no upload for this long is dropped (default 2 minutes).
    pub idle_timeout: Duration,
    /// Largest number of files in one request.
    pub max_files: usize,
    /// Requests one address may make per minute before it gets HTTP 429.
    pub max_requests_per_minute: usize,
    /// The prompt.
    pub handler: Arc<dyn ReceiveHandler>,
}

impl ReceiveConfig {
    /// Defaults: ask every time, verify the sender, 60 s to answer.
    pub fn new(dir: impl Into<PathBuf>, handler: Arc<dyn ReceiveHandler>) -> Self {
        ReceiveConfig {
            dir: dir.into(),
            pin: None,
            mode: ReceiveMode::AskEveryTime,
            subfolder_per_sender: false,
            verify_sender: true,
            request_timeout: Duration::from_secs(60),
            idle_timeout: Duration::from_secs(120),
            max_files: 100_000,
            max_requests_per_minute: 30,
            handler,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotState {
    Pending,
    Active,
    Done,
}

struct Slot {
    meta: FileMeta,
    token: String,
    parts: Vec<String>,
    name: String,
    state: SlotState,
}

struct Session {
    id: String,
    peer: DeviceInfo,
    remote: IpAddr,
    dir: PathBuf,
    slots: HashMap<String, Slot>,
    cancel: Canceller,
    last_active: Instant,
    started: Instant,
    total: u64,
    files_total: usize,
    done_files: usize,
    bytes: Arc<AtomicU64>,
}

#[derive(Default)]
struct Inner {
    session: Option<Session>,
    pending: bool,
    hits: HashMap<IpAddr, VecDeque<Instant>>,
    pin_fails: HashMap<IpAddr, (u32, Instant)>,
    ignored: HashMap<String, (u32, Instant)>,
    quick_until: Option<Instant>,
}

pub(crate) struct ReceiveState {
    pub config: ReceiveConfig,
    inner: Mutex<Inner>,
}

const PIN_ATTEMPTS: u32 = 3;
const PIN_LOCKOUT: Duration = Duration::from_secs(60);
const IGNORED_LIMIT: u32 = 3;
const IGNORED_LOCKOUT: Duration = Duration::from_secs(300);

impl ReceiveState {
    pub fn new(config: ReceiveConfig) -> Self {
        ReceiveState {
            config,
            inner: Mutex::default(),
        }
    }

    pub fn set_quick_save(&self, d: Duration) {
        self.inner.lock().unwrap().quick_until = Some(Instant::now() + d);
    }

    pub fn cancel_all(&self) {
        if let Some(s) = &self.inner.lock().unwrap().session {
            s.cancel.cancel();
        }
    }
}

/// Clears the "a prompt is open" flag unless the request became a session.
struct PendingGuard<'a>(&'a ReceiveState);

impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        self.0.inner.lock().unwrap().pending = false;
    }
}

fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= (*a.get(i).unwrap_or(&0) ^ *b.get(i).unwrap_or(&0)) as usize;
    }
    diff == 0
}

/// Route one request.
pub(crate) async fn handle(
    shared: Arc<Shared>,
    req: Request<Incoming>,
    peer: SocketAddr,
) -> Response<Body> {
    let path = req.uri().path().to_string();
    let Some(route) = path.strip_prefix(API_PREFIX) else {
        return http::error(404, "not found");
    };
    match (req.method(), route) {
        (&Method::GET, "/info") => http::json(200, &shared.info(None)),
        (&Method::POST, "/register") => register(&shared, req, peer).await,
        (&Method::POST, "/prepare-upload") => prepare_upload(&shared, req, peer).await,
        (&Method::POST, "/upload") => upload(&shared, req, peer).await,
        (&Method::POST, "/cancel") => cancel(&shared, req, peer),
        _ => http::error(404, "not found"),
    }
}

async fn register(
    shared: &Arc<Shared>,
    req: Request<Incoming>,
    peer: SocketAddr,
) -> Response<Body> {
    let Ok(body) = http::read_limited(req.into_body(), 64 * 1024).await else {
        return http::error(400, "invalid body");
    };
    let Ok(info) = serde_json::from_slice::<DeviceInfo>(&body) else {
        return http::error(400, "invalid body");
    };
    shared.add_device(info, peer.ip());
    http::json(200, &shared.info(None))
}

fn cancel(shared: &Arc<Shared>, req: Request<Incoming>, peer: SocketAddr) -> Response<Body> {
    let q = query_pairs(req.uri().query());
    let Some(id) = query_get(&q, "sessionId") else {
        return http::error(400, "missing sessionId");
    };
    let Some(rs) = &shared.receive else {
        return http::status(200);
    };
    let from_session_peer = rs
        .inner
        .lock()
        .unwrap()
        .session
        .as_ref()
        .is_some_and(|s| s.id == id && s.remote == peer.ip());
    if from_session_peer {
        end_session(shared, id);
    }
    http::status(200)
}

/// End a receiving session: stop its uploads, forget it so the next request is not blocked,
/// and tell subscribers. Returns false when there is no such session.
pub(crate) fn end_session(shared: &Arc<Shared>, session_id: &str) -> bool {
    let Some(rs) = &shared.receive else {
        return false;
    };
    let session = {
        let mut inner = rs.inner.lock().unwrap();
        match &inner.session {
            Some(s) if s.id == session_id => inner.session.take(),
            _ => None,
        }
    };
    let Some(s) = session else { return false };
    s.cancel.cancel();
    shared.active.lock().unwrap().remove(&s.id);
    shared.emit_transfer(
        &s.id,
        Direction::Receive,
        &s.peer.alias,
        TransferKind::Cancelled,
    );
    true
}

fn rate_limited(inner: &mut Inner, ip: IpAddr, max: usize) -> bool {
    let now = Instant::now();
    let q = inner.hits.entry(ip).or_default();
    while q
        .front()
        .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60))
    {
        q.pop_front();
    }
    q.push_back(now);
    let limited = q.len() > max;
    if inner.hits.len() > 4096 {
        inner.hits.retain(|_, q| {
            q.back()
                .is_some_and(|t| now.duration_since(*t) < Duration::from_secs(60))
        });
    }
    limited
}

fn expire_session(inner: &mut Inner, idle: Duration) -> Option<(String, String)> {
    let stale = inner.session.as_ref().is_some_and(|s| {
        s.last_active.elapsed() > idle && !s.slots.values().any(|f| f.state == SlotState::Active)
    });
    if stale {
        let s = inner.session.take().unwrap();
        s.cancel.cancel();
        return Some((s.id, s.peer.alias));
    }
    None
}

async fn prepare_upload(
    shared: &Arc<Shared>,
    req: Request<Incoming>,
    peer: SocketAddr,
) -> Response<Body> {
    let Some(rs) = &shared.receive else {
        return http::error(403, "this device is not receiving");
    };
    let cfg = &rs.config;
    let q = query_pairs(req.uri().query());
    let pin = query_get(&q, "pin").map(str::to_string);
    let ip = peer.ip();

    // Rate limit and PIN, before reading anything large.
    {
        let mut inner = rs.inner.lock().unwrap();
        if let Some((id, name)) = expire_session(&mut inner, cfg.idle_timeout) {
            drop(inner);
            shared.active.lock().unwrap().remove(&id);
            shared.emit_transfer(&id, Direction::Receive, &name, TransferKind::Cancelled);
            inner = rs.inner.lock().unwrap();
        }
        if rate_limited(&mut inner, ip, cfg.max_requests_per_minute) {
            return http::error(429, "too many requests");
        }
        if let Some((n, since)) = inner.pin_fails.get(&ip)
            && *n >= PIN_ATTEMPTS
            && since.elapsed() < PIN_LOCKOUT
        {
            return http::error(429, "too many wrong PINs");
        }
    }
    if let Some(want) = &cfg.pin {
        let ok = pin.as_deref().is_some_and(|p| ct_eq(p, want));
        let mut inner = rs.inner.lock().unwrap();
        if !ok {
            let e = inner.pin_fails.entry(ip).or_insert((0, Instant::now()));
            if e.1.elapsed() >= PIN_LOCKOUT {
                *e = (0, Instant::now());
            }
            if pin.is_some() {
                e.0 += 1;
                e.1 = Instant::now();
            }
            return http::error(401, "a PIN is required");
        }
        inner.pin_fails.remove(&ip);
    }

    let Ok(body) = http::read_limited(req.into_body(), MAX_PREPARE_BODY).await else {
        return http::error(400, "invalid body");
    };
    let Ok(parsed) = serde_json::from_slice::<PrepareUploadRequest>(&body) else {
        return http::error(400, "invalid body");
    };
    let sender = parsed.info.sanitized();
    if !sender.version_compatible() {
        return http::error(400, "incompatible protocol version");
    }
    if parsed.files.is_empty() || parsed.files.len() > cfg.max_files {
        return http::error(400, "invalid file list");
    }

    // Validate every file before anything is asked or created.
    let mut total: u64 = 0;
    let mut files: Vec<(FileMeta, Vec<String>, String)> = Vec::with_capacity(parsed.files.len());
    for (key, meta) in parsed.files {
        if meta.id != key {
            return http::error(400, "file id does not match its key");
        }
        let Ok(mut parts) = fsx::split_relative(&meta.file_name) else {
            return http::error(400, "invalid file name");
        };
        let name = parts
            .pop()
            .expect("split_relative returns at least one part");
        let Some(t) = total.checked_add(meta.size) else {
            return http::error(400, "invalid size");
        };
        total = t;
        files.push((meta, parts, name));
    }
    files.sort_by(|a, b| a.0.id.cmp(&b.0.id));

    // Prove the sender holds the certificate it claims by connecting back to it.
    let verified = cfg.verify_sender
        && sender.scheme_or_default() == Scheme::Https
        && is_fingerprint(&sender.fingerprint)
        && http::connect(
            SocketAddr::new(ip, sender.port_or_default()),
            Scheme::Https,
            Some(&sender.fingerprint),
            Duration::from_millis(1500),
        )
        .await
        .is_ok();

    let trusted = shared.trust.is_trusted(&sender.fingerprint);
    let alias_conflict = shared
        .trust
        .alias_conflict(&sender.alias, &sender.fingerprint);

    // Reserve the single session slot while the user decides.
    {
        let mut inner = rs.inner.lock().unwrap();
        if inner.session.is_some() || inner.pending {
            return http::error(409, "blocked by another session");
        }
        if let Some((n, since)) = inner.ignored.get(&sender.fingerprint)
            && *n >= IGNORED_LIMIT
            && since.elapsed() < IGNORED_LOCKOUT
        {
            return http::error(429, "too many unanswered requests");
        }
        inner.pending = true;
    }
    let _guard = PendingGuard(rs);

    let text = (files.len() == 1
        && files[0].0.file_type.starts_with("text/plain")
        && files[0]
            .0
            .preview
            .as_ref()
            .is_some_and(|p| p.len() <= MAX_TEXT_BYTES)
        && files[0].0.preview.as_ref().map(|p| p.len() as u64) == Some(files[0].0.size))
    .then(|| files[0].0.preview.clone().unwrap_or_default());

    let request = IncomingRequest {
        sender: sender.clone(),
        remote: ip,
        verified,
        trusted,
        alias_conflict,
        check_code: check_code(&shared.fingerprint, &sender.fingerprint),
        files: files
            .iter()
            .map(|(m, parts, name)| IncomingFile {
                id: m.id.clone(),
                name: if parts.is_empty() {
                    name.clone()
                } else {
                    format!("{}/{}", parts.join("/"), name)
                },
                size: m.size,
                mime: m.file_type.clone(),
                risky: fsx::looks_executable(name),
            })
            .collect(),
        total_size: total,
        text: text.clone(),
    };

    let quick = rs
        .inner
        .lock()
        .unwrap()
        .quick_until
        .is_some_and(|t| Instant::now() < t);
    let auto = quick
        || (cfg.mode == ReceiveMode::AutoAcceptTrusted && verified && trusted && !alias_conflict);
    let decision = if auto {
        Decision::accept_all()
    } else {
        match tokio::time::timeout(cfg.request_timeout, cfg.handler.decide(request)).await {
            Ok(d) => {
                rs.inner.lock().unwrap().ignored.remove(&sender.fingerprint);
                d
            }
            Err(_) => {
                let mut inner = rs.inner.lock().unwrap();
                let e = inner
                    .ignored
                    .entry(sender.fingerprint.clone())
                    .or_insert((0, Instant::now()));
                e.0 += 1;
                e.1 = Instant::now();
                return http::error(403, "the request was not answered in time");
            }
        }
    };
    let Decision::Accept { files: only, trust } = decision else {
        return http::error(403, "declined");
    };
    if trust && verified {
        let _ = shared.trust.trust(&sender.fingerprint, &sender.alias);
    }
    if let Some(only) = &only {
        files.retain(|(m, _, _)| only.contains(&m.id));
    }
    if files.is_empty() {
        return http::error(403, "declined");
    }

    // A text message needs no transfer: the text came in the preview.
    if let Some(text) = text {
        shared.emit_transfer(
            &random_hex(8),
            Direction::Receive,
            &sender.alias,
            TransferKind::Text { text },
        );
        return http::status(204);
    }

    let accepted_total: u64 = files.iter().map(|(m, _, _)| m.size).sum();
    let mut dir = cfg.dir.clone();
    if cfg.subfolder_per_sender {
        dir.push(fsx::sanitize_component(&sender.alias));
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return http::error(500, &format!("cannot create the save folder: {e}"));
    }
    if let Some(free) = fsx::free_space(&dir)
        && free < accepted_total
    {
        return http::error(500, "not enough disk space");
    }

    let id = random_hex(16);
    let mut tokens = HashMap::new();
    let mut slots = HashMap::new();
    for (meta, parts, name) in files {
        let token = random_hex(16);
        tokens.insert(meta.id.clone(), token.clone());
        slots.insert(
            meta.id.clone(),
            Slot {
                meta,
                token,
                parts,
                name,
                state: SlotState::Pending,
            },
        );
    }
    let files_total = slots.len();
    let cancel = Canceller::new();
    let session = Session {
        id: id.clone(),
        peer: sender.clone(),
        remote: ip,
        dir,
        slots,
        cancel: cancel.clone(),
        last_active: Instant::now(),
        started: Instant::now(),
        total: accepted_total,
        files_total,
        done_files: 0,
        bytes: Arc::new(AtomicU64::new(0)),
    };
    {
        let mut inner = rs.inner.lock().unwrap();
        inner.session = Some(session);
    }
    shared.active.lock().unwrap().insert(id.clone(), cancel);
    shared.emit_transfer(
        &id,
        Direction::Receive,
        &sender.alias,
        TransferKind::Started {
            files: files_total,
            bytes: accepted_total,
        },
    );
    http::json(
        200,
        &PrepareUploadResponse {
            session_id: id,
            files: tokens,
        },
    )
}

struct UploadPlan {
    session_id: String,
    peer: String,
    dir: PathBuf,
    parts: Vec<String>,
    name: String,
    meta: FileMeta,
    cancel: Canceller,
    bytes: Arc<AtomicU64>,
    total: u64,
}

fn release_slot(rs: &ReceiveState, session_id: &str, file_id: &str, state: SlotState) {
    let mut inner = rs.inner.lock().unwrap();
    if let Some(s) = &mut inner.session
        && s.id == session_id
        && let Some(slot) = s.slots.get_mut(file_id)
    {
        slot.state = state;
        s.last_active = Instant::now();
    }
}

async fn upload(shared: &Arc<Shared>, req: Request<Incoming>, peer: SocketAddr) -> Response<Body> {
    let Some(rs) = &shared.receive else {
        return http::error(403, "this device is not receiving");
    };
    let q = query_pairs(req.uri().query());
    let (Some(session_id), Some(file_id), Some(token)) = (
        query_get(&q, "sessionId"),
        query_get(&q, "fileId"),
        query_get(&q, "token"),
    ) else {
        return http::error(400, "missing parameters");
    };

    let plan = {
        let mut inner = rs.inner.lock().unwrap();
        let Some(s) = &mut inner.session else {
            return http::error(403, "invalid token or IP address");
        };
        if s.id != session_id || s.remote != peer.ip() {
            return http::error(403, "invalid token or IP address");
        }
        let Some(slot) = s.slots.get_mut(file_id) else {
            return http::error(403, "invalid token or IP address");
        };
        if !ct_eq(&slot.token, token) {
            return http::error(403, "invalid token or IP address");
        }
        if slot.state != SlotState::Pending {
            return http::error(409, "this file is already being received");
        }
        slot.state = SlotState::Active;
        s.last_active = Instant::now();
        UploadPlan {
            session_id: s.id.clone(),
            peer: s.peer.alias.clone(),
            dir: s.dir.clone(),
            parts: slot.parts.clone(),
            name: slot.name.clone(),
            meta: slot.meta.clone(),
            cancel: s.cancel.clone(),
            bytes: s.bytes.clone(),
            total: s.total,
        }
    };

    let outcome = receive_file(shared, &plan, req.into_body()).await;
    match outcome {
        Ok(path) => {
            release_slot(rs, &plan.session_id, file_id, SlotState::Done);
            shared.emit_transfer(
                &plan.session_id,
                Direction::Receive,
                &plan.peer,
                TransferKind::FileDone {
                    file: plan.meta.file_name.clone(),
                    path: Some(path),
                },
            );
            finish_if_complete(shared, rs, &plan.session_id);
            http::status(200)
        }
        Err(err) => {
            let cancelled = matches!(err, Error::Cancelled);
            release_slot(rs, &plan.session_id, file_id, SlotState::Pending);
            if cancelled {
                end_session(shared, &plan.session_id);
            } else {
                shared.emit_transfer(
                    &plan.session_id,
                    Direction::Receive,
                    &plan.peer,
                    TransferKind::Failed {
                        error: err.to_string(),
                    },
                );
            }
            match err {
                Error::HashMismatch => http::error(422, "the file hash did not match"),
                Error::Cancelled => http::error(409, "the session was cancelled"),
                Error::Input(m) => http::error(400, &m),
                e => http::error(500, &e.to_string()),
            }
        }
    }
}

fn finish_if_complete(shared: &Arc<Shared>, rs: &ReceiveState, session_id: &str) {
    let mut inner = rs.inner.lock().unwrap();
    let Some(s) = &mut inner.session else { return };
    if s.id != session_id {
        return;
    }
    s.done_files = s
        .slots
        .values()
        .filter(|f| f.state == SlotState::Done)
        .count();
    if s.done_files < s.files_total {
        return;
    }
    let (id, name, files, bytes, elapsed) = (
        s.id.clone(),
        s.peer.alias.clone(),
        s.done_files,
        s.bytes.load(Ordering::Relaxed),
        s.started.elapsed(),
    );
    inner.session = None;
    drop(inner);
    shared.active.lock().unwrap().remove(&id);
    shared.emit_transfer(
        &id,
        Direction::Receive,
        &name,
        TransferKind::Finished {
            files,
            bytes,
            elapsed,
        },
    );
}

async fn receive_file(
    shared: &Arc<Shared>,
    plan: &UploadPlan,
    mut body: Incoming,
) -> Result<PathBuf, Error> {
    let dir = fsx::ensure_dirs(&plan.dir, &plan.parts)?;
    let (std_file, part) = fsx::create_part(&dir, &plan.name, &random_hex(4))?;
    let mut got = 0u64;
    let result = write_body(shared, plan, &mut body, std_file, &mut got).await;
    match result {
        Ok(()) => Ok(part.finalize()?),
        Err(e) => {
            // Take this attempt's bytes back out of the session counter: a retry starts over.
            plan.bytes.fetch_sub(got, Ordering::Relaxed);
            part.discard();
            Err(e)
        }
    }
}

async fn write_body(
    shared: &Arc<Shared>,
    plan: &UploadPlan,
    body: &mut Incoming,
    std_file: std::fs::File,
    got_out: &mut u64,
) -> Result<(), Error> {
    let mut out = BufWriter::with_capacity(256 * 1024, tokio::fs::File::from_std(std_file));
    let size = plan.meta.size;
    let mut hasher = plan.meta.sha256.is_some().then(Sha256::new);
    let got = got_out;
    let mut cancel_rx = plan.cancel.subscribe();
    let mut last_emit = Instant::now();
    loop {
        if plan.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let frame = tokio::select! {
            f = tokio::time::timeout(Duration::from_secs(60), body.frame()) => f?,
            _ = cancel_rx.changed() => return Err(Error::Cancelled),
        };
        let Some(frame) = frame else { break };
        let frame = frame?;
        let Ok(data) = frame.into_data() else {
            continue;
        };
        *got += data.len() as u64;
        if *got > size {
            return Err(Error::Input("more data than declared".into()));
        }
        if let Some(h) = &mut hasher {
            h.update(&data);
        }
        out.write_all(&data).await?;
        let total_done =
            plan.bytes.fetch_add(data.len() as u64, Ordering::Relaxed) + data.len() as u64;
        if last_emit.elapsed() >= Duration::from_millis(100) {
            last_emit = Instant::now();
            shared.emit_transfer(
                &plan.session_id,
                Direction::Receive,
                &plan.peer,
                TransferKind::Progress {
                    file: plan.meta.file_name.clone(),
                    file_done: *got,
                    file_size: size,
                    total_done,
                    total: plan.total,
                },
            );
        }
    }
    if *got != size {
        return Err(Error::Input("less data than declared".into()));
    }
    out.flush().await?;
    if let (Some(h), Some(want)) = (hasher, &plan.meta.sha256)
        && !hex(&h.finalize()).eq_ignore_ascii_case(want)
    {
        return Err(Error::HashMismatch);
    }
    let file = out.into_inner().into_std().await;
    if let Some(m) = plan
        .meta
        .metadata
        .as_ref()
        .and_then(|m| m.modified.as_deref())
        && let Some(secs) = parse_iso8601(m)
    {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if secs > 0 && secs <= now + 86_400 {
            let _ = file.set_modified(UNIX_EPOCH + Duration::from_secs(secs as u64));
        }
    }
    drop(file);
    Ok(())
}
