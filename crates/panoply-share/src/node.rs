//! A running Panoply share device: the HTTP(S) server, discovery, trust and events.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use panoply_core::CancelToken;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, watch};

use crate::discovery;
use crate::error::{Error, Result};
use crate::http;
use crate::identity::Identity;
use crate::netif;
use crate::proto::{
    DEFAULT_PORT, DeviceInfo, DeviceType, MULTICAST_GROUP, PROTOCOL_VERSION, Scheme, random_hex,
};
use crate::receive::{self, ReceiveConfig, ReceiveState};
use crate::send::{SendJob, SendOptions};
use crate::trust::TrustStore;

/// Which way a transfer goes, from this device's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// This device is sending.
    Send,
    /// This device is receiving.
    Receive,
}

/// Something that happened during a transfer.
#[derive(Debug, Clone)]
pub enum TransferKind {
    /// The receiver accepted and bytes are about to move.
    Started {
        /// Number of files accepted.
        files: usize,
        /// Their total size.
        bytes: u64,
    },
    /// Bytes moved. Throttled to about ten events a second.
    Progress {
        /// Name of the file in flight.
        file: String,
        /// Bytes of this file done.
        file_done: u64,
        /// Size of this file.
        file_size: u64,
        /// Bytes done across the whole transfer.
        total_done: u64,
        /// Bytes in the whole transfer.
        total: u64,
    },
    /// One file is complete. `path` is where it was saved (receiving only).
    FileDone {
        /// File name as sent.
        file: String,
        /// Where it landed on this device.
        path: Option<PathBuf>,
    },
    /// A text message arrived (receiving only).
    Text {
        /// The message.
        text: String,
    },
    /// Everything finished.
    Finished {
        /// Files moved.
        files: usize,
        /// Bytes moved.
        bytes: u64,
        /// Wall time.
        elapsed: Duration,
    },
    /// The transfer stopped with an error.
    Failed {
        /// Plain message.
        error: String,
    },
    /// The transfer was cancelled by either side.
    Cancelled,
}

/// One event on a transfer.
#[derive(Debug, Clone)]
pub struct TransferEvent {
    /// Identifies the transfer (the session id when receiving).
    pub id: String,
    /// Direction.
    pub direction: Direction,
    /// Alias of the other device.
    pub peer: String,
    /// What happened.
    pub kind: TransferKind,
}

/// Events published by a [`Node`].
#[derive(Debug, Clone)]
pub enum Event {
    /// A device appeared (or changed alias, address or port).
    DeviceFound(Device),
    /// Transfer progress and results.
    Transfer(TransferEvent),
}

/// A device on the network.
#[derive(Debug, Clone)]
pub struct Device {
    /// What the device says about itself, with `port` and `protocol` always filled in.
    pub info: DeviceInfo,
    /// Where it was heard from.
    pub addr: IpAddr,
    /// When it was last heard from.
    pub last_seen: SystemTime,
}

impl Device {
    /// The socket address of its server.
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.addr, self.info.port_or_default())
    }

    /// Display name.
    pub fn alias(&self) -> &str {
        &self.info.alias
    }

    /// Certificate fingerprint (or random id in HTTP mode).
    pub fn fingerprint(&self) -> &str {
        &self.info.fingerprint
    }
}

/// How the node starts.
#[derive(Clone)]
pub struct NodeConfig {
    /// Display name announced to others.
    pub alias: String,
    /// Hardware or OS label.
    pub device_model: Option<String>,
    /// Device kind.
    pub device_type: DeviceType,
    /// HTTPS (default) or plain HTTP.
    pub scheme: Scheme,
    /// Address to bind the server to. Default `0.0.0.0`.
    pub bind: IpAddr,
    /// Server port. `0` picks a free one.
    pub port: u16,
    /// When the port is taken, pick a free one instead of failing. Right for a sender that
    /// only needs a port other devices can use to verify it.
    pub ephemeral_if_busy: bool,
    /// Run multicast discovery.
    pub discovery: bool,
    /// UDP port for discovery.
    pub multicast_port: u16,
    /// Multicast group.
    pub multicast_group: Ipv4Addr,
    /// Port the HTTP scan probes on other hosts, besides our own port (default 53317).
    pub scan_port: u16,
    /// Use exactly these local addresses for discovery instead of the detected LANs.
    pub interfaces: Option<Vec<Ipv4Addr>>,
    /// Folder for the identity and the trust list. `None` keeps both in memory.
    pub data_dir: Option<PathBuf>,
    /// Receiving settings. `None` means this node does not receive: `prepare-upload` is
    /// answered with a decline.
    pub receive: Option<ReceiveConfig>,
}

impl NodeConfig {
    /// A config with defaults: HTTPS on port 53317, discovery on, not receiving.
    pub fn new(alias: impl Into<String>) -> Self {
        NodeConfig {
            alias: alias.into(),
            device_model: Some(format!(
                "{}{}",
                crate::proto::PANOPLY_MODEL_PREFIX,
                crate::proto::os_label()
            )),
            device_type: DeviceType::Desktop,
            scheme: Scheme::Https,
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: DEFAULT_PORT,
            ephemeral_if_busy: false,
            discovery: true,
            multicast_port: DEFAULT_PORT,
            multicast_group: MULTICAST_GROUP,
            scan_port: DEFAULT_PORT,
            interfaces: None,
            data_dir: None,
            receive: None,
        }
    }
}

/// The default folder for the identity and trust list: `PANOPLY_DATA_DIR/share`, else the
/// per-user data folder of the platform.
pub fn default_data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("PANOPLY_DATA_DIR") {
        return PathBuf::from(p).join("share");
    }
    let home = || {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
    };
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/Panoply/share")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join("AppData/Roaming"))
            .join("Panoply/share")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"))
            .join("panoply/share")
    }
}

/// A cancel switch that can be polled (`is_cancelled`) and awaited (`subscribe`).
#[derive(Clone)]
pub(crate) struct Canceller {
    token: CancelToken,
    tx: Arc<watch::Sender<bool>>,
}

impl Canceller {
    pub fn new() -> Self {
        Canceller {
            token: CancelToken::new(),
            tx: Arc::new(watch::channel(false).0),
        }
    }

    pub fn cancel(&self) {
        self.token.cancel();
        let _ = self.tx.send(true);
    }

    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.tx.subscribe()
    }
}

pub(crate) struct Shared {
    pub fingerprint: String,
    pub alias: String,
    pub device_model: Option<String>,
    pub device_type: DeviceType,
    pub scheme: Scheme,
    pub port: u16,
    pub multicast_port: u16,
    pub multicast_group: Ipv4Addr,
    pub scan_port: u16,
    pub lans: Vec<Ipv4Addr>,
    pub trust: Arc<TrustStore>,
    pub devices: Mutex<HashMap<String, Device>>,
    pub events: broadcast::Sender<Event>,
    pub receive: Option<ReceiveState>,
    pub active: Mutex<HashMap<String, Canceller>>,
    pub shutdown: watch::Sender<bool>,
    pub local_addr: SocketAddr,
}

impl Shared {
    /// Our own description. `announce` is set only for multicast messages.
    pub fn info(&self, announce: Option<bool>) -> DeviceInfo {
        DeviceInfo {
            alias: self.alias.clone(),
            version: PROTOCOL_VERSION.into(),
            device_model: self.device_model.clone(),
            device_type: Some(self.device_type),
            fingerprint: self.fingerprint.clone(),
            port: Some(self.port),
            protocol: Some(self.scheme),
            download: Some(false),
            announce,
        }
    }

    pub fn emit(&self, e: Event) {
        let _ = self.events.send(e);
    }

    pub fn emit_transfer(&self, id: &str, direction: Direction, peer: &str, kind: TransferKind) {
        self.emit(Event::Transfer(TransferEvent {
            id: id.to_string(),
            direction,
            peer: peer.to_string(),
            kind,
        }));
    }

    /// Record a device heard from `ip`. Returns the stored device, or `None` for ourselves and
    /// for peers on an incompatible protocol version.
    pub fn add_device(&self, info: DeviceInfo, ip: IpAddr) -> Option<Device> {
        let mut info = info.sanitized();
        if info.fingerprint == self.fingerprint || !info.version_compatible() {
            return None;
        }
        info.port = Some(info.port_or_default());
        info.protocol = Some(info.scheme_or_default());
        info.announce = None;
        let dev = Device {
            info,
            addr: ip,
            last_seen: SystemTime::now(),
        };
        let changed = {
            let mut map = self.devices.lock().unwrap();
            let changed = match map.get(&dev.info.fingerprint) {
                Some(old) => {
                    old.addr != dev.addr
                        || old.info.alias != dev.info.alias
                        || old.info.port != dev.info.port
                        || old.info.protocol != dev.info.protocol
                }
                None => true,
            };
            map.insert(dev.info.fingerprint.clone(), dev.clone());
            changed
        };
        if changed {
            self.emit(Event::DeviceFound(dev.clone()));
        }
        Some(dev)
    }
}

/// Stops the server when the last [`Node`] handle is dropped.
struct StopOnDrop(Arc<Shared>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let _ = self.0.shutdown.send(true);
        if let Some(r) = &self.0.receive {
            r.cancel_all();
        }
    }
}

/// A running device. Cheap to clone; all clones share one server, which stops when the last
/// clone is dropped or [`Node::shutdown`] is called. Keep the node alive while a send is
/// running: receivers connect back to it to verify the sender.
#[derive(Clone)]
pub struct Node {
    pub(crate) shared: Arc<Shared>,
    _stop: Arc<StopOnDrop>,
}

impl Node {
    /// Bind the server, load or create the identity and trust list, start answering requests
    /// and (when enabled) listening for and announcing on the multicast group.
    ///
    /// Must be called inside a tokio runtime.
    pub async fn start(config: NodeConfig) -> Result<Node> {
        let (identity, trust) = match (&config.data_dir, config.scheme) {
            (Some(dir), Scheme::Https) => (
                Some(Identity::load_or_create(dir)?),
                TrustStore::open(&dir.join("trust.json")),
            ),
            (Some(dir), Scheme::Http) => (None, TrustStore::open(&dir.join("trust.json"))),
            (None, Scheme::Https) => (Some(Identity::generate()?), TrustStore::in_memory()),
            (None, Scheme::Http) => (None, TrustStore::in_memory()),
        };
        let fingerprint = match &identity {
            Some(id) => id.fingerprint().to_string(),
            None => random_hex(16),
        };
        let tls = identity.as_ref().map(Identity::server_config).transpose()?;

        let bind = SocketAddr::new(config.bind, config.port);
        let listener = match TcpListener::bind(bind).await {
            Ok(l) => l,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && config.ephemeral_if_busy => {
                TcpListener::bind(SocketAddr::new(config.bind, 0)).await?
            }
            Err(e) => {
                return Err(Error::Bind(format!("cannot listen on {bind}: {e}")));
            }
        };
        let local_addr = listener.local_addr()?;

        let lans = match &config.interfaces {
            Some(v) => v.clone(),
            None => netif::lan_interfaces().into_iter().map(|l| l.ip).collect(),
        };
        let (events, _) = broadcast::channel(1024);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let receive = config.receive.map(ReceiveState::new);
        let shared = Arc::new(Shared {
            fingerprint,
            alias: config.alias,
            device_model: config.device_model,
            device_type: config.device_type,
            scheme: config.scheme,
            port: local_addr.port(),
            multicast_port: config.multicast_port,
            multicast_group: config.multicast_group,
            scan_port: config.scan_port,
            lans,
            trust: Arc::new(trust),
            devices: Mutex::default(),
            events,
            receive,
            active: Mutex::default(),
            shutdown,
            local_addr,
        });

        let handler_state = shared.clone();
        tokio::spawn(http::serve(
            listener,
            tls,
            move |req, peer| receive::handle(handler_state.clone(), req, peer),
            shutdown_rx.clone(),
        ));

        let node = Node {
            _stop: Arc::new(StopOnDrop(shared.clone())),
            shared,
        };
        if config.discovery {
            discovery::start(&node.shared, shutdown_rx)?;
            discovery::announce(&node.shared);
        }
        Ok(node)
    }

    /// Our own description as announced to others.
    pub fn info(&self) -> DeviceInfo {
        self.shared.info(None)
    }

    /// Our certificate fingerprint (a random id in HTTP mode).
    pub fn fingerprint(&self) -> &str {
        &self.shared.fingerprint
    }

    /// The address the server listens on.
    pub fn local_addr(&self) -> SocketAddr {
        self.shared.local_addr
    }

    /// The trust list.
    pub fn trust(&self) -> &TrustStore {
        &self.shared.trust
    }

    /// Every device heard from so far, most recently seen first.
    pub fn devices(&self) -> Vec<Device> {
        let mut v: Vec<Device> = self
            .shared
            .devices
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect();
        v.sort_by_key(|d| std::cmp::Reverse(d.last_seen));
        v
    }

    /// Subscribe to events. A slow subscriber misses old events instead of blocking the node.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.shared.events.subscribe()
    }

    /// Announce this device on the multicast group (once on every LAN).
    pub fn announce(&self) {
        discovery::announce(&self.shared);
    }

    /// Probe every host of the local subnets with the HTTP `register` call. Finds devices when
    /// multicast is dropped. Takes a few seconds on a /24.
    pub async fn scan(&self) -> Vec<Device> {
        discovery::scan_lans(&self.shared).await
    }

    /// Probe these addresses with the HTTP `register` call.
    pub async fn scan_addrs(&self, addrs: &[SocketAddr]) -> Vec<Device> {
        discovery::scan_addrs(&self.shared, addrs).await
    }

    /// Find a known device by alias (case insensitive), fingerprint prefix, or probe an
    /// `ip` or `ip:port` that is not known yet.
    pub async fn find(&self, query: &str) -> Result<Device> {
        let q = query.trim();
        if q.is_empty() {
            return Err(Error::NoSuchDevice(query.into()));
        }
        let known = self.devices();
        if let Some(d) = known.iter().find(|d| d.info.alias.eq_ignore_ascii_case(q)) {
            return Ok(d.clone());
        }
        let lower = q.to_ascii_lowercase();
        if lower.len() >= 4
            && let Some(d) = known
                .iter()
                .find(|d| d.info.fingerprint.starts_with(&lower))
        {
            return Ok(d.clone());
        }
        let addr = q.parse::<SocketAddr>().ok().or_else(|| {
            q.parse::<IpAddr>()
                .ok()
                .map(|ip| SocketAddr::new(ip, DEFAULT_PORT))
        });
        if let Some(addr) = addr
            && let Some(d) = self.scan_addrs(&[addr]).await.into_iter().next()
        {
            return Ok(d);
        }
        Err(Error::NoSuchDevice(query.into()))
    }

    /// Send files and folders to a device. Returns at once; follow it through the job.
    pub fn send(&self, target: Device, items: Vec<PathBuf>, options: SendOptions) -> SendJob {
        crate::send::start(
            self.shared.clone(),
            target,
            crate::send::Payload::Files(items),
            options,
        )
    }

    /// Send a text message to a device.
    pub fn send_text(&self, target: Device, text: String, options: SendOptions) -> SendJob {
        crate::send::start(
            self.shared.clone(),
            target,
            crate::send::Payload::Text(text),
            options,
        )
    }

    /// Cancel a transfer by its id. Returns false for an unknown id.
    pub fn cancel(&self, transfer_id: &str) -> bool {
        if crate::receive::end_session(&self.shared, transfer_id) {
            return true;
        }
        match self.shared.active.lock().unwrap().get(transfer_id) {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    /// Ids of transfers in flight.
    pub fn active_transfers(&self) -> Vec<String> {
        self.shared.active.lock().unwrap().keys().cloned().collect()
    }

    /// Accept files from any device without asking for the next `d` (quick save).
    pub fn set_quick_save(&self, d: Duration) {
        if let Some(r) = &self.shared.receive {
            r.set_quick_save(d);
        }
    }

    /// The folder files are received into, when receiving.
    pub fn receive_dir(&self) -> Option<&Path> {
        self.shared.receive.as_ref().map(|r| r.config.dir.as_path())
    }

    /// Stop the server and discovery. Transfers in flight are cut.
    pub fn shutdown(&self) {
        let _ = self.shared.shutdown.send(true);
        if let Some(r) = &self.shared.receive {
            r.cancel_all();
        }
    }
}
