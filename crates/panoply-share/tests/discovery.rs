//! Discovery on the local subnet. Uses the machine's private LAN address and a private port, so
//! it never talks to a real LocalSend app, and skips when there is no LAN interface.

mod common;

use std::net::{IpAddr, Ipv4Addr};
use std::process::Command;
use std::time::{Duration, Instant};

use common::*;
use panoply_share::netif::lan_interfaces;
use panoply_share::{Event, Node, NodeConfig, proto::DeviceType};

fn lan_ip() -> Option<Ipv4Addr> {
    lan_interfaces().first().map(|l| l.ip)
}

fn lan_config(alias: &str, mport: u16) -> NodeConfig {
    let mut c = NodeConfig::new(alias);
    c.port = 0;
    c.multicast_port = mport;
    c.scan_port = mport;
    c.discovery = true;
    c.device_type = DeviceType::Mobile;
    c
}

/// True when this machine can send to the multicast group on that LAN. Hosted CI VMs (the
/// macOS runners) answer "No route to host" for any multicast send, which no code can fix, so
/// the multicast tests say so and stand down there. The HTTP scan test needs no multicast.
fn multicast_works(ip: Ipv4Addr) -> bool {
    use socket2::{Domain, Protocol, Socket, Type};
    let Ok(sock) = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)) else {
        return false;
    };
    let target = std::net::SocketAddr::from((Ipv4Addr::new(224, 0, 0, 167), free_udp_port()));
    sock.bind(&std::net::SocketAddr::from((ip, 0)).into())
        .is_ok()
        && sock.set_multicast_if_v4(&ip).is_ok()
        && sock.send_to(b"probe", &target.into()).is_ok()
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(8);
    while Instant::now() < end {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn two_nodes_find_each_other_by_multicast_without_polling() {
    let Some(ip) = lan_ip() else {
        return eprintln!("SKIPPED: no private LAN interface");
    };
    if !multicast_works(ip) {
        return eprintln!("SKIPPED: this machine cannot send multicast on {ip}");
    }
    let mport = free_udp_port();
    let a = Node::start(lan_config("Alpha", mport)).await.unwrap();
    let mut events = a.subscribe();
    let b = Node::start(lan_config("Beta", mport)).await.unwrap();

    // Beta announced when it started. Alpha hears it, registers back, and both know each other.
    until("alpha to list beta", || {
        a.devices().iter().any(|d| d.alias() == "Beta")
    })
    .await;
    until("beta to list alpha", || {
        b.devices().iter().any(|d| d.alias() == "Alpha")
    })
    .await;
    let found = a
        .devices()
        .into_iter()
        .find(|d| d.alias() == "Beta")
        .unwrap();
    assert_eq!(found.fingerprint(), b.fingerprint());
    assert_eq!(found.info.port, Some(b.local_addr().port()));
    assert_eq!(found.info.kind(), DeviceType::Mobile);
    assert!(matches!(found.addr, IpAddr::V4(v) if v == ip || v.is_private()));
    // The event stream saw it too.
    let mut got = false;
    while let Ok(ev) = events.try_recv() {
        got |= matches!(ev, Event::DeviceFound(d) if d.alias() == "Beta");
    }
    assert!(got);
    // No device lists itself.
    assert!(
        a.devices()
            .iter()
            .all(|d| d.fingerprint() != a.fingerprint())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn http_scan_finds_a_device_when_multicast_is_off() {
    let Some(_) = lan_ip() else {
        return eprintln!("SKIPPED: no private LAN interface");
    };
    let mport = free_udp_port();
    let mut ca = lan_config("Quiet", mport);
    ca.discovery = false; // never announces, never listens
    ca.port = free_tcp_port();
    let a = Node::start(ca).await.unwrap();

    let mut cb = lan_config("Scanner", mport);
    cb.discovery = false;
    cb.scan_port = a.local_addr().port();
    let b = Node::start(cb).await.unwrap();
    assert!(b.devices().is_empty());
    let t = Instant::now();
    let found = b.scan().await;
    assert!(found.iter().any(|d| d.alias() == "Quiet"), "{found:?}");
    assert!(
        t.elapsed() < Duration::from_secs(10),
        "a /24 sweep took {:?}",
        t.elapsed()
    );
    // The scanned device learned about the scanner through the same register call.
    assert!(a.devices().iter().any(|d| d.alias() == "Scanner"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn an_independent_peer_announcing_on_the_group_is_found_and_answered() {
    let Some(ip) = lan_ip() else {
        return eprintln!("SKIPPED: no private LAN interface");
    };
    if !multicast_works(ip) {
        return eprintln!("SKIPPED: this machine cannot send multicast on {ip}");
    }
    let Some(py) = tls13_python() else {
        return eprintln!("SKIPPED: no python3 with TLS 1.3");
    };
    if !have("openssl") {
        return eprintln!("SKIPPED: no openssl");
    }
    let work = TempDir::new("work");
    let (cert, key) = openssl_cert(work.path()).unwrap();
    let mport = free_udp_port();
    let sport = free_tcp_port();
    let node = Node::start(lan_config("Panoply", mport)).await.unwrap();

    let mut c = Command::new(&py);
    c.arg("-I")
        .arg(mock_script())
        .args(["announce", &mport.to_string(), &sport.to_string()]);
    c.arg(&cert).arg(&key).arg(ip.to_string());
    let mut mock = Proc::spawn(c);
    mock.expect("ANNOUNCED", 10);
    // We heard the multicast announcement from the other implementation...
    until("the mock announcer to be listed", || {
        node.devices().iter().any(|d| d.alias() == "Mock Announcer")
    })
    .await;
    // ...and answered it the way the spec says: POST /register to its server.
    let reg = mock.expect("REGISTER", 10);
    assert!(
        reg.contains("Panoply") && reg.contains(node.fingerprint()),
        "{reg}"
    );
}
