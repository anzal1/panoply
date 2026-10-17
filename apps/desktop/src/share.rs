//! Sharing files with other devices: the logic behind the Share screen.
//!
//! Three layers, none of them with a UI type:
//!
//! - [`ShareModel`] is the view model. It is changed only by [`EngineEvent`]s and by a few user
//!   actions, and it knows what the screen shows (empty, devices found, sending, an incoming
//!   prompt, done, error) and the plain words for each. It is tested without a network.
//! - [`Engine`] runs a `panoply_share::Node` (LocalSend v2, discovery, receive prompt) and the QR
//!   page on a private tokio runtime in its own thread, and turns what happens into
//!   [`EngineEvent`]s.
//! - The screen starts an engine only while it is open, or while receiving is on, and drops it
//!   after that. A dropped engine has no thread, no socket and no timer, so a closed Share screen
//!   costs no CPU.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use panoply_share::qr::{QrEvent, QrOptions, QrOutcome, QrShare};
use panoply_share::{
    Decision, Device, DeviceType, Direction, Error, Event, IncomingRequest, Node, NodeConfig,
    ReceiveConfig, SendCancel, SendOptions, SendSummary, TransferKind, handler_fn,
};
use tokio::sync::{mpsc, oneshot};

use crate::units::format_bytes;

// ---------------------------------------------------------------------------------------------
// What the screen shows

/// The kind of a nearby device, for its icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    /// A computer.
    Computer,
    /// A phone or tablet.
    Phone,
    /// A browser tab.
    Browser,
    /// Anything else (a server, a device with no screen).
    Other,
}

/// One device in the grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceRow {
    /// Certificate fingerprint: what a send is addressed to.
    pub id: String,
    /// The name the device gave itself.
    pub alias: String,
    /// "macOS", "Android" and so on, when it said.
    pub model: String,
    /// Icon choice.
    pub kind: DeviceKind,
    /// Address, for the small print.
    pub addr: String,
}

impl DeviceRow {
    fn from_device(d: &Device) -> DeviceRow {
        DeviceRow {
            id: d.fingerprint().to_string(),
            alias: d.alias().to_string(),
            model: d.info.device_model.clone().unwrap_or_default(),
            kind: match d.info.device_type.unwrap_or_default() {
                DeviceType::Desktop => DeviceKind::Computer,
                DeviceType::Mobile => DeviceKind::Phone,
                DeviceType::Web => DeviceKind::Browser,
                _ => DeviceKind::Other,
            },
            addr: d.addr.to_string(),
        }
    }
}

/// A file or folder waiting to be sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Staged {
    /// Full path.
    pub path: PathBuf,
    /// Name to show.
    pub name: String,
    /// Size in bytes, folders summed. `None` when it could not be read.
    pub size: Option<u64>,
    /// A folder.
    pub folder: bool,
}

impl Staged {
    /// Read a path for the list. Folders are summed one level deep, which is enough for a label.
    pub fn read(path: &Path) -> Staged {
        let meta = std::fs::metadata(path).ok();
        let folder = meta.as_ref().is_some_and(|m| m.is_dir());
        let size = if folder {
            Some(folder_size(path, 4))
        } else {
            meta.map(|m| m.len())
        };
        Staged {
            path: path.to_path_buf(),
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            size,
            folder,
        }
    }
}

fn folder_size(dir: &Path, depth: u8) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    rd.flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() && depth > 0 => folder_size(&e.path(), depth - 1),
            Ok(m) if m.is_file() => m.len(),
            _ => 0,
        })
        .sum()
}

/// What went wrong, in terms a person can act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// This computer could not start listening (port taken, network off, blocked).
    CannotListen(String),
    /// A device was seen but would not answer: usually its firewall.
    Unreachable,
    /// The other device said no.
    Declined,
    /// The other device is receiving from someone else right now.
    Busy,
    /// The user stopped it.
    Cancelled,
    /// Anything else, in the library's own words.
    Other(String),
}

impl Problem {
    /// Sort an error from the share crate.
    pub fn from_error(e: &Error) -> Problem {
        match e {
            Error::Rejected => Problem::Declined,
            Error::Busy => Problem::Busy,
            Error::Cancelled => Problem::Cancelled,
            Error::Bind(m) => Problem::CannotListen(m.clone()),
            Error::Timeout | Error::Io(_) => Problem::Unreachable,
            other => Problem::Other(other.to_string()),
        }
    }

    /// The headline and the line under it.
    pub fn words(&self, peer: &str) -> (String, String) {
        match self {
            Problem::CannotListen(m) => (
                "Panoply cannot use the network".to_string(),
                format!(
                    "{}. Check that you are connected to Wi-Fi or Ethernet and that no other file sharing app is running.",
                    sentence_case(m)
                ),
            ),
            Problem::Unreachable => (
                format!("{peer} did not answer"),
                "Its firewall may be blocking Panoply, or it left the network. Allow Panoply on that device, or use the QR code instead."
                    .to_string(),
            ),
            Problem::Declined => (
                format!("{peer} declined"),
                "Nothing was sent. Ask them to accept on their screen and try again.".to_string(),
            ),
            Problem::Busy => (
                format!("{peer} is busy"),
                "It is receiving from someone else. Try again in a moment.".to_string(),
            ),
            Problem::Cancelled => (
                "Cancelled".to_string(),
                "Nothing more was sent. Files that already arrived stay where they are.".to_string(),
            ),
            Problem::Other(m) => ("The transfer failed".to_string(), format!("{m}.")),
        }
    }
}

/// The text with its first letter in capitals, for a library message used as a sentence.
fn sentence_case(m: &str) -> String {
    let mut c = m.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// How a send is going.
#[derive(Clone, Debug, PartialEq)]
pub enum SendPhase {
    /// Asked, and the other device has not answered yet.
    Waiting,
    /// Bytes are moving.
    Sending {
        /// The file in flight.
        file: String,
        /// Bytes done across the whole transfer.
        done: u64,
        /// Bytes in the whole transfer.
        total: u64,
    },
    /// All of it arrived.
    Done {
        /// Files moved.
        files: usize,
        /// Bytes moved.
        bytes: u64,
    },
    /// Stopped by either side.
    Cancelled,
    /// Stopped by a problem.
    Failed(Problem),
}

/// The send that is running or just ended.
#[derive(Clone, Debug, PartialEq)]
pub struct SendView {
    /// The transfer id, once it is known.
    pub id: Option<String>,
    /// Who it goes to.
    pub peer: String,
    /// What it is doing.
    pub phase: SendPhase,
    /// Files in the request.
    pub files: usize,
    /// Four words to compare with the other screen, when it is a Panoply app that shows them too.
    pub check_code: Option<String>,
}

impl SendView {
    /// Progress from 0 to 1, or `None` while there is nothing to measure.
    pub fn fraction(&self) -> Option<f32> {
        match &self.phase {
            SendPhase::Sending { done, total, .. } if *total > 0 => {
                Some((*done as f64 / *total as f64).clamp(0.0, 1.0) as f32)
            }
            SendPhase::Done { .. } => Some(1.0),
            _ => None,
        }
    }

    /// True while the send can still be cancelled.
    pub fn active(&self) -> bool {
        matches!(self.phase, SendPhase::Waiting | SendPhase::Sending { .. })
    }
}

/// A file offered to this computer, as the prompt lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingFileRow {
    /// Name as the sender gave it.
    pub name: String,
    /// Size in bytes.
    pub size: u64,
    /// A type that runs code when opened.
    pub risky: bool,
}

/// Someone wants to send files here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingView {
    /// Answers the prompt.
    pub id: String,
    /// The sender's name.
    pub sender: String,
    /// Four words both screens show, to check they talk to each other.
    pub check_code: String,
    /// The sender proved the identity it announced.
    pub verified: bool,
    /// The files offered.
    pub files: Vec<IncomingFileRow>,
    /// All sizes added.
    pub total: u64,
    /// The text, when it is a message.
    pub text: Option<String>,
}

/// A transfer coming in, after the prompt was accepted.
#[derive(Clone, Debug, PartialEq)]
pub struct ReceiveView {
    /// Transfer id.
    pub id: String,
    /// The sender's name.
    pub peer: String,
    /// What it is doing.
    pub phase: ReceivePhase,
    /// Where the first file landed, to reveal it.
    pub first: Option<PathBuf>,
}

/// How a receive is going.
#[derive(Clone, Debug, PartialEq)]
pub enum ReceivePhase {
    /// Bytes are moving.
    Receiving {
        /// Bytes done.
        done: u64,
        /// Bytes expected.
        total: u64,
    },
    /// All of it arrived and sits in the folder.
    Done {
        /// Files received.
        files: usize,
        /// Bytes received.
        bytes: u64,
    },
    /// Stopped by either side.
    Cancelled,
    /// Stopped by a problem.
    Failed(String),
}

/// The QR page for devices without the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QrView {
    /// The address the code points to.
    pub url: String,
    /// Side of the code in modules.
    pub side: usize,
    /// Modules, row by row, true for dark.
    pub modules: Vec<bool>,
    /// The address is on this computer's network (false means loopback only).
    pub on_lan: bool,
    /// What was shared.
    pub files: usize,
    /// Devices that opened the page.
    pub visitors: usize,
    /// Files downloaded.
    pub downloads: usize,
    /// Set when the page closed, and why.
    pub closed: Option<QrClosed>,
}

/// Why the QR page closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QrClosed {
    /// The download finished.
    Completed,
    /// Nobody finished in ten minutes.
    Expired,
    /// The user closed it.
    Revoked,
}

/// What the network side is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Net {
    /// Nothing runs: the screen is closed and receiving is off.
    Off,
    /// Starting.
    Starting,
    /// Listening and looking for devices.
    On {
        /// This computer's name as others see it.
        alias: String,
    },
    /// Could not start.
    Failed(Problem),
}

/// The state of the whole screen.
#[derive(Clone, Debug)]
pub struct ShareModel {
    /// The network side.
    pub net: Net,
    /// Devices found, by name.
    pub devices: Vec<DeviceRow>,
    /// The first sweep of the network has ended.
    pub scanned: bool,
    /// Receive mode: this computer shows up on other devices and asks before saving.
    pub receiving: bool,
    /// Where received files go.
    pub save_dir: PathBuf,
    /// Files waiting to be sent.
    pub staged: Vec<Staged>,
    /// The send that is running or just ended.
    pub send: Option<SendView>,
    /// An unanswered prompt.
    pub incoming: Option<IncomingView>,
    /// The latest receive.
    pub receive: Option<ReceiveView>,
    /// The QR page, when one is up.
    pub qr: Option<QrView>,
    /// A line for a problem that is not a transfer's.
    pub note: Option<String>,
}

/// Everything the engine tells the screen.
#[derive(Clone, Debug)]
pub enum EngineEvent {
    /// The node is up.
    Listening {
        /// This computer's name on the network.
        alias: String,
    },
    /// The node could not start.
    NetFailed(Problem),
    /// The list of devices, whole, after every change.
    Devices(Vec<DeviceRow>),
    /// The sweep of the network ended.
    ScanFinished,
    /// A send has an id.
    SendStarted {
        /// Transfer id.
        id: String,
        /// The four words to compare with the other screen. Only between two Panoply apps.
        check_code: Option<String>,
    },
    /// Something in a transfer, either direction.
    Transfer {
        /// Transfer id.
        id: String,
        /// Which way.
        direction: Direction,
        /// The other device.
        peer: String,
        /// What happened.
        kind: TransferKind,
    },
    /// A send ended; `Err` when it did not finish.
    SendEnded {
        /// Transfer id.
        id: String,
        /// How.
        result: Result<(usize, u64), Problem>,
    },
    /// Someone asks to send here.
    Incoming(IncomingView),
    /// The prompt is gone (answered, or the sender gave up).
    IncomingGone(String),
    /// The QR page is up.
    QrReady(QrView),
    /// A visitor or a download on the QR page.
    QrActivity {
        /// A browser opened the page.
        visit: bool,
        /// A file finished downloading.
        download: bool,
    },
    /// The QR page closed.
    QrClosed(QrClosed),
    /// The QR page could not start.
    QrFailed(Problem),
}

impl ShareModel {
    /// A fresh model: nothing running, files go to `save_dir`.
    pub fn new(save_dir: PathBuf) -> ShareModel {
        ShareModel {
            net: Net::Off,
            devices: Vec::new(),
            scanned: false,
            receiving: false,
            save_dir,
            staged: Vec::new(),
            send: None,
            incoming: None,
            receive: None,
            qr: None,
            note: None,
        }
    }

    /// The engine was asked to start.
    pub fn starting(&mut self) {
        self.net = Net::Starting;
        self.devices.clear();
        self.scanned = false;
    }

    /// The engine was dropped: nothing is listening, so nothing is found and nothing is asked.
    pub fn stopped(&mut self) {
        self.net = Net::Off;
        self.devices.clear();
        self.scanned = false;
        self.incoming = None;
        self.qr = None;
        if self.send.as_ref().is_some_and(SendView::active) {
            self.send = None;
        }
    }

    /// Add paths to the files waiting, skipping ones already there.
    pub fn stage(&mut self, paths: &[PathBuf]) {
        for p in paths {
            if !self.staged.iter().any(|s| &s.path == p) {
                self.staged.push(Staged::read(p));
            }
        }
        // A new set of files starts a new conversation: the last send's result is old news.
        if self.send.as_ref().is_some_and(|s| !s.active()) {
            self.send = None;
        }
    }

    /// Take one path off the waiting list.
    pub fn unstage(&mut self, path: &Path) {
        self.staged.retain(|s| s.path != path);
    }

    /// Total size of what is waiting.
    pub fn staged_bytes(&self) -> u64 {
        self.staged.iter().filter_map(|s| s.size).sum()
    }

    /// A send starts toward `peer`.
    pub fn begin_send(&mut self, peer: &str) {
        self.send = Some(SendView {
            id: None,
            peer: peer.to_string(),
            phase: SendPhase::Waiting,
            files: self.staged.len(),
            check_code: None,
        });
    }

    /// Clear the result of a finished send.
    pub fn dismiss_send(&mut self) {
        if self.send.as_ref().is_some_and(|s| !s.active()) {
            self.send = None;
        }
    }

    /// Clear the result of a finished receive.
    pub fn dismiss_receive(&mut self) {
        self.receive = None;
    }

    /// The prompt was answered by the user.
    pub fn answered(&mut self, accepted: bool) {
        if let Some(i) = self.incoming.take()
            && accepted
        {
            self.receive = Some(ReceiveView {
                id: i.id,
                peer: i.sender,
                phase: ReceivePhase::Receiving {
                    done: 0,
                    total: i.total,
                },
                first: None,
            });
        }
    }

    /// Change the model for one thing the engine said.
    pub fn apply(&mut self, ev: EngineEvent) {
        match ev {
            EngineEvent::Listening { alias } => self.net = Net::On { alias },
            EngineEvent::NetFailed(p) => self.net = Net::Failed(p),
            EngineEvent::Devices(d) => self.devices = d,
            EngineEvent::ScanFinished => self.scanned = true,
            EngineEvent::SendStarted { id, check_code } => {
                if let Some(s) = &mut self.send {
                    s.id = Some(id);
                    s.check_code = check_code;
                }
            }
            EngineEvent::Transfer {
                id,
                direction: Direction::Send,
                kind,
                ..
            } => {
                let Some(s) = &mut self.send else { return };
                if s.id.as_deref().is_some_and(|x| x != id) {
                    return;
                }
                match kind {
                    TransferKind::Started { files, bytes } => {
                        s.files = files;
                        s.phase = SendPhase::Sending {
                            file: String::new(),
                            done: 0,
                            total: bytes,
                        };
                    }
                    TransferKind::Progress {
                        file,
                        total_done,
                        total,
                        ..
                    } => {
                        s.phase = SendPhase::Sending {
                            file,
                            done: total_done,
                            total,
                        };
                    }
                    TransferKind::Cancelled => s.phase = SendPhase::Cancelled,
                    _ => {}
                }
            }
            EngineEvent::Transfer {
                id,
                direction: Direction::Receive,
                peer,
                kind,
            } => self.apply_receive(id, peer, kind),
            EngineEvent::SendEnded { id, result } => {
                let Some(s) = &mut self.send else { return };
                if s.id.as_deref().is_some_and(|x| x != id) {
                    return;
                }
                s.phase = match result {
                    Ok((files, bytes)) => SendPhase::Done { files, bytes },
                    Err(Problem::Cancelled) => SendPhase::Cancelled,
                    Err(p) => SendPhase::Failed(p),
                };
            }
            EngineEvent::Incoming(i) => self.incoming = Some(i),
            EngineEvent::IncomingGone(id) => {
                if self.incoming.as_ref().is_some_and(|i| i.id == id) {
                    self.incoming = None;
                }
            }
            EngineEvent::QrReady(q) => self.qr = Some(q),
            EngineEvent::QrActivity { visit, download } => {
                if let Some(q) = &mut self.qr {
                    q.visitors += usize::from(visit);
                    q.downloads += usize::from(download);
                }
            }
            EngineEvent::QrClosed(c) => {
                if let Some(q) = &mut self.qr {
                    q.closed = Some(c);
                }
            }
            EngineEvent::QrFailed(p) => {
                self.qr = None;
                self.note = Some(p.words("").1);
            }
        }
    }

    fn apply_receive(&mut self, id: String, peer: String, kind: TransferKind) {
        let fresh = |id: String, peer: String| ReceiveView {
            id,
            peer,
            phase: ReceivePhase::Receiving { done: 0, total: 0 },
            first: None,
        };
        let r = self
            .receive
            .get_or_insert_with(|| fresh(id.clone(), peer.clone()));
        if r.id != id {
            *r = fresh(id, peer);
        }
        match kind {
            TransferKind::Started { bytes, .. } => {
                r.phase = ReceivePhase::Receiving {
                    done: 0,
                    total: bytes,
                };
            }
            TransferKind::Progress {
                total_done, total, ..
            } => {
                r.phase = ReceivePhase::Receiving {
                    done: total_done,
                    total,
                };
            }
            TransferKind::FileDone { path, .. } => {
                if r.first.is_none() {
                    r.first = path;
                }
            }
            TransferKind::Finished { files, bytes, .. } => {
                r.phase = ReceivePhase::Done { files, bytes };
            }
            TransferKind::Failed { error } => r.phase = ReceivePhase::Failed(error),
            TransferKind::Cancelled => r.phase = ReceivePhase::Cancelled,
            TransferKind::Text { .. } => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Words

/// "3 files, 12.4 MB" for the list of waiting files.
pub fn staged_label(staged: &[Staged]) -> String {
    let n = staged.len();
    let bytes: u64 = staged.iter().filter_map(|s| s.size).sum();
    let what = if n == 1 {
        "1 item".to_string()
    } else {
        format!("{n} items")
    };
    format!("{what}, {}", format_bytes(bytes))
}

/// The line under a progress bar: "4.1 MB of 12.4 MB".
pub fn progress_label(done: u64, total: u64) -> String {
    format!("{} of {}", format_bytes(done), format_bytes(total))
}

/// "1 file" or "3 files".
pub fn files_word(n: usize) -> String {
    if n == 1 {
        "1 file".to_string()
    } else {
        format!("{n} files")
    }
}

/// What the empty device grid says. `None` when there are devices to show.
///
/// Before the first sweep ends it only says it is looking; after, it says what to check, with the
/// firewall named because that is the usual reason on a network that does have other devices.
pub fn empty_copy(m: &ShareModel) -> Option<(&'static str, &'static str)> {
    if !m.devices.is_empty() {
        return None;
    }
    Some(if m.scanned {
        (
            "No devices found",
            "Open Panoply or LocalSend on another device on this Wi-Fi. If it is open and still missing, a firewall on either side may be blocking it. The QR code works with any phone browser.",
        )
    } else {
        (
            "Looking for devices",
            "Other devices running Panoply or LocalSend on this network show up here on their own.",
        )
    })
}

/// A short label for a device's second line: "macOS", or the address when it said nothing.
pub fn device_sub(d: &DeviceRow) -> String {
    if d.model.is_empty() {
        d.addr.clone()
    } else {
        d.model.clone()
    }
}

// ---------------------------------------------------------------------------------------------
// The folder files land in

/// The user's Downloads folder, or Panoply's data folder when the system has none.
pub fn downloads_dir() -> PathBuf {
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    match home {
        Some(h) if h.join("Downloads").is_dir() => h.join("Downloads"),
        Some(h) => h.join("Downloads"),
        None => crate::paths::data_dir().join("received"),
    }
}

// ---------------------------------------------------------------------------------------------
// The engine

/// How the engine starts.
#[derive(Clone)]
pub struct EngineConfig {
    /// This computer's name on the network.
    pub alias: String,
    /// Where received files go.
    pub save_dir: PathBuf,
    /// Where the identity and the trust list live. `None` keeps them in memory (tests).
    pub data_dir: Option<PathBuf>,
    /// Answer incoming requests (receive mode). Can be changed later.
    pub receiving: bool,
    /// Multicast and scan discovery. Off for loopback tests.
    pub discovery: bool,
    /// Address to listen on.
    pub bind: std::net::IpAddr,
    /// Port. `0` picks one.
    pub port: u16,
    /// Addresses the sweep probes besides the local networks (a device typed in, loopback tests).
    pub probe: Vec<std::net::SocketAddr>,
}

impl EngineConfig {
    /// The real thing: this computer's name, Downloads, the user's identity, the LocalSend port.
    pub fn app(receiving: bool) -> EngineConfig {
        EngineConfig {
            alias: computer_name(),
            save_dir: downloads_dir(),
            data_dir: Some(panoply_share::node::default_data_dir()),
            receiving,
            discovery: true,
            bind: std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
            port: 53317,
            probe: Vec::new(),
        }
    }
}

/// This computer's name, as other devices will list it.
pub fn computer_name() -> String {
    if let Ok(a) = std::env::var("PANOPLY_ALIAS")
        && !a.trim().is_empty()
    {
        return a;
    }
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().trim_end_matches(".local").to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Panoply".into())
}

enum Cmd {
    Rescan,
    Send { device: String, paths: Vec<PathBuf> },
    CancelSend,
    Decide { id: String, accept: bool },
    SetReceiving(bool),
    StartQr(Vec<PathBuf>),
    StopQr,
    Stop,
}

/// A running share node. Dropping it stops the thread, the sockets and the timers.
pub struct Engine {
    tx: mpsc::UnboundedSender<Cmd>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Engine {
    /// Start a node on its own thread. Everything it does comes back on the returned channel.
    pub fn start(cfg: EngineConfig) -> (Engine, UnboundedReceiver<EngineEvent>) {
        let (ev_tx, ev_rx) = unbounded();
        let (tx, rx) = mpsc::unbounded_channel();
        let thread = std::thread::Builder::new()
            .name("panoply-share".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .thread_name("panoply-share-io")
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = ev_tx.unbounded_send(EngineEvent::NetFailed(
                            Problem::CannotListen(e.to_string()),
                        ));
                        return;
                    }
                };
                rt.block_on(run(cfg, rx, ev_tx));
                // Do not wait for stragglers: the sockets are closed, the process goes on.
                rt.shutdown_background();
            })
            .ok();
        (Engine { tx, thread }, ev_rx)
    }

    /// Sweep the network again.
    pub fn rescan(&self) {
        let _ = self.tx.send(Cmd::Rescan);
    }

    /// Send `paths` to the device with this fingerprint.
    pub fn send(&self, device: &str, paths: Vec<PathBuf>) {
        let _ = self.tx.send(Cmd::Send {
            device: device.to_string(),
            paths,
        });
    }

    /// Stop the running send.
    pub fn cancel_send(&self) {
        let _ = self.tx.send(Cmd::CancelSend);
    }

    /// Answer a prompt.
    pub fn decide(&self, id: &str, accept: bool) {
        let _ = self.tx.send(Cmd::Decide {
            id: id.to_string(),
            accept,
        });
    }

    /// Turn receive mode on or off.
    pub fn set_receiving(&self, on: bool) {
        let _ = self.tx.send(Cmd::SetReceiving(on));
    }

    /// Put `paths` behind a QR code and a web page.
    pub fn start_qr(&self, paths: Vec<PathBuf>) {
        let _ = self.tx.send(Cmd::StartQr(paths));
    }

    /// Close the QR page.
    pub fn stop_qr(&self) {
        let _ = self.tx.send(Cmd::StopQr);
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
        // The thread ends within moments of Stop; waiting keeps the port free for a restart.
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>;

/// Sends `IncomingGone` when the prompt's future ends, however it ends.
struct Gone(String, UnboundedSender<EngineEvent>);

impl Drop for Gone {
    fn drop(&mut self) {
        let _ = self
            .1
            .unbounded_send(EngineEvent::IncomingGone(self.0.clone()));
    }
}

fn incoming_view(id: String, r: &IncomingRequest) -> IncomingView {
    IncomingView {
        id,
        sender: r.sender.alias.clone(),
        // Only Panoply shows these words, so a LocalSend sender gets no instruction to compare them.
        check_code: if panoply_share::is_panoply(&r.sender) {
            r.check_code.clone()
        } else {
            String::new()
        },
        verified: r.verified,
        files: r
            .files
            .iter()
            .map(|f| IncomingFileRow {
                name: f.name.clone(),
                size: f.size,
                risky: f.risky,
            })
            .collect(),
        total: r.total_size,
        text: r.text.clone(),
    }
}

async fn run(
    cfg: EngineConfig,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
    ev: UnboundedSender<EngineEvent>,
) {
    let receiving = Arc::new(AtomicBool::new(cfg.receiving));
    let pending: Pending = Arc::default();
    let counter = Arc::new(AtomicU64::new(1));

    let handler = {
        let receiving = receiving.clone();
        let pending = pending.clone();
        let ev = ev.clone();
        handler_fn(move |req| {
            let receiving = receiving.clone();
            let pending = pending.clone();
            let ev = ev.clone();
            let n = counter.fetch_add(1, Ordering::Relaxed);
            Box::pin(async move {
                if !receiving.load(Ordering::Relaxed) {
                    return Decision::Decline;
                }
                let id = format!("in-{n}");
                let (tx, answer) = oneshot::channel();
                pending.lock().unwrap().insert(id.clone(), tx);
                let _gone = Gone(id.clone(), ev.clone());
                let _ = ev.unbounded_send(EngineEvent::Incoming(incoming_view(id.clone(), &req)));
                let accepted = answer.await.unwrap_or(false);
                pending.lock().unwrap().remove(&id);
                if accepted {
                    Decision::accept_all()
                } else {
                    Decision::Decline
                }
            })
        })
    };

    let mut nc = NodeConfig::new(cfg.alias.clone());
    nc.bind = cfg.bind;
    nc.port = cfg.port;
    nc.multicast_port = cfg.port;
    nc.discovery = cfg.discovery;
    nc.data_dir = cfg.data_dir.clone();
    // The default port is the one other devices look for; when another app holds it, a free one
    // still lets this computer send, and the multicast announcement carries the port.
    nc.ephemeral_if_busy = true;
    nc.receive = Some(ReceiveConfig::new(cfg.save_dir.clone(), handler));
    let node = match Node::start(nc).await {
        Ok(n) => n,
        Err(e) => {
            let _ = ev.unbounded_send(EngineEvent::NetFailed(Problem::from_error(&e)));
            return;
        }
    };
    let _ = ev.unbounded_send(EngineEvent::Listening { alias: cfg.alias });

    // Device and transfer events, from the node to the screen.
    let mut events = node.subscribe();
    let fwd_node = node.clone();
    let fwd_ev = ev.clone();
    let forward = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(Event::DeviceFound(_))
                | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let _ = fwd_ev.unbounded_send(EngineEvent::Devices(rows(&fwd_node)));
                }
                Ok(Event::Transfer(t)) => {
                    let _ = fwd_ev.unbounded_send(EngineEvent::Transfer {
                        id: t.id,
                        direction: t.direction,
                        peer: t.peer,
                        kind: t.kind,
                    });
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let probe = cfg.probe.clone();
    let sweep_lan = cfg.discovery;
    let scan = move |node: Node, ev: UnboundedSender<EngineEvent>| {
        let probe = probe.clone();
        tokio::spawn(async move {
            // The multicast announcement may already have found devices; the sweep covers the rest.
            let sweep = async {
                if sweep_lan {
                    node.scan().await;
                }
                if !probe.is_empty() {
                    node.scan_addrs(&probe).await;
                }
            };
            let _ = tokio::time::timeout(Duration::from_secs(8), sweep).await;
            let _ = ev.unbounded_send(EngineEvent::Devices(rows(&node)));
            let _ = ev.unbounded_send(EngineEvent::ScanFinished);
        })
    };
    let mut scanning = Some(scan(node.clone(), ev.clone()));

    let mut send_cancel: Option<SendCancel> = None;
    let mut qr: Option<Arc<QrShare>> = None;
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Cmd::Rescan => {
                if let Some(s) = scanning.take() {
                    s.abort();
                }
                node.announce();
                scanning = Some(scan(node.clone(), ev.clone()));
            }
            Cmd::Send { device, paths } => {
                let Some(dev) = node
                    .devices()
                    .into_iter()
                    .find(|d| d.fingerprint() == device)
                else {
                    let _ = ev.unbounded_send(EngineEvent::SendEnded {
                        id: String::new(),
                        result: Err(Problem::Unreachable),
                    });
                    continue;
                };
                let check_code = panoply_share::is_panoply(&dev.info)
                    .then(|| panoply_share::check_code(node.fingerprint(), dev.fingerprint()));
                let job = node.send(dev, paths, SendOptions::default());
                let tid = job.id().to_string();
                send_cancel = Some(job.cancel_handle());
                let _ = ev.unbounded_send(EngineEvent::SendStarted {
                    id: tid.clone(),
                    check_code,
                });
                let ev2 = ev.clone();
                // The sender node must stay alive until the receiver has verified it.
                let keep = node.clone();
                tokio::spawn(async move {
                    let result = job.wait().await;
                    drop(keep);
                    let result = match result {
                        Ok(SendSummary { files, bytes, .. }) => Ok((files, bytes)),
                        Err(e) => Err(Problem::from_error(&e)),
                    };
                    let _ = ev2.unbounded_send(EngineEvent::SendEnded { id: tid, result });
                });
            }
            Cmd::CancelSend => {
                if let Some(c) = &send_cancel {
                    c.cancel();
                }
            }
            Cmd::Decide { id, accept } => {
                if let Some(tx) = pending.lock().unwrap().remove(&id) {
                    let _ = tx.send(accept);
                }
            }
            Cmd::SetReceiving(on) => {
                receiving.store(on, Ordering::Relaxed);
                if !on {
                    // Anything still being asked is declined.
                    pending.lock().unwrap().clear();
                }
            }
            Cmd::StartQr(paths) => {
                if let Some(old) = qr.take() {
                    old.revoke();
                }
                match QrShare::start(QrOptions {
                    files: paths.clone(),
                    ..QrOptions::default()
                })
                .await
                {
                    Ok(share) => {
                        let share = Arc::new(share);
                        let (side, modules) =
                            panoply_share::qr::qr_modules(share.url()).unwrap_or((0, Vec::new()));
                        let _ = ev.unbounded_send(EngineEvent::QrReady(QrView {
                            url: share.url().to_string(),
                            side,
                            modules,
                            on_lan: share.on_lan(),
                            files: paths.len(),
                            visitors: 0,
                            downloads: 0,
                            closed: None,
                        }));
                        let mut qe = share.subscribe();
                        let ev2 = ev.clone();
                        tokio::spawn(async move {
                            while let Ok(e) = qe.recv().await {
                                let (visit, download) = match e {
                                    QrEvent::Connected { .. } => (true, false),
                                    QrEvent::Downloaded { .. } => (false, true),
                                    _ => continue,
                                };
                                let _ =
                                    ev2.unbounded_send(EngineEvent::QrActivity { visit, download });
                            }
                        });
                        let waiter = share.clone();
                        let ev3 = ev.clone();
                        tokio::spawn(async move {
                            let c = match waiter.wait().await {
                                QrOutcome::Completed => QrClosed::Completed,
                                QrOutcome::Expired => QrClosed::Expired,
                                QrOutcome::Revoked => QrClosed::Revoked,
                            };
                            let _ = ev3.unbounded_send(EngineEvent::QrClosed(c));
                        });
                        qr = Some(share);
                    }
                    Err(e) => {
                        let _ = ev.unbounded_send(EngineEvent::QrFailed(Problem::from_error(&e)));
                    }
                }
            }
            Cmd::StopQr => {
                if let Some(q) = qr.take() {
                    q.revoke();
                }
            }
            Cmd::Stop => break,
        }
    }
    if let Some(q) = qr.take() {
        q.revoke();
    }
    if let Some(s) = scanning.take() {
        s.abort();
    }
    forward.abort();
    node.shutdown();
}

fn rows(node: &Node) -> Vec<DeviceRow> {
    let mut v: Vec<DeviceRow> = node.devices().iter().map(DeviceRow::from_device).collect();
    v.sort_by(|a, b| {
        a.alias
            .to_lowercase()
            .cmp(&b.alias.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::channel::mpsc::UnboundedReceiver;
    use std::time::Instant;

    fn model() -> ShareModel {
        ShareModel::new(PathBuf::from("/tmp/dl"))
    }

    fn dev(id: &str, alias: &str) -> DeviceRow {
        DeviceRow {
            id: id.into(),
            alias: alias.into(),
            model: "macOS".into(),
            kind: DeviceKind::Computer,
            addr: "192.168.1.20".into(),
        }
    }

    fn sending(m: &mut ShareModel) {
        m.stage(&[PathBuf::from("/nonexistent/a.pdf")]);
        m.begin_send("Pixel 9");
        m.apply(EngineEvent::SendStarted {
            id: "t1".into(),
            check_code: None,
        });
    }

    fn transfer(kind: TransferKind, dir: Direction) -> EngineEvent {
        EngineEvent::Transfer {
            id: "t1".into(),
            direction: dir,
            peer: "Pixel 9".into(),
            kind,
        }
    }

    #[test]
    fn empty_state_changes_after_the_first_sweep() {
        let mut m = model();
        assert_eq!(empty_copy(&m).unwrap().0, "Looking for devices");
        m.apply(EngineEvent::ScanFinished);
        let (title, body) = empty_copy(&m).unwrap();
        assert_eq!(title, "No devices found");
        assert!(body.contains("firewall"));
        assert!(body.contains("QR"));
        m.apply(EngineEvent::Devices(vec![dev("a", "Mac mini")]));
        assert!(empty_copy(&m).is_none());
    }

    #[test]
    fn closing_the_screen_forgets_devices_and_prompts() {
        let mut m = model();
        m.apply(EngineEvent::Listening { alias: "Me".into() });
        m.apply(EngineEvent::Devices(vec![dev("a", "Mac mini")]));
        m.apply(EngineEvent::ScanFinished);
        m.apply(EngineEvent::Incoming(IncomingView {
            id: "in-1".into(),
            sender: "Pixel".into(),
            check_code: "a b c d".into(),
            verified: true,
            files: vec![],
            total: 0,
            text: None,
        }));
        m.stopped();
        assert_eq!(m.net, Net::Off);
        assert!(m.devices.is_empty() && m.incoming.is_none() && !m.scanned);
    }

    #[test]
    fn a_send_walks_through_waiting_sending_done() {
        let mut m = model();
        sending(&mut m);
        let s = m.send.as_ref().unwrap();
        assert_eq!(s.phase, SendPhase::Waiting);
        assert_eq!(s.fraction(), None);
        assert!(s.active());

        m.apply(transfer(
            TransferKind::Started {
                files: 2,
                bytes: 1000,
            },
            Direction::Send,
        ));
        m.apply(transfer(
            TransferKind::Progress {
                file: "a.pdf".into(),
                file_done: 250,
                file_size: 500,
                total_done: 250,
                total: 1000,
            },
            Direction::Send,
        ));
        let s = m.send.as_ref().unwrap();
        assert_eq!(s.files, 2);
        assert_eq!(s.fraction(), Some(0.25));

        m.apply(EngineEvent::SendEnded {
            id: "t1".into(),
            result: Ok((2, 1000)),
        });
        let s = m.send.as_ref().unwrap();
        assert_eq!(
            s.phase,
            SendPhase::Done {
                files: 2,
                bytes: 1000
            }
        );
        assert_eq!(s.fraction(), Some(1.0));
        assert!(!s.active());
        m.dismiss_send();
        assert!(m.send.is_none());
    }

    #[test]
    fn events_of_another_transfer_are_ignored() {
        let mut m = model();
        sending(&mut m);
        m.apply(EngineEvent::Transfer {
            id: "other".into(),
            direction: Direction::Send,
            peer: "x".into(),
            kind: TransferKind::Cancelled,
        });
        assert_eq!(m.send.as_ref().unwrap().phase, SendPhase::Waiting);
        m.apply(EngineEvent::SendEnded {
            id: "other".into(),
            result: Err(Problem::Declined),
        });
        assert_eq!(m.send.as_ref().unwrap().phase, SendPhase::Waiting);
    }

    #[test]
    fn declined_cancelled_and_unreachable_are_told_apart() {
        for (error, expect) in [
            (Error::Rejected, Problem::Declined),
            (Error::Busy, Problem::Busy),
            (Error::Cancelled, Problem::Cancelled),
            (Error::Timeout, Problem::Unreachable),
            (
                Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionRefused)),
                Problem::Unreachable,
            ),
        ] {
            assert_eq!(Problem::from_error(&error), expect);
        }
        let mut m = model();
        sending(&mut m);
        m.apply(EngineEvent::SendEnded {
            id: "t1".into(),
            result: Err(Problem::Cancelled),
        });
        assert_eq!(m.send.as_ref().unwrap().phase, SendPhase::Cancelled);

        let (head, body) = Problem::Unreachable.words("Pixel 9");
        assert_eq!(head, "Pixel 9 did not answer");
        assert!(body.contains("firewall"));
        assert_eq!(Problem::Declined.words("Pixel 9").0, "Pixel 9 declined");
    }

    #[test]
    fn staging_skips_duplicates_and_clears_an_old_result() {
        let dir = std::env::temp_dir().join(format!("panoply-share-stage-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("sub/b.txt"), vec![0u8; 50]).unwrap();
        let mut m = model();
        m.stage(&[dir.join("a.txt"), dir.join("a.txt"), dir.join("sub")]);
        assert_eq!(m.staged.len(), 2);
        assert_eq!(m.staged_bytes(), 150);
        assert!(m.staged[1].folder);
        assert_eq!(staged_label(&m.staged), "2 items, 150 bytes");
        m.begin_send("Mac");
        m.apply(EngineEvent::SendEnded {
            id: String::new(),
            result: Err(Problem::Declined),
        });
        m.stage(&[dir.join("sub/b.txt")]);
        assert!(m.send.is_none(), "adding files starts over");
        m.unstage(&dir.join("sub"));
        assert_eq!(m.staged.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_incoming_prompt_becomes_a_receive_and_lands_done() {
        let mut m = model();
        let view = IncomingView {
            id: "in-1".into(),
            sender: "Pixel 9".into(),
            check_code: "river stone lamp oak".into(),
            verified: true,
            files: vec![IncomingFileRow {
                name: "trip.zip".into(),
                size: 4000,
                risky: false,
            }],
            total: 4000,
            text: None,
        };
        m.apply(EngineEvent::Incoming(view));
        assert!(m.incoming.is_some());
        m.answered(true);
        assert!(m.incoming.is_none());
        assert_eq!(
            m.receive.as_ref().unwrap().phase,
            ReceivePhase::Receiving {
                done: 0,
                total: 4000
            }
        );
        m.apply(transfer(
            TransferKind::Progress {
                file: "trip.zip".into(),
                file_done: 1000,
                file_size: 4000,
                total_done: 1000,
                total: 4000,
            },
            Direction::Receive,
        ));
        m.apply(transfer(
            TransferKind::FileDone {
                file: "trip.zip".into(),
                path: Some(PathBuf::from("/tmp/dl/trip.zip")),
            },
            Direction::Receive,
        ));
        m.apply(transfer(
            TransferKind::Finished {
                files: 1,
                bytes: 4000,
                elapsed: Duration::from_secs(1),
            },
            Direction::Receive,
        ));
        let r = m.receive.as_ref().unwrap();
        assert_eq!(
            r.phase,
            ReceivePhase::Done {
                files: 1,
                bytes: 4000
            }
        );
        assert_eq!(r.first.as_deref(), Some(Path::new("/tmp/dl/trip.zip")));
    }

    #[test]
    fn declining_clears_the_prompt_and_starts_no_receive() {
        let mut m = model();
        m.apply(EngineEvent::Incoming(IncomingView {
            id: "in-2".into(),
            sender: "Pixel".into(),
            check_code: String::new(),
            verified: false,
            files: vec![],
            total: 1,
            text: None,
        }));
        m.answered(false);
        assert!(m.incoming.is_none() && m.receive.is_none());
        // A prompt that the sender gave up on goes away by itself.
        m.apply(EngineEvent::Incoming(IncomingView {
            id: "in-3".into(),
            sender: "Pixel".into(),
            check_code: String::new(),
            verified: false,
            files: vec![],
            total: 1,
            text: None,
        }));
        m.apply(EngineEvent::IncomingGone("in-9".into()));
        assert!(m.incoming.is_some());
        m.apply(EngineEvent::IncomingGone("in-3".into()));
        assert!(m.incoming.is_none());
    }

    #[test]
    fn qr_counts_visitors_and_remembers_why_it_closed() {
        let mut m = model();
        m.apply(EngineEvent::QrReady(QrView {
            url: "http://192.168.1.5:5000/abc".into(),
            side: 21,
            modules: vec![false; 441],
            on_lan: true,
            files: 3,
            visitors: 0,
            downloads: 0,
            closed: None,
        }));
        m.apply(EngineEvent::QrActivity {
            visit: true,
            download: false,
        });
        m.apply(EngineEvent::QrActivity {
            visit: false,
            download: true,
        });
        m.apply(EngineEvent::QrClosed(QrClosed::Completed));
        let q = m.qr.as_ref().unwrap();
        assert_eq!(
            (q.visitors, q.downloads, q.closed),
            (1, 1, Some(QrClosed::Completed))
        );
        m.apply(EngineEvent::QrFailed(Problem::CannotListen("busy".into())));
        assert!(m.qr.is_none() && m.note.is_some());
    }

    #[test]
    fn listening_failure_is_a_screen_state() {
        let mut m = model();
        m.starting();
        assert_eq!(m.net, Net::Starting);
        m.apply(EngineEvent::NetFailed(Problem::CannotListen(
            "address in use".into(),
        )));
        let Net::Failed(p) = &m.net else { panic!() };
        let (head, body) = p.words("");
        assert_eq!(head, "Panoply cannot use the network");
        assert!(body.starts_with("Address in use"));
    }

    #[test]
    fn check_code_is_offered_only_for_panoply_peers() {
        let mut info = panoply_share::DeviceInfo {
            alias: "Pixel".into(),
            version: "2.1".into(),
            device_model: Some("Android".into()),
            device_type: None,
            fingerprint: "f".into(),
            port: None,
            protocol: None,
            download: None,
            announce: None,
        };
        assert!(!panoply_share::is_panoply(&info));
        info.device_model = Some("Panoply macOS".into());
        assert!(panoply_share::is_panoply(&info));
    }

    // ---- the engine, over loopback -----------------------------------------------------------

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    /// Wait for an event the check likes, up to ten seconds, and keep the model in step.
    fn pump(
        rx: &mut UnboundedReceiver<EngineEvent>,
        m: &mut ShareModel,
        mut until: impl FnMut(&ShareModel, &EngineEvent) -> bool,
    ) -> EngineEvent {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            match rx.try_recv() {
                Ok(ev) => {
                    m.apply(ev.clone());
                    if until(m, &ev) {
                        return ev;
                    }
                }
                Err(futures::channel::mpsc::TryRecvError::Closed) => panic!("the engine stopped"),
                Err(_) => {
                    assert!(Instant::now() < end, "timed out waiting for the engine");
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }

    fn loopback(
        alias: &str,
        dir: &Path,
        port: u16,
        receiving: bool,
        probe: Vec<std::net::SocketAddr>,
    ) -> EngineConfig {
        EngineConfig {
            alias: alias.into(),
            save_dir: dir.to_path_buf(),
            data_dir: None,
            receiving,
            discovery: false,
            bind: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            port,
            probe,
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("panoply-share-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn bytes(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 31 % 251) as u8).collect()
    }

    #[test]
    fn engine_sends_a_file_over_loopback_and_the_bytes_match() {
        let src = scratch("src");
        let dst = scratch("dst");
        let payload = bytes(2 * 1024 * 1024 + 123);
        std::fs::write(src.join("photo.bin"), &payload).unwrap();

        let rport = free_port();
        let (_recv, mut rrx) = Engine::start(loopback("Receiver", &dst, rport, true, vec![]));
        let (send, mut srx) = Engine::start(loopback(
            "Sender",
            &src,
            0,
            false,
            vec![std::net::SocketAddr::from(([127, 0, 0, 1], rport))],
        ));
        let mut sm = model();
        let mut rm = model();
        pump(&mut rrx, &mut rm, |m, _| matches!(m.net, Net::On { .. }));
        pump(&mut srx, &mut sm, |m, _| !m.devices.is_empty() && m.scanned);
        assert_eq!(sm.devices[0].alias, "Receiver");
        assert!(empty_copy(&sm).is_none());

        let id = sm.devices[0].id.clone();
        sm.stage(&[src.join("photo.bin")]);
        sm.begin_send("Receiver");
        send.send(&id, vec![src.join("photo.bin")]);

        // The receiver is asked, and says yes.
        let ev = pump(&mut rrx, &mut rm, |m, _| m.incoming.is_some());
        let EngineEvent::Incoming(i) = ev else {
            panic!()
        };
        assert_eq!(i.sender, "Sender");
        assert_eq!(i.files[0].name, "photo.bin");
        assert_eq!(i.total, payload.len() as u64);
        assert_eq!(
            i.check_code.split(' ').count(),
            4,
            "Panoply to Panoply shows a code"
        );
        let code = i.check_code.clone();
        let prompt = rm.incoming.clone().unwrap();
        _recv.decide(&prompt.id, true);
        rm.answered(true);

        pump(&mut srx, &mut sm, |m, _| {
            matches!(
                m.send.as_ref().map(|s| &s.phase),
                Some(SendPhase::Done { .. })
            )
        });
        assert_eq!(
            sm.send.as_ref().unwrap().phase,
            SendPhase::Done {
                files: 1,
                bytes: payload.len() as u64
            }
        );
        pump(&mut rrx, &mut rm, |m, _| {
            matches!(
                m.receive.as_ref().map(|r| &r.phase),
                Some(ReceivePhase::Done { .. })
            )
        });
        assert_eq!(std::fs::read(dst.join("photo.bin")).unwrap(), payload);
        assert!(rm.receive.as_ref().unwrap().first.is_some());
        assert_eq!(
            sm.send.as_ref().unwrap().check_code.as_deref(),
            Some(code.as_str()),
            "both screens show the same words"
        );

        drop(send);
        drop(_recv);
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }

    #[test]
    fn engine_declined_send_says_declined_and_saves_nothing() {
        let src = scratch("src2");
        let dst = scratch("dst2");
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        let rport = free_port();
        let (recv, mut rrx) = Engine::start(loopback("Receiver", &dst, rport, true, vec![]));
        let (send, mut srx) = Engine::start(loopback(
            "Sender",
            &src,
            0,
            false,
            vec![std::net::SocketAddr::from(([127, 0, 0, 1], rport))],
        ));
        let mut sm = model();
        let mut rm = model();
        pump(&mut srx, &mut sm, |m, _| !m.devices.is_empty());
        let id = sm.devices[0].id.clone();
        sm.stage(&[src.join("a.txt")]);
        sm.begin_send("Receiver");
        send.send(&id, vec![src.join("a.txt")]);
        pump(&mut rrx, &mut rm, |m, _| m.incoming.is_some());
        recv.decide(&rm.incoming.clone().unwrap().id, false);
        pump(&mut srx, &mut sm, |m, _| {
            matches!(
                m.send.as_ref().map(|s| &s.phase),
                Some(SendPhase::Failed(_))
            )
        });
        assert_eq!(
            sm.send.as_ref().unwrap().phase,
            SendPhase::Failed(Problem::Declined)
        );
        assert!(!dst.join("a.txt").exists());
        drop(send);
        drop(recv);
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }

    #[test]
    fn a_node_that_is_not_receiving_declines_without_a_prompt() {
        let src = scratch("src3");
        let dst = scratch("dst3");
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        let rport = free_port();
        let (recv, mut rrx) = Engine::start(loopback("Receiver", &dst, rport, false, vec![]));
        let (send, mut srx) = Engine::start(loopback(
            "Sender",
            &src,
            0,
            false,
            vec![std::net::SocketAddr::from(([127, 0, 0, 1], rport))],
        ));
        let (mut sm, mut rm) = (model(), model());
        pump(&mut rrx, &mut rm, |m, _| matches!(m.net, Net::On { .. }));
        pump(&mut srx, &mut sm, |m, _| !m.devices.is_empty());
        sm.begin_send("Receiver");
        send.send(&sm.devices[0].id.clone(), vec![src.join("a.txt")]);
        pump(&mut srx, &mut sm, |m, _| {
            matches!(
                m.send.as_ref().map(|s| &s.phase),
                Some(SendPhase::Failed(_))
            )
        });
        assert!(rm.incoming.is_none());
        while let Ok(ev) = rrx.try_recv() {
            assert!(!matches!(ev, EngineEvent::Incoming(_)));
        }
        drop(send);
        drop(recv);
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }

    #[test]
    fn a_dropped_engine_leaves_the_port_free() {
        let dst = scratch("dst4");
        let port = free_port();
        let (e, mut rx) = Engine::start(loopback("A", &dst, port, true, vec![]));
        let mut m = model();
        pump(&mut rx, &mut m, |m, _| matches!(m.net, Net::On { .. }));
        drop(e);
        // Nothing listens any more: the same port binds again at once.
        std::net::TcpListener::bind(("127.0.0.1", port)).expect("port is free after drop");
        let _ = std::fs::remove_dir_all(&dst);
    }

    #[test]
    fn qr_page_opens_and_closes() {
        let src = scratch("src5");
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        let (e, mut rx) = Engine::start(loopback("A", &src, 0, false, vec![]));
        let mut m = model();
        pump(&mut rx, &mut m, |m, _| matches!(m.net, Net::On { .. }));
        e.start_qr(vec![src.join("a.txt")]);
        pump(&mut rx, &mut m, |m, _| m.qr.is_some());
        let q = m.qr.as_ref().unwrap();
        assert!(q.url.starts_with("http://"));
        assert_eq!(q.modules.len(), q.side * q.side);
        assert!(q.modules.iter().any(|d| *d));
        e.stop_qr();
        pump(&mut rx, &mut m, |m, _| {
            m.qr.as_ref().is_some_and(|q| q.closed.is_some())
        });
        assert_eq!(m.qr.as_ref().unwrap().closed, Some(QrClosed::Revoked));
        let _ = std::fs::remove_dir_all(&src);
    }
}
