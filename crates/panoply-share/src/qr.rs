//! The QR fallback: a one-time web page on the local network for devices without Panoply.
//!
//! The URL carries a 128 bit random token. The page lists the files, offers each one (with
//! range support so browsers resume) and a streamed zip of everything, and can accept uploads
//! into a folder. It works without JavaScript and makes no external requests. The share closes
//! after the first completed transfer or after the expiry, whichever is first, and can be
//! revoked at any moment.

use std::collections::{HashMap, HashSet};
use std::io::SeekFrom;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::{Method, Request, Response};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufWriter};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, watch};

use crate::error::{Error, Result};
use crate::fsx;
use crate::http::{self, Body, chan_body, url_encode};
use crate::identity::Identity;
use crate::multipart::{Multipart, Part, boundary_of};
use crate::netif;
use crate::proto::random_hex;
use crate::send::expand;
use crate::zip::{ZipEntry, ZipPlan};

const CHUNK: usize = 256 * 1024;

/// What to share and how.
#[derive(Clone)]
pub struct QrOptions {
    /// Files and folders offered for download. May be empty when `upload_dir` is set.
    pub files: Vec<PathBuf>,
    /// Let the other device upload into this folder.
    pub upload_dir: Option<PathBuf>,
    /// Ask for this PIN before showing anything.
    pub pin: Option<String>,
    /// Serve HTTPS with this identity (browsers warn about the self-signed certificate).
    /// `None` is plain HTTP.
    pub tls: Option<Arc<Identity>>,
    /// The share closes after this long (default 10 minutes).
    pub expires: Duration,
    /// Address to bind. Default `0.0.0.0`.
    pub bind: IpAddr,
    /// Port. `0` picks a random free one.
    pub port: u16,
    /// Address to put in the URL. Default: the first private LAN address.
    pub advertise: Option<IpAddr>,
    /// Name of the zip download.
    pub zip_name: String,
    /// Largest accepted upload in bytes.
    pub max_upload_bytes: u64,
    /// Write Zip64 records even for small archives. For tests.
    #[doc(hidden)]
    pub force_zip64: bool,
}

impl Default for QrOptions {
    fn default() -> Self {
        QrOptions {
            files: Vec::new(),
            upload_dir: None,
            pin: None,
            tls: None,
            expires: Duration::from_secs(600),
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 0,
            advertise: None,
            zip_name: "panoply-share.zip".into(),
            max_upload_bytes: 64 << 30,
            force_zip64: false,
        }
    }
}

/// Why a share closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrOutcome {
    /// A download or upload completed.
    Completed,
    /// Nobody finished before the expiry.
    Expired,
    /// The owner closed it.
    Revoked,
}

/// Things that happen on a share, for a live list.
#[derive(Debug, Clone)]
pub enum QrEvent {
    /// A browser opened the page.
    Connected {
        /// Its address.
        ip: IpAddr,
        /// Its `User-Agent`.
        user_agent: String,
    },
    /// A download started.
    DownloadStarted {
        /// Who.
        ip: IpAddr,
        /// What.
        file: String,
    },
    /// A download (or the last part of a resumed one) was fully sent.
    Downloaded {
        /// Who.
        ip: IpAddr,
        /// What (`zip` for the archive).
        file: String,
        /// Bytes sent in that response.
        bytes: u64,
    },
    /// A file was uploaded and saved.
    Uploaded {
        /// Who.
        ip: IpAddr,
        /// Where it was saved.
        path: PathBuf,
        /// Size.
        bytes: u64,
    },
    /// The share closed.
    Closed(QrOutcome),
}

struct FileItem {
    name: String,
    path: PathBuf,
    size: u64,
    mtime: i64,
}

struct State {
    token: String,
    scheme: &'static str,
    files: Vec<FileItem>,
    upload_dir: Option<PathBuf>,
    pin: Option<String>,
    zip_name: String,
    max_upload: u64,
    force_zip64: bool,
    sessions: Mutex<HashSet<String>>,
    pin_fails: Mutex<HashMap<IpAddr, (u32, Instant)>>,
    seen: Mutex<HashSet<(IpAddr, String)>>,
    delivered: Mutex<HashSet<usize>>,
    zip_done: AtomicBool,
    uploads: AtomicU32,
    closed: watch::Sender<Option<QrOutcome>>,
    shutdown: watch::Sender<bool>,
    events: broadcast::Sender<QrEvent>,
}

impl State {
    fn close(&self, outcome: QrOutcome) {
        let first = self.closed.send_if_modified(|v| {
            if v.is_none() {
                *v = Some(outcome);
                true
            } else {
                false
            }
        });
        if first {
            let _ = self.events.send(QrEvent::Closed(outcome));
            let _ = self.shutdown.send(true);
        }
    }

    fn emit(&self, e: QrEvent) {
        let _ = self.events.send(e);
    }

    fn check_complete(&self) {
        let all =
            !self.files.is_empty() && self.delivered.lock().unwrap().len() == self.files.len();
        if all || self.zip_done.load(Ordering::Relaxed) {
            self.close(QrOutcome::Completed);
        }
    }
}

/// A running one-time share.
pub struct QrShare {
    url: String,
    state: Arc<State>,
    closed: watch::Receiver<Option<QrOutcome>>,
    /// The server task, taken by the first `wait` so the process only goes on once the last
    /// response has been written out.
    server: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    lan: bool,
}

impl QrShare {
    /// Start serving. Must run inside a tokio runtime.
    pub async fn start(opts: QrOptions) -> Result<QrShare> {
        if opts.files.is_empty() && opts.upload_dir.is_none() {
            return Err(Error::Input("there is nothing to share".into()));
        }
        let items = if opts.files.is_empty() {
            Vec::new()
        } else {
            let paths = opts.files.clone();
            tokio::task::spawn_blocking(move || expand(&paths))
                .await
                .map_err(|e| Error::Io(std::io::Error::other(e)))??
        };
        let files: Vec<FileItem> = items
            .into_iter()
            .map(|i| FileItem {
                name: i.name,
                path: i.path,
                size: i.size,
                mtime: i.modified.unwrap_or(0),
            })
            .collect();
        if let Some(d) = &opts.upload_dir {
            std::fs::create_dir_all(d)?;
        }

        let tls = opts.tls.as_ref().map(|i| i.server_config()).transpose()?;
        let listener = TcpListener::bind(SocketAddr::new(opts.bind, opts.port))
            .await
            .map_err(|e| Error::Bind(format!("cannot listen: {e}")))?;
        let port = listener.local_addr()?.port();

        let lan_ip = netif::lan_interfaces()
            .into_iter()
            .next()
            .map(|l| IpAddr::V4(l.ip));
        let (host, lan) = match (opts.advertise, lan_ip) {
            (Some(a), _) => (a, true),
            (None, _) if !opts.bind.is_unspecified() => (opts.bind, !opts.bind.is_loopback()),
            (None, Some(ip)) => (ip, true),
            (None, None) => (IpAddr::V4(Ipv4Addr::LOCALHOST), false),
        };
        let token = random_hex(16);
        let scheme = if tls.is_some() { "https" } else { "http" };
        let url = format!("{scheme}://{}:{port}/{token}", fmt_host(host));

        let (closed_tx, closed_rx) = watch::channel(None);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (events, _) = broadcast::channel(256);
        let state = Arc::new(State {
            token,
            scheme,
            files,
            upload_dir: opts.upload_dir,
            pin: opts.pin,
            zip_name: fsx::sanitize_component(&opts.zip_name),
            max_upload: opts.max_upload_bytes,
            force_zip64: opts.force_zip64,
            sessions: Mutex::default(),
            pin_fails: Mutex::default(),
            seen: Mutex::default(),
            delivered: Mutex::default(),
            zip_done: AtomicBool::new(false),
            uploads: AtomicU32::new(0),
            closed: closed_tx,
            shutdown: shutdown_tx,
            events,
        });

        let handler_state = state.clone();
        let server = tokio::spawn(http::serve(
            listener,
            tls,
            move |req, peer| route(handler_state.clone(), req, peer),
            shutdown_rx,
        ));
        let expiry_state = state.clone();
        let expires = opts.expires;
        tokio::spawn(async move {
            let mut rx = expiry_state.shutdown.subscribe();
            tokio::select! {
                _ = tokio::time::sleep(expires) => expiry_state.close(QrOutcome::Expired),
                _ = rx.changed() => {}
            }
        });
        Ok(QrShare {
            url,
            state,
            closed: closed_rx,
            server: tokio::sync::Mutex::new(Some(server)),
            lan,
        })
    }

    /// The link to open on the other device.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// False when the computer has no private network address and the URL points at
    /// localhost: the other device cannot reach it.
    pub fn on_lan(&self) -> bool {
        self.lan
    }

    /// `"http"` or `"https"`.
    pub fn scheme(&self) -> &'static str {
        self.state.scheme
    }

    /// The URL's secret token.
    pub fn token(&self) -> &str {
        &self.state.token
    }

    /// Subscribe to connection and transfer events.
    pub fn subscribe(&self) -> broadcast::Receiver<QrEvent> {
        self.state.events.subscribe()
    }

    /// Close the share now.
    pub fn revoke(&self) {
        self.state.close(QrOutcome::Revoked);
    }

    /// Wait until the share closes and say why.
    pub async fn wait(&self) -> QrOutcome {
        let mut rx = self.closed.clone();
        let outcome = match rx.wait_for(|v| v.is_some()).await {
            Ok(v) => v.expect("wait_for returned a closed value"),
            Err(_) => QrOutcome::Revoked,
        };
        // The share closes the moment the last chunk is queued, which can be before it has
        // reached the socket. Let the connections finish so the last download is whole.
        if let Some(server) = self.server.lock().await.take() {
            let _ =
                tokio::time::timeout(http::DRAIN_TIMEOUT + Duration::from_secs(1), server).await;
        }
        outcome
    }
}

impl Drop for QrShare {
    fn drop(&mut self) {
        self.state.close(QrOutcome::Revoked);
    }
}

fn fmt_host(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v) => v.to_string(),
        IpAddr::V6(v) => format!("[{v}]"),
    }
}

/// The QR code for `url` as text for a terminal, using half block characters.
///
/// Dark terminals need the inverted rendering (light modules drawn as filled blocks) to scan;
/// pass `invert = false` for a light terminal.
pub fn terminal_qr(url: &str, invert: bool) -> Result<String> {
    use qrcode::render::unicode::Dense1x2;
    let code = qrcode::QrCode::with_error_correction_level(url.as_bytes(), qrcode::EcLevel::L)
        .map_err(|e| Error::Input(format!("cannot make a QR code: {e}")))?;
    let (dark, light) = if invert {
        (Dense1x2::Light, Dense1x2::Dark)
    } else {
        (Dense1x2::Dark, Dense1x2::Light)
    };
    Ok(code
        .render::<Dense1x2>()
        .dark_color(dark)
        .light_color(light)
        .quiet_zone(true)
        .build())
}

/// The QR code for `url` as a square of modules for a GUI to draw: the side length (quiet zone
/// not included) and the modules row by row, `true` for a dark one. The caller draws its own
/// four module quiet zone.
pub fn qr_modules(url: &str) -> Result<(usize, Vec<bool>)> {
    use qrcode::Color;
    let code = qrcode::QrCode::with_error_correction_level(url.as_bytes(), qrcode::EcLevel::L)
        .map_err(|e| Error::Input(format!("cannot make a QR code: {e}")))?;
    let side = code.width();
    Ok((
        side,
        code.to_colors()
            .into_iter()
            .map(|c| c == Color::Dark)
            .collect(),
    ))
}

// ---------------------------------------------------------------- requests

fn secure(mut r: Response<Body>) -> Response<Body> {
    let h = r.headers_mut();
    h.insert("cache-control", "no-store".parse().unwrap());
    h.insert("x-content-type-options", "nosniff".parse().unwrap());
    h.insert("referrer-policy", "no-referrer".parse().unwrap());
    h.insert("x-frame-options", "DENY".parse().unwrap());
    h.insert(
        "content-security-policy",
        "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'"
            .parse()
            .unwrap(),
    );
    r
}

fn html(code: u16, body: String) -> Response<Body> {
    let mut r = Response::new(http::full(body));
    *r.status_mut() = hyper::StatusCode::from_u16(code).unwrap();
    r.headers_mut()
        .insert("content-type", "text/html; charset=utf-8".parse().unwrap());
    secure(r)
}

fn not_found() -> Response<Body> {
    // The same answer for a wrong token and for a wrong path, so neither is a signal.
    html(404, page("Not found", "<p>Nothing here.</p>"))
}

fn esc(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&#39;".to_string(),
            c => c.to_string(),
        })
        .collect()
}

fn human(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=en><head><meta charset=utf-8>\
<meta name=viewport content=\"width=device-width,initial-scale=1\">\
<meta name=robots content=noindex><title>{}</title><style>\
:root{{color-scheme:light dark;--bg:#fff;--fg:#1a1a1a;--mut:#666;--line:#ddd;--acc:#0b57d0}}\
@media(prefers-color-scheme:dark){{:root{{--bg:#151515;--fg:#eee;--mut:#999;--line:#333;--acc:#8ab4f8}}}}\
body{{margin:0;background:var(--bg);color:var(--fg);font:16px/1.45 system-ui,sans-serif}}\
main{{max-width:34rem;margin:0 auto;padding:1.5rem 1rem}}\
h1{{font-size:1.25rem;margin:0 0 1rem}}\
ul{{list-style:none;margin:0 0 1rem;padding:0}}\
li{{display:flex;justify-content:space-between;gap:1rem;padding:.7rem 0;border-top:1px solid var(--line)}}\
li:last-child{{border-bottom:1px solid var(--line)}}\
a{{color:var(--acc);word-break:break-all}}span{{color:var(--mut);white-space:nowrap}}\
.btn,button{{display:inline-block;padding:.7rem 1.1rem;border-radius:.5rem;border:0;background:var(--acc);color:var(--bg);font:inherit;text-decoration:none}}\
input{{font:inherit;padding:.6rem;border:1px solid var(--line);border-radius:.4rem;background:transparent;color:inherit;max-width:100%}}\
p.note{{color:var(--mut);font-size:.9rem}}</style></head><body><main><h1>{}</h1>{}</main></body></html>",
        esc(title),
        esc(title),
        body
    )
}

fn authorized(state: &State, req: &Request<Incoming>) -> bool {
    if state.pin.is_none() {
        return true;
    }
    let sessions = state.sessions.lock().unwrap();
    req.headers()
        .get_all("cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().strip_prefix("ps="))
        .any(|v| sessions.contains(v))
}

fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= (*a.get(i).unwrap_or(&0) ^ *b.get(i).unwrap_or(&0)) as usize;
    }
    diff == 0
}

async fn route(state: Arc<State>, req: Request<Incoming>, peer: SocketAddr) -> Response<Body> {
    let path = req.uri().path().to_string();
    let mut segs = path.trim_start_matches('/').split('/');
    let Some(tok) = segs.next() else {
        return not_found();
    };
    if !ct_eq(tok, &state.token) {
        return not_found();
    }
    let rest: Vec<&str> = segs.filter(|s| !s.is_empty()).collect();
    let head = req.method() == Method::HEAD;
    match (req.method(), rest.as_slice()) {
        (&Method::GET | &Method::HEAD, []) => landing(&state, &req, peer),
        (&Method::POST, ["pin"]) => submit_pin(&state, req, peer).await,
        _ if !authorized(&state, &req) => html(
            403,
            page(
                "PIN needed",
                "<p>Open the link again and enter the PIN.</p>",
            ),
        ),
        (&Method::GET | &Method::HEAD, ["d", i]) => {
            match i.parse::<usize>().ok().filter(|i| *i < state.files.len()) {
                Some(i) => download(&state, &req, peer, i, head).await,
                None => not_found(),
            }
        }
        (&Method::GET | &Method::HEAD, ["zip"]) if state.files.len() > 1 => zip(&state, peer, head),
        (&Method::POST, ["upload"]) if state.upload_dir.is_some() => {
            upload(&state, req, peer).await
        }
        _ => not_found(),
    }
}

fn landing(state: &Arc<State>, req: &Request<Incoming>, peer: SocketAddr) -> Response<Body> {
    let ua = req
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .chars()
        .take(200)
        .collect::<String>();
    if state.seen.lock().unwrap().insert((peer.ip(), ua.clone())) {
        state.emit(QrEvent::Connected {
            ip: peer.ip(),
            user_agent: ua,
        });
    }
    if !authorized(state, req) {
        return html(
            200,
            page(
                "Enter the PIN",
                &format!(
                    "<form method=post action=\"/{}/pin\"><p><input name=pin type=password inputmode=numeric autocomplete=off autofocus required></p><p><button>Continue</button></p></form>",
                    state.token
                ),
            ),
        );
    }
    let mut body = String::new();
    if !state.files.is_empty() {
        body.push_str("<ul>");
        for (i, f) in state.files.iter().enumerate() {
            body.push_str(&format!(
                "<li><a href=\"/{}/d/{i}\" download=\"{}\">{}</a><span>{}</span></li>",
                state.token,
                esc(f.name.rsplit('/').next().unwrap_or(&f.name)),
                esc(&f.name),
                human(f.size)
            ));
        }
        body.push_str("</ul>");
        if state.files.len() > 1 {
            let total: u64 = state.files.iter().map(|f| f.size).sum();
            body.push_str(&format!(
                "<p><a class=btn href=\"/{}/zip\">Download all as zip ({})</a></p>",
                state.token,
                human(total)
            ));
        }
    }
    if state.upload_dir.is_some() {
        body.push_str(&format!(
            "<h2 style=\"font-size:1.05rem\">Send files to this computer</h2><form method=post action=\"/{}/upload\" enctype=\"multipart/form-data\"><p><input type=file name=f multiple required></p><p><button>Upload</button></p></form>",
            state.token
        ));
    }
    body.push_str("<p class=note>This link works once and closes when the transfer is done.</p>");
    html(200, page("Panoply share", &body))
}

async fn submit_pin(
    state: &Arc<State>,
    req: Request<Incoming>,
    peer: SocketAddr,
) -> Response<Body> {
    let Some(want) = &state.pin else {
        return not_found();
    };
    {
        let fails = state.pin_fails.lock().unwrap();
        if let Some((n, t)) = fails.get(&peer.ip())
            && *n >= 5
            && t.elapsed() < Duration::from_secs(60)
        {
            return html(
                429,
                page("Too many tries", "<p>Wait a minute and try again.</p>"),
            );
        }
    }
    let Ok(body) = http::read_limited(req.into_body(), 1024).await else {
        return html(400, page("Bad request", "<p>That did not work.</p>"));
    };
    let form = http::query_pairs(Some(&String::from_utf8_lossy(&body)));
    let given = http::query_get(&form, "pin").unwrap_or("");
    if !ct_eq(given, want) {
        let mut fails = state.pin_fails.lock().unwrap();
        let e = fails.entry(peer.ip()).or_insert((0, Instant::now()));
        if e.1.elapsed() >= Duration::from_secs(60) {
            *e = (0, Instant::now());
        }
        e.0 += 1;
        e.1 = Instant::now();
        return html(
            401,
            page(
                "Wrong PIN",
                &format!(
                    "<p>That PIN is not right.</p><p><a class=btn href=\"/{}\">Try again</a></p>",
                    state.token
                ),
            ),
        );
    }
    state.pin_fails.lock().unwrap().remove(&peer.ip());
    let sid = random_hex(16);
    state.sessions.lock().unwrap().insert(sid.clone());
    let mut r = Response::new(http::empty());
    *r.status_mut() = hyper::StatusCode::SEE_OTHER;
    r.headers_mut()
        .insert("location", format!("/{}", state.token).parse().unwrap());
    r.headers_mut().insert(
        "set-cookie",
        format!("ps={sid}; Path=/{}; HttpOnly; SameSite=Strict", state.token)
            .parse()
            .unwrap(),
    );
    secure(r)
}

fn disposition(name: &str) -> String {
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_control() && c != '"' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!(
        "attachment; filename=\"{ascii}\"; filename*=UTF-8''{}",
        url_encode(name)
    )
}

/// One `bytes=` range. `Ok(None)` means serve everything.
fn parse_range(h: &str, size: u64) -> std::result::Result<Option<(u64, u64)>, ()> {
    let Some(spec) = h.trim().strip_prefix("bytes=") else {
        return Ok(None);
    };
    if spec.contains(',') {
        return Ok(None);
    }
    let Some((a, b)) = spec.split_once('-') else {
        return Ok(None);
    };
    let (start, end) = if a.is_empty() {
        let n: u64 = b.parse().map_err(|_| ())?;
        if n == 0 {
            return Err(());
        }
        (size.saturating_sub(n), size.saturating_sub(1))
    } else {
        let s: u64 = a.parse().map_err(|_| ())?;
        let e: u64 = if b.is_empty() {
            size.saturating_sub(1)
        } else {
            b.parse().map_err(|_| ())?
        };
        (s, e.min(size.saturating_sub(1)))
    };
    if size == 0 || start >= size || start > end {
        return Err(());
    }
    Ok(Some((start, end)))
}

fn etag_of(f: &FileItem) -> String {
    format!("\"{:x}-{:x}\"", f.size, f.mtime)
}

async fn download(
    state: &Arc<State>,
    req: &Request<Incoming>,
    peer: SocketAddr,
    idx: usize,
    head: bool,
) -> Response<Body> {
    let f = &state.files[idx];
    // The file must be what it was when the share started.
    let Ok(md) = tokio::fs::metadata(&f.path).await else {
        return not_found();
    };
    if md.len() != f.size {
        return html(
            409,
            page(
                "File changed",
                "<p>This file changed after the share started.</p>",
            ),
        );
    }
    let etag = etag_of(f);
    let range_header = req.headers().get("range").and_then(|v| v.to_str().ok());
    // If-Range: only honour the range when the validator still matches.
    let if_range_ok = req
        .headers()
        .get("if-range")
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| v == etag);
    let range = match (range_header, if_range_ok) {
        (Some(h), true) => match parse_range(h, f.size) {
            Ok(r) => r,
            Err(()) => {
                let mut r = Response::new(http::empty());
                *r.status_mut() = hyper::StatusCode::RANGE_NOT_SATISFIABLE;
                r.headers_mut().insert(
                    "content-range",
                    format!("bytes */{}", f.size).parse().unwrap(),
                );
                return secure(r);
            }
        },
        _ => None,
    };
    let (start, end) = range.unwrap_or((0, f.size.saturating_sub(1)));
    let len = if f.size == 0 { 0 } else { end - start + 1 };
    let name = f.name.rsplit('/').next().unwrap_or(&f.name).to_string();

    let (tx, body) = chan_body(Some(len), 4);
    let mut resp = Response::new(if head { http::empty() } else { body });
    let h = resp.headers_mut();
    h.insert("content-type", "application/octet-stream".parse().unwrap());
    h.insert("content-disposition", disposition(&name).parse().unwrap());
    h.insert("accept-ranges", "bytes".parse().unwrap());
    h.insert("etag", etag.parse().unwrap());
    h.insert("content-length", len.to_string().parse().unwrap());
    if range.is_some() {
        *resp.status_mut() = hyper::StatusCode::PARTIAL_CONTENT;
        resp.headers_mut().insert(
            "content-range",
            format!("bytes {start}-{end}/{}", f.size).parse().unwrap(),
        );
    }
    if head {
        return secure(resp);
    }

    state.emit(QrEvent::DownloadStarted {
        ip: peer.ip(),
        file: f.name.clone(),
    });
    let (state, path, file_name) = (state.clone(), f.path.clone(), f.name.clone());
    let reaches_end = len == 0 || end + 1 == f.size;
    tokio::spawn(async move {
        let sent = async {
            let mut file = tokio::fs::File::open(&path).await?;
            file.seek(SeekFrom::Start(start)).await?;
            let mut left = len;
            let mut buf = vec![0u8; CHUNK];
            while left > 0 {
                let want = (left as usize).min(CHUNK);
                let n = file.read(&mut buf[..want]).await?;
                if n == 0 {
                    return Err(std::io::Error::other("the file ended early"));
                }
                left -= n as u64;
                if tx
                    .send(Ok(Bytes::copy_from_slice(&buf[..n])))
                    .await
                    .is_err()
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "client left",
                    ));
                }
            }
            Ok(())
        }
        .await;
        if sent.is_ok() {
            state.emit(QrEvent::Downloaded {
                ip: peer.ip(),
                file: file_name,
                bytes: len,
            });
            if reaches_end {
                state.delivered.lock().unwrap().insert(idx);
                state.check_complete();
            }
        } else if let Err(e) = sent
            && e.kind() != std::io::ErrorKind::BrokenPipe
        {
            let _ = tx.send(Err(e)).await;
        }
    });
    secure(resp)
}

fn zip(state: &Arc<State>, peer: SocketAddr, head: bool) -> Response<Body> {
    let entries: Vec<ZipEntry> = state
        .files
        .iter()
        .map(|f| ZipEntry {
            name: f.name.clone(),
            path: f.path.clone(),
            size: f.size,
            mtime: f.mtime,
        })
        .collect();
    let plan = ZipPlan::new(entries, state.force_zip64);
    let total = plan.total_len();
    let (tx, body) = chan_body(Some(total), 4);
    let mut resp = Response::new(if head { http::empty() } else { body });
    let h = resp.headers_mut();
    h.insert("content-type", "application/zip".parse().unwrap());
    h.insert(
        "content-disposition",
        disposition(&state.zip_name).parse().unwrap(),
    );
    h.insert("content-length", total.to_string().parse().unwrap());
    if head {
        return secure(resp);
    }
    state.emit(QrEvent::DownloadStarted {
        ip: peer.ip(),
        file: "zip".into(),
    });
    let state = state.clone();
    tokio::spawn(async move {
        if plan.write(tx).await.is_ok() {
            state.emit(QrEvent::Downloaded {
                ip: peer.ip(),
                file: "zip".into(),
                bytes: total,
            });
            state.zip_done.store(true, Ordering::Relaxed);
            state.check_complete();
        }
    });
    secure(resp)
}

async fn upload(state: &Arc<State>, req: Request<Incoming>, peer: SocketAddr) -> Response<Body> {
    let Some(dir) = state.upload_dir.clone() else {
        return not_found();
    };
    let Some(boundary) = req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .and_then(boundary_of)
    else {
        return html(400, page("Bad upload", "<p>Use the form on the page.</p>"));
    };
    if let Some(len) = req
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
    {
        if len > state.max_upload {
            return html(
                413,
                page("Too large", "<p>That upload is larger than allowed.</p>"),
            );
        }
        if let Some(free) = fsx::free_space(&dir)
            && free < len
        {
            return html(
                507,
                page(
                    "Disk full",
                    "<p>This computer does not have room for that.</p>",
                ),
            );
        }
    }
    match save_upload(state, req.into_body(), &boundary, &dir, peer).await {
        Ok(n) if n > 0 => {
            state.uploads.fetch_add(n as u32, Ordering::Relaxed);
            state.close(QrOutcome::Completed);
            html(
                200,
                page(
                    "Received",
                    &format!(
                        "<p>{n} file{} received. This link is now closed.</p>",
                        if n == 1 { "" } else { "s" }
                    ),
                ),
            )
        }
        Ok(_) => html(400, page("No file", "<p>Choose a file first.</p>")),
        Err(e) => html(
            400,
            page("Upload failed", &format!("<p>{}</p>", esc(&e.to_string()))),
        ),
    }
}

async fn save_upload(
    state: &Arc<State>,
    mut body: Incoming,
    boundary: &str,
    dir: &std::path::Path,
    peer: SocketAddr,
) -> Result<usize> {
    struct Current {
        out: BufWriter<tokio::fs::File>,
        part: fsx::PartFile,
        bytes: u64,
    }
    let mut parser = Multipart::new(boundary);
    let mut current: Option<Current> = None;
    let mut saved = 0usize;
    let mut total = 0u64;
    let mut parts = Vec::new();
    let result: Result<()> = async {
        while let Some(frame) = tokio::time::timeout(Duration::from_secs(60), body.frame()).await? {
            let frame = frame?;
            let Ok(data) = frame.into_data() else {
                continue;
            };
            parts.clear();
            parser
                .feed(&data, &mut parts)
                .map_err(|e| Error::Input(e.to_string()))?;
            for p in parts.drain(..) {
                match p {
                    Part::FileStart(raw) => {
                        let leaf = raw.rsplit(['/', '\\']).next().unwrap_or("");
                        let name = fsx::sanitize_component(leaf);
                        let (file, part) = fsx::create_part(dir, &name, &random_hex(4))?;
                        current = Some(Current {
                            out: BufWriter::with_capacity(CHUNK, tokio::fs::File::from_std(file)),
                            part,
                            bytes: 0,
                        });
                    }
                    Part::Data(d) => {
                        total += d.len() as u64;
                        if total > state.max_upload {
                            return Err(Error::Input("the upload is larger than allowed".into()));
                        }
                        if let Some(c) = &mut current {
                            c.bytes += d.len() as u64;
                            c.out.write_all(&d).await?;
                        }
                    }
                    Part::FileEnd => {
                        if let Some(mut c) = current.take() {
                            c.out.flush().await?;
                            drop(c.out);
                            let path = c.part.finalize()?;
                            saved += 1;
                            state.emit(QrEvent::Uploaded {
                                ip: peer.ip(),
                                path,
                                bytes: c.bytes,
                            });
                        }
                    }
                }
            }
        }
        if !parser.is_done() {
            return Err(Error::Input("the upload ended early".into()));
        }
        Ok(())
    }
    .await;
    if let Some(c) = current {
        c.part.discard();
    }
    result.map(|()| saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_parsing() {
        assert_eq!(parse_range("bytes=0-99", 1000), Ok(Some((0, 99))));
        assert_eq!(parse_range("bytes=500-", 1000), Ok(Some((500, 999))));
        assert_eq!(parse_range("bytes=-100", 1000), Ok(Some((900, 999))));
        assert_eq!(parse_range("bytes=900-5000", 1000), Ok(Some((900, 999))));
        assert_eq!(parse_range("bytes=1000-", 1000), Err(()));
        assert_eq!(parse_range("bytes=5-2", 1000), Err(()));
        assert_eq!(parse_range("bytes=0-1,5-6", 1000), Ok(None));
        assert_eq!(parse_range("items=0-1", 1000), Ok(None));
    }

    #[test]
    fn escapes_html() {
        assert_eq!(
            esc("<a href=\"x\">&'"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
    }

    #[test]
    fn disposition_keeps_unicode_names() {
        let d = disposition("naïve \"x\".txt");
        assert!(d.contains("filename=\"na_ve _x_.txt\""));
        assert!(d.contains("filename*=UTF-8''na%C3%AFve%20%22x%22.txt"));
    }

    #[test]
    fn page_is_small_and_external_free() {
        let mut body = String::new();
        for i in 0..50 {
            body.push_str(&format!(
                "<li><a href=\"/t/d/{i}\">file {i}.bin</a><span>1.0 MB</span></li>"
            ));
        }
        let p = page("Panoply share", &body);
        assert!(p.len() < 20 * 1024, "{}", p.len());
        assert!(!p.contains("http://") && !p.contains("https://"));
    }
}
