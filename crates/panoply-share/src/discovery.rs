//! Finding devices: UDP multicast announcements and the HTTP `register` scan fallback.
//!
//! No periodic beacon. A node announces once when it starts (and when asked to), and answers
//! announcements it hears. Hearing `announce: true` triggers a `POST /register` to the sender
//! with our description; when that cannot connect, the answer goes back over UDP with
//! `announce: false`.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket as StdUdp};
use std::sync::Arc;
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::sync::{Semaphore, watch};

use crate::error::{Error, Result};
use crate::http;
use crate::netif;
use crate::node::{Device, Shared};
use crate::proto::{API_PREFIX, DeviceInfo, Scheme};

const MAX_DATAGRAM: usize = 16 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_millis(1200);
const SCAN_CONCURRENCY: usize = 128;

fn bind_multicast(group: Ipv4Addr, port: u16, ifaces: &[Ipv4Addr]) -> io::Result<StdUdp> {
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    #[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos"))))]
    sock.set_reuse_port(true)?;
    sock.set_nonblocking(true)?;
    sock.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)).into())?;
    let mut joined = false;
    let mut last = None;
    for ip in ifaces {
        match sock.join_multicast_v4(&group, ip) {
            Ok(()) => joined = true,
            Err(e) => last = Some(e),
        }
    }
    if !joined && let Some(e) = last {
        return Err(e);
    }
    Ok(sock.into())
}

/// Start listening for announcements. Does nothing (and is not an error) when there is no
/// usable network.
pub(crate) fn start(shared: &Arc<Shared>, mut shutdown: watch::Receiver<bool>) -> Result<()> {
    if shared.lans.is_empty() {
        return Ok(());
    }
    let std_sock = match bind_multicast(shared.multicast_group, shared.multicast_port, &shared.lans)
    {
        Ok(s) => s,
        // Discovery is best effort: a busy or filtered UDP port must not stop sending files.
        Err(_) => return Ok(()),
    };
    let sock = UdpSocket::from_std(std_sock)?;
    let shared = shared.clone();
    tokio::spawn(async move {
        let mut buf = vec![0u8; MAX_DATAGRAM];
        let mut last_reply: HashMap<String, Instant> = HashMap::new();
        loop {
            let (n, from) = tokio::select! {
                r = sock.recv_from(&mut buf) => match r {
                    Ok(v) => v,
                    Err(_) => continue,
                },
                _ = shutdown.changed() => return,
            };
            let Ok(info) = serde_json::from_slice::<DeviceInfo>(&buf[..n]) else {
                continue;
            };
            let wants_reply = info.announce == Some(true);
            let Some(dev) = shared.add_device(info, from.ip()) else {
                continue;
            };
            if !wants_reply {
                continue;
            }
            // One reply per device every two seconds, so a flapping or hostile announcer
            // cannot turn us into a request generator.
            let now = Instant::now();
            if let Some(t) = last_reply.get(dev.fingerprint())
                && now.duration_since(*t) < Duration::from_secs(2)
            {
                continue;
            }
            last_reply.insert(dev.fingerprint().to_string(), now);
            if last_reply.len() > 1024 {
                last_reply.retain(|_, t| now.duration_since(*t) < Duration::from_secs(60));
            }
            let shared = shared.clone();
            tokio::spawn(async move { reply(&shared, &dev).await });
        }
    });
    Ok(())
}

async fn reply(shared: &Arc<Shared>, dev: &Device) {
    let me = shared.info(None);
    if register_with(
        shared,
        dev.socket_addr(),
        dev.info.scheme_or_default(),
        Some(dev.fingerprint()),
        &me,
    )
    .await
    .is_err()
    {
        send_datagram(shared, &shared.info(Some(false)));
    }
}

/// Send one announcement (`announce: true`) on every LAN.
pub(crate) fn announce(shared: &Arc<Shared>) {
    send_datagram(shared, &shared.info(Some(true)));
}

fn send_datagram(shared: &Shared, info: &DeviceInfo) {
    let Ok(payload) = serde_json::to_vec(info) else {
        return;
    };
    let target = SocketAddr::from((shared.multicast_group, shared.multicast_port));
    for ip in &shared.lans {
        let _ = send_from(*ip, &payload, target);
    }
}

fn send_from(ip: Ipv4Addr, payload: &[u8], target: SocketAddr) -> io::Result<()> {
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    sock.bind(&SocketAddr::from((ip, 0)).into())?;
    sock.set_multicast_if_v4(&ip)?;
    sock.set_multicast_loop_v4(true)?;
    sock.set_multicast_ttl_v4(1)?;
    sock.send_to(payload, &target.into())?;
    Ok(())
}

/// `POST /register` to a device with our description. Returns the device's own description
/// from the response, which also registers it with us.
pub(crate) async fn register_with(
    shared: &Arc<Shared>,
    addr: SocketAddr,
    scheme: Scheme,
    expected_fp: Option<&str>,
    me: &DeviceInfo,
) -> Result<Device> {
    let mut conn = http::connect(addr, scheme, expected_fp, PROBE_TIMEOUT).await?;
    let body = http::full(serde_json::to_vec(me)?);
    let (code, bytes) = tokio::time::timeout(
        Duration::from_secs(3),
        conn.call(hyper::Method::POST, &format!("{API_PREFIX}/register"), body),
    )
    .await??;
    if code != 200 {
        return Err(Error::Remote {
            status: code,
            message: http::message_of(&bytes),
        });
    }
    let mut info: DeviceInfo = serde_json::from_slice(&bytes)?;
    // The spec's example response leaves these out: they are where we just connected.
    info.port.get_or_insert(addr.port());
    info.protocol.get_or_insert(scheme);
    // In HTTPS mode the certificate we just talked to is the identity. A device whose body
    // names a different (well formed) fingerprint is lying about who it is.
    if let Some(seen) = &conn.peer_fingerprint
        && crate::proto::is_fingerprint(&info.fingerprint)
        && !info.fingerprint.eq_ignore_ascii_case(seen)
    {
        return Err(Error::FingerprintMismatch);
    }
    shared
        .add_device(info, addr.ip())
        .ok_or_else(|| Error::Input("that is this device or speaks another protocol".into()))
}

async fn probe(shared: &Arc<Shared>, addr: SocketAddr) -> Option<Device> {
    let me = shared.info(None);
    match register_with(shared, addr, Scheme::Https, None, &me).await {
        Ok(d) => Some(d),
        Err(Error::Timeout) => None,
        Err(Error::Io(e))
            if matches!(
                e.kind(),
                io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::HostUnreachable
                    | io::ErrorKind::NetworkUnreachable
                    | io::ErrorKind::TimedOut
            ) =>
        {
            None
        }
        // Something answers but does not speak TLS: try plain HTTP.
        Err(_) => register_with(shared, addr, Scheme::Http, None, &me)
            .await
            .ok(),
    }
}

/// Probe each address.
pub(crate) async fn scan_addrs(shared: &Arc<Shared>, addrs: &[SocketAddr]) -> Vec<Device> {
    let sem = Arc::new(Semaphore::new(SCAN_CONCURRENCY));
    let mut tasks = Vec::new();
    for addr in addrs {
        let addr = *addr;
        let shared = shared.clone();
        let sem = sem.clone();
        tasks.push(tokio::spawn(async move {
            let _p = sem.acquire().await.ok()?;
            probe(&shared, addr).await
        }));
    }
    let mut found = Vec::new();
    for t in tasks {
        if let Ok(Some(d)) = t.await {
            found.push(d);
        }
    }
    found
}

/// Probe every host of every local subnet on our port.
pub(crate) async fn scan_lans(shared: &Arc<Shared>) -> Vec<Device> {
    let mut addrs = Vec::new();
    for lan in netif::lan_interfaces() {
        if !shared.lans.contains(&lan.ip) {
            continue;
        }
        addrs.extend(
            lan.scan_hosts()
                .into_iter()
                .map(|h| SocketAddr::new(IpAddr::V4(h), shared.scan_port)),
        );
    }
    // Also our own port when it differs, so two Panoply nodes on custom ports still meet.
    if shared.port != shared.scan_port {
        let own: Vec<SocketAddr> = addrs
            .iter()
            .map(|a| SocketAddr::new(a.ip(), shared.port))
            .collect();
        addrs.extend(own);
    }
    scan_addrs(shared, &addrs).await
}
