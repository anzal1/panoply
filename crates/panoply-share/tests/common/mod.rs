//! Helpers shared by the integration tests.
#![allow(dead_code)]

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use panoply_share::{
    Device, Event, Node, NodeConfig, ReceiveConfig, ReceiveHandler, TransferEvent, TransferKind,
    accept_all_handler,
};
use sha2::{Digest, Sha256};

pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "panoply-share-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, p: &str) -> PathBuf {
        self.0.join(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A loopback node: no multicast, free port, in-memory identity.
pub fn loopback_config(alias: &str) -> NodeConfig {
    let mut c = NodeConfig::new(alias);
    c.bind = IpAddr::V4(Ipv4Addr::LOCALHOST);
    c.port = 0;
    c.discovery = false;
    c
}

pub async fn receiver(alias: &str, dir: &Path, handler: Arc<dyn ReceiveHandler>) -> Node {
    let mut c = loopback_config(alias);
    c.receive = Some(ReceiveConfig::new(dir, handler));
    Node::start(c).await.unwrap()
}

pub async fn accepting_receiver(alias: &str, dir: &Path) -> Node {
    receiver(alias, dir, accept_all_handler()).await
}

pub async fn sender(alias: &str) -> Node {
    Node::start(loopback_config(alias)).await.unwrap()
}

/// The device record `from` would have for `to` after one register round trip.
pub async fn meet(from: &Node, to: &Node) -> Device {
    let found = from.scan_addrs(&[to.local_addr()]).await;
    assert_eq!(found.len(), 1, "the register probe must find the peer");
    found.into_iter().next().unwrap()
}

pub fn sha256_file(p: &Path) -> String {
    let mut h = Sha256::new();
    h.update(std::fs::read(p).unwrap());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Deterministic pseudo random bytes.
pub fn blob(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

/// Wait for a transfer event matching `f`.
pub async fn wait_for(
    rx: &mut tokio::sync::broadcast::Receiver<Event>,
    mut f: impl FnMut(&TransferEvent) -> bool,
) -> TransferEvent {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match rx.recv().await {
                Ok(Event::Transfer(t)) if f(&t) => return t,
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(e) => panic!("event stream ended: {e}"),
            }
        }
    })
    .await
    .expect("timed out waiting for an event")
}

pub fn is_finished(t: &TransferEvent) -> bool {
    matches!(t.kind, TransferKind::Finished { .. })
}

/// All regular files below `dir`, as sorted relative paths.
pub fn tree(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                walk(base, &p, out);
            } else {
                out.push(
                    p.strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut v = Vec::new();
    walk(dir, dir, &mut v);
    v.sort();
    v
}

// ---- external processes ---------------------------------------------------------------------

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;

/// A child process whose stdout lines can be waited for. Killed on drop.
pub struct Proc {
    child: Child,
    lines: mpsc::Receiver<String>,
    pub seen: Vec<String>,
}

impl Proc {
    pub fn spawn(mut cmd: Command) -> Proc {
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .stdin(Stdio::null())
            .spawn()
            .expect("spawn");
        let out = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for l in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        Proc {
            child,
            lines: rx,
            seen: Vec::new(),
        }
    }

    /// Wait for a line starting with `prefix` and return it.
    pub fn expect(&mut self, prefix: &str, secs: u64) -> String {
        let deadline = std::time::Instant::now() + Duration::from_secs(secs);
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(l) => {
                    self.seen.push(l.clone());
                    if l.starts_with(prefix) {
                        return l;
                    }
                }
                Err(_) => panic!(
                    "no line starting with {prefix:?} in {secs}s; saw {:?}",
                    self.seen
                ),
            }
        }
    }

    pub fn wait(&mut self) -> bool {
        self.child.wait().map(|s| s.success()).unwrap_or(false)
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A python3 with TLS 1.3, if the machine has one (the interop peer needs it).
pub fn tls13_python() -> Option<String> {
    [
        "python3",
        "/opt/homebrew/bin/python3",
        "/usr/local/bin/python3",
    ]
    .iter()
    .find(|p| {
        Command::new(p)
            .args(["-c", "import ssl,sys;sys.exit(0 if ssl.HAS_TLSv1_3 else 1)"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
    .map(|s| s.to_string())
}

/// Make a self-signed RSA certificate with the openssl command line tool, the way many stock
/// LocalSend style apps do (note: `openssl req -x509` marks it as a CA).
pub fn openssl_cert(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let cert = dir.join("mock-cert.pem");
    let key = dir.join("mock-key.pem");
    let ok = Command::new("openssl")
        .args([
            "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=mock",
        ])
        .arg("-keyout")
        .arg(&key)
        .arg("-out")
        .arg(&cert)
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    ok.then_some((cert, key))
}

pub fn mock_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_localsend.py")
}

pub fn free_tcp_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

pub fn free_udp_port() -> u16 {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

pub fn have(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}
