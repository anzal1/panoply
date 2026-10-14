//! The sending side: expand files and folders, `prepare-upload`, upload in parallel over
//! reused connections, progress and cancel.

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio::task::JoinHandle;

use crate::error::{Error, Result};
use crate::fsx::mime_for;
use crate::http::{self, Conn, chan_body, message_of, url_encode};
use crate::node::{Canceller, Device, Direction, Shared, TransferKind};
use crate::proto::{
    API_PREFIX, FileMeta, FileTimes, PrepareUploadRequest, PrepareUploadResponse, hex, iso8601,
    random_hex,
};
use crate::receive::BoxFuture;

const CHUNK: usize = 256 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// The receiver may be waiting on a person, so the prepare call is patient.
const PREPARE_TIMEOUT: Duration = Duration::from_secs(120);
const PIN_ATTEMPTS: u32 = 3;

/// Asks for a PIN when the receiver wants one. Called with the attempt number (1 first);
/// return `None` to give up.
pub type PinProvider = Arc<dyn Fn(u32) -> BoxFuture<Option<String>> + Send + Sync>;

/// Settings for one send.
#[derive(Clone)]
pub struct SendOptions {
    /// PIN to try first.
    pub pin: Option<String>,
    /// Called when the receiver answers 401.
    pub pin_provider: Option<PinProvider>,
    /// Hash every file first and send the SHA-256, so the receiver can verify the content.
    pub hash_files: bool,
    /// Parallel upload connections.
    pub workers: usize,
}

impl Default for SendOptions {
    fn default() -> Self {
        SendOptions {
            pin: None,
            pin_provider: None,
            hash_files: false,
            workers: 4,
        }
    }
}

/// What a finished send moved.
#[derive(Debug, Clone)]
pub struct SendSummary {
    /// Files sent.
    pub files: usize,
    /// Bytes sent.
    pub bytes: u64,
    /// Wall time including the wait for the receiver to accept.
    pub elapsed: Duration,
    /// True when the receiver took a text message and needed no transfer.
    pub text_delivered: bool,
}

/// A send in progress.
pub struct SendJob {
    id: String,
    cancel: Canceller,
    handle: JoinHandle<Result<SendSummary>>,
}

impl SendJob {
    /// Identifies the transfer in events.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Stop sending. The receiver is told, and partial files are removed on its side.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// A handle that cancels this job after [`SendJob::wait`] has taken the job.
    pub fn cancel_handle(&self) -> SendCancel {
        SendCancel(self.cancel.clone())
    }

    /// Wait for the end.
    pub async fn wait(self) -> Result<SendSummary> {
        match self.handle.await {
            Ok(r) => r,
            Err(e) => Err(Error::Io(io::Error::other(e))),
        }
    }
}

/// Cancels a send from anywhere.
#[derive(Clone)]
pub struct SendCancel(Canceller);

impl SendCancel {
    /// Stop the send.
    pub fn cancel(&self) {
        self.0.cancel();
    }
}

pub(crate) enum Payload {
    Files(Vec<PathBuf>),
    Text(String),
}

#[derive(Clone)]
pub(crate) struct Item {
    pub(crate) id: String,
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) mime: &'static str,
    pub(crate) modified: Option<i64>,
    pub(crate) accessed: Option<i64>,
    pub(crate) sha256: Option<String>,
}

fn secs(t: io::Result<SystemTime>) -> Option<i64> {
    t.ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

fn item_for(path: &Path, name: String, md: &std::fs::Metadata) -> Item {
    Item {
        id: String::new(),
        path: path.to_path_buf(),
        name,
        size: md.len(),
        mime: mime_for(path),
        modified: secs(md.modified()),
        accessed: secs(md.accessed()),
        sha256: None,
    }
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<Item>) -> io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = format!("{prefix}/{}", e.file_name().to_string_lossy());
        // Links inside a folder are skipped: a link out of the tree must not leak what it
        // points to, and a link back into it must not loop.
        let ft = e.file_type()?;
        if ft.is_dir() {
            walk(&e.path(), &name, out)?;
        } else if ft.is_file() {
            out.push(item_for(&e.path(), name, &e.metadata()?));
        }
    }
    Ok(())
}

pub(crate) fn expand(paths: &[PathBuf]) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    for p in paths {
        let md = std::fs::metadata(p).map_err(|e| Error::Input(format!("{}: {e}", p.display())))?;
        let base = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        if md.is_dir() {
            walk(p, &base, &mut items)
                .map_err(|e| Error::Input(format!("{}: {e}", p.display())))?;
        } else if md.is_file() {
            items.push(item_for(p, base, &md));
        } else {
            return Err(Error::Input(format!(
                "{} is not a file or folder",
                p.display()
            )));
        }
    }
    if items.is_empty() {
        return Err(Error::Input("there is nothing to send".into()));
    }
    for (i, it) in items.iter_mut().enumerate() {
        it.id = format!("f{i}");
    }
    Ok(items)
}

fn hash_file(path: &Path) -> io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

fn meta_of(it: &Item, preview: Option<String>) -> FileMeta {
    FileMeta {
        id: it.id.clone(),
        file_name: it.name.clone(),
        size: it.size,
        file_type: it.mime.to_string(),
        sha256: it.sha256.clone(),
        preview,
        metadata: Some(FileTimes {
            modified: it.modified.map(iso8601),
            accessed: it.accessed.map(iso8601),
        }),
    }
}

pub(crate) fn start(
    shared: Arc<Shared>,
    target: Device,
    payload: Payload,
    options: SendOptions,
) -> SendJob {
    let id = random_hex(8);
    let cancel = Canceller::new();
    shared
        .active
        .lock()
        .unwrap()
        .insert(id.clone(), cancel.clone());
    let (job_id, job_cancel) = (id.clone(), cancel.clone());
    let handle = tokio::spawn(async move {
        let started = Instant::now();
        let peer = target.info.alias.clone();
        let result = run(&shared, &target, payload, &options, &id, &cancel, started).await;
        shared.active.lock().unwrap().remove(&id);
        match &result {
            Ok(s) => shared.emit_transfer(
                &id,
                Direction::Send,
                &peer,
                TransferKind::Finished {
                    files: s.files,
                    bytes: s.bytes,
                    elapsed: s.elapsed,
                },
            ),
            Err(Error::Cancelled) => {
                shared.emit_transfer(&id, Direction::Send, &peer, TransferKind::Cancelled)
            }
            Err(e) => shared.emit_transfer(
                &id,
                Direction::Send,
                &peer,
                TransferKind::Failed {
                    error: e.to_string(),
                },
            ),
        }
        result
    });
    SendJob {
        id: job_id,
        cancel: job_cancel,
        handle,
    }
}

fn status_error(code: u16, body: &[u8]) -> Error {
    match code {
        401 => Error::PinRequired,
        403 => Error::Rejected,
        409 => Error::Busy,
        422 => Error::HashMismatch,
        429 => Error::TooManyRequests,
        status => Error::Remote {
            status,
            message: message_of(body),
        },
    }
}

async fn open(target: &Device) -> Result<Conn> {
    http::connect(
        target.socket_addr(),
        target.info.scheme_or_default(),
        Some(target.fingerprint()),
        CONNECT_TIMEOUT,
    )
    .await
}

async fn run(
    shared: &Arc<Shared>,
    target: &Device,
    payload: Payload,
    options: &SendOptions,
    id: &str,
    cancel: &Canceller,
    started: Instant,
) -> Result<SendSummary> {
    if !target.info.version_compatible() {
        return Err(Error::IncompatibleVersion(target.info.version.clone()));
    }

    // Build the file list.
    let (items, text) = match payload {
        Payload::Files(paths) => {
            let hash = options.hash_files;
            let items = tokio::task::spawn_blocking(move || -> Result<Vec<Item>> {
                let mut items = expand(&paths)?;
                if hash {
                    for it in &mut items {
                        it.sha256 = Some(hash_file(&it.path)?);
                    }
                }
                Ok(items)
            })
            .await
            .map_err(|e| Error::Io(io::Error::other(e)))??;
            (items, None)
        }
        Payload::Text(t) => {
            let it = Item {
                id: "msg".into(),
                path: PathBuf::new(),
                name: format!("{}.txt", random_hex(4)),
                size: t.len() as u64,
                mime: "text/plain",
                modified: None,
                accessed: None,
                sha256: None,
            };
            (vec![it], Some(t))
        }
    };
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    // prepare-upload, with PIN retries.
    let mut conn = Some(open(target).await?);
    let request = PrepareUploadRequest {
        info: shared.info(None),
        files: items
            .iter()
            .map(|it| (it.id.clone(), meta_of(it, text.clone())))
            .collect(),
    };
    let body = serde_json::to_vec(&request)?;
    let mut pin = options.pin.clone();
    let mut attempt = 0;
    let prepared = loop {
        let path = match &pin {
            Some(p) => format!("{API_PREFIX}/prepare-upload?pin={}", url_encode(p)),
            None => format!("{API_PREFIX}/prepare-upload"),
        };
        let mut cancel_rx = cancel.subscribe();
        let call = tokio::time::timeout(
            PREPARE_TIMEOUT,
            conn.as_mut().expect("open until the uploads start").call(
                hyper::Method::POST,
                &path,
                http::full(body.clone()),
            ),
        );
        let (code, bytes) = tokio::select! {
            r = call => r??,
            _ = cancel_rx.changed() => return Err(Error::Cancelled),
        };
        match code {
            204 => {
                return Ok(SendSummary {
                    files: items.len(),
                    bytes: items.iter().map(|i| i.size).sum(),
                    elapsed: started.elapsed(),
                    text_delivered: text.is_some(),
                });
            }
            200 => break serde_json::from_slice::<PrepareUploadResponse>(&bytes)?,
            401 => {
                attempt += 1;
                let next = match &options.pin_provider {
                    Some(p) if attempt < PIN_ATTEMPTS + 1 => p(attempt).await,
                    _ => None,
                };
                match next {
                    Some(p) => pin = Some(p),
                    None => return Err(Error::PinRequired),
                }
            }
            c => return Err(status_error(c, &bytes)),
        }
    };

    // Upload what the receiver accepted.
    let accepted: Vec<Item> = items
        .into_iter()
        .filter(|i| prepared.files.contains_key(&i.id))
        .collect();
    if accepted.is_empty() {
        return Err(Error::Rejected);
    }
    let total: u64 = accepted.iter().map(|i| i.size).sum();
    let n_files = accepted.len();
    shared.emit_transfer(
        id,
        Direction::Send,
        &target.info.alias,
        TransferKind::Started {
            files: n_files,
            bytes: total,
        },
    );

    let ctx = Arc::new(UploadCtx {
        shared: shared.clone(),
        target: target.clone(),
        transfer_id: id.to_string(),
        session_id: prepared.session_id.clone(),
        tokens: prepared.files,
        cancel: cancel.clone(),
        failed: AtomicBool::new(false),
        done: AtomicU64::new(0),
        total,
        last_emit: Mutex::new(Instant::now()),
    });
    let queue = Arc::new(Mutex::new(VecDeque::from(accepted)));
    let workers = options.workers.clamp(1, 16).min(n_files);
    let mut tasks = Vec::new();
    for w in 0..workers {
        let ctx = ctx.clone();
        let queue = queue.clone();
        let first = if w == 0 { conn.take() } else { None };
        tasks.push(tokio::spawn(async move { worker(ctx, queue, first).await }));
    }
    let mut first_err: Option<Error> = None;
    for t in tasks {
        let r = match t.await {
            Ok(r) => r,
            Err(e) => Err(Error::Io(io::Error::other(e))),
        };
        if let Err(e) = r
            && first_err.is_none()
        {
            first_err = Some(e);
        }
    }
    if let Some(e) = first_err {
        // Tell the receiver to drop the session so its next request is not blocked.
        let _ = best_effort_cancel(target, &prepared.session_id).await;
        return Err(e);
    }
    Ok(SendSummary {
        files: n_files,
        bytes: total,
        elapsed: started.elapsed(),
        text_delivered: false,
    })
}

struct UploadCtx {
    shared: Arc<Shared>,
    target: Device,
    transfer_id: String,
    session_id: String,
    tokens: std::collections::HashMap<String, String>,
    cancel: Canceller,
    failed: AtomicBool,
    done: AtomicU64,
    total: u64,
    last_emit: Mutex<Instant>,
}

impl UploadCtx {
    fn progress(&self, file: &str, file_done: u64, file_size: u64, force: bool) {
        let mut last = self.last_emit.lock().unwrap();
        if !force && last.elapsed() < Duration::from_millis(100) {
            return;
        }
        *last = Instant::now();
        self.shared.emit_transfer(
            &self.transfer_id,
            Direction::Send,
            &self.target.info.alias,
            TransferKind::Progress {
                file: file.to_string(),
                file_done,
                file_size,
                total_done: self.done.load(Ordering::Relaxed),
                total: self.total,
            },
        );
    }
}

async fn worker(
    ctx: Arc<UploadCtx>,
    queue: Arc<Mutex<VecDeque<Item>>>,
    first: Option<Conn>,
) -> Result<()> {
    let mut conn = first;
    loop {
        if ctx.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if ctx.failed.load(Ordering::Relaxed) {
            return Ok(());
        }
        let Some(item) = queue.lock().unwrap().pop_front() else {
            return Ok(());
        };
        let res = upload_with_retry(&ctx, &mut conn, &item).await;
        if let Err(e) = res {
            ctx.failed.store(true, Ordering::Relaxed);
            return Err(e);
        }
        ctx.shared.emit_transfer(
            &ctx.transfer_id,
            Direction::Send,
            &ctx.target.info.alias,
            TransferKind::FileDone {
                file: item.name.clone(),
                path: None,
            },
        );
    }
}

async fn upload_with_retry(
    ctx: &Arc<UploadCtx>,
    conn: &mut Option<Conn>,
    item: &Item,
) -> Result<()> {
    let mut retried = false;
    loop {
        if conn.as_ref().is_none_or(Conn::is_closed) {
            *conn = Some(open(&ctx.target).await?);
        }
        let c = conn.as_mut().expect("connection was just opened");
        match upload_one(ctx, c, item).await {
            Ok(()) => return Ok(()),
            // A keep-alive connection the receiver closed while idle: reconnect once.
            Err(Error::Io(_)) if !retried && !ctx.cancel.is_cancelled() => {
                retried = true;
                *conn = None;
            }
            Err(_) if ctx.cancel.is_cancelled() => return Err(Error::Cancelled),
            Err(e) => return Err(e),
        }
    }
}

async fn upload_one(ctx: &Arc<UploadCtx>, conn: &mut Conn, item: &Item) -> Result<()> {
    let token = ctx
        .tokens
        .get(&item.id)
        .ok_or_else(|| Error::Input("no token for a file".into()))?;
    let path = format!(
        "{API_PREFIX}/upload?sessionId={}&fileId={}&token={}",
        url_encode(&ctx.session_id),
        url_encode(&item.id),
        url_encode(token)
    );
    let (tx, body) = chan_body(Some(item.size), 4);
    let sent = Arc::new(AtomicU64::new(0));
    let producer = {
        let (ctx, item, sent) = (ctx.clone(), item.clone(), sent.clone());
        tokio::spawn(async move {
            let mut f = match tokio::fs::File::open(&item.path).await {
                Ok(f) => f,
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    return;
                }
            };
            let mut buf = vec![0u8; CHUNK];
            let mut total = 0u64;
            loop {
                if ctx.cancel.is_cancelled() {
                    let _ = tx.send(Err(io::Error::other("cancelled"))).await;
                    return;
                }
                let n = match f.read(&mut buf).await {
                    Ok(n) => n,
                    Err(e) => {
                        let _ = tx.send(Err(e)).await;
                        return;
                    }
                };
                if n == 0 {
                    break;
                }
                total += n as u64;
                if total > item.size {
                    let _ = tx
                        .send(Err(io::Error::other("the file grew while sending")))
                        .await;
                    return;
                }
                if tx
                    .send(Ok(Bytes::copy_from_slice(&buf[..n])))
                    .await
                    .is_err()
                {
                    return;
                }
                sent.fetch_add(n as u64, Ordering::Relaxed);
                ctx.done.fetch_add(n as u64, Ordering::Relaxed);
                ctx.progress(&item.name, total, item.size, false);
            }
            if total != item.size {
                let _ = tx
                    .send(Err(io::Error::other("the file shrank while sending")))
                    .await;
            }
        })
    };
    let mut cancel_rx = ctx.cancel.subscribe();
    let req = conn.send_typed(hyper::Method::POST, &path, "application/octet-stream", body);
    let outcome: Result<(u16, Bytes)> = tokio::select! {
        r = req => match r {
            Ok(resp) => {
                let code = resp.status().as_u16();
                http::read_limited(resp.into_body(), 64 * 1024).await.map(|b| (code, b))
            }
            Err(e) => Err(e),
        },
        _ = cancel_rx.changed() => Err(Error::Cancelled),
    };
    producer.abort();
    match outcome {
        Ok((200, _)) => {
            if item.size >= 1 << 20 {
                ctx.progress(&item.name, item.size, item.size, true);
            }
            Ok(())
        }
        Ok((code, body)) => {
            ctx.done
                .fetch_sub(sent.load(Ordering::Relaxed), Ordering::Relaxed);
            Err(status_error(code, &body))
        }
        Err(e) => {
            ctx.done
                .fetch_sub(sent.load(Ordering::Relaxed), Ordering::Relaxed);
            Err(e)
        }
    }
}

async fn best_effort_cancel(target: &Device, session_id: &str) -> Result<()> {
    let mut conn = open(target).await?;
    let path = format!("{API_PREFIX}/cancel?sessionId={}", url_encode(session_id));
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        conn.call(hyper::Method::POST, &path, http::empty()),
    )
    .await;
    Ok(())
}
