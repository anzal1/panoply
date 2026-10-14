//! Hostile input: path traversal, links in the way, spoofed identities and request floods.

mod common;

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::*;
use panoply_share::http::{self, Conn};
use panoply_share::proto::PrepareUploadResponse;
use panoply_share::{
    Decision, IncomingRequest, Node, ReceiveConfig, ReceiveMode, Scheme, accept_all_handler,
    handler_fn,
};
use serde_json::json;

async fn conn(node: &Node) -> Conn {
    http::connect(
        node.local_addr(),
        Scheme::Https,
        None,
        Duration::from_secs(5),
    )
    .await
    .unwrap()
}

fn prepare_body(alias: &str, fingerprint: &str, port: u16, name: &str, size: usize) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "info": {
            "alias": alias, "version": "2.1", "deviceType": "desktop",
            "fingerprint": fingerprint, "port": port, "protocol": "https"
        },
        "files": { "f": { "id": "f", "fileName": name, "size": size, "fileType": "text/plain-ish" } }
    }))
    .unwrap()
}

async fn push(c: &mut Conn, name: &str, data: &[u8]) -> (u16, u16) {
    let (code, body) = c
        .call(
            hyper::Method::POST,
            "/api/localsend/v2/prepare-upload",
            http::full(prepare_body(
                "Mallory",
                &"a".repeat(64),
                1,
                name,
                data.len(),
            )),
        )
        .await
        .unwrap();
    if code != 200 {
        return (code, 0);
    }
    let r: PrepareUploadResponse = serde_json::from_slice(&body).unwrap();
    let (up, _) = c
        .call(
            hyper::Method::POST,
            &format!(
                "/api/localsend/v2/upload?sessionId={}&fileId=f&token={}",
                r.session_id, r.files["f"]
            ),
            http::full(data.to_vec()),
        )
        .await
        .unwrap();
    if up != 200 {
        // A sender that gives up cancels, which frees the receiver for the next request.
        let _ = c
            .call(
                hyper::Method::POST,
                &format!("/api/localsend/v2/cancel?sessionId={}", r.session_id),
                http::empty(),
            )
            .await;
    }
    (code, up)
}

fn lax_receiver_config(dir: &std::path::Path) -> panoply_share::NodeConfig {
    let mut cfg = loopback_config("R");
    let mut rc = ReceiveConfig::new(dir, accept_all_handler());
    rc.max_requests_per_minute = 10_000;
    cfg.receive = Some(rc);
    cfg
}

#[tokio::test]
async fn no_file_name_writes_outside_the_save_folder() {
    let outer = TempDir::new("outer");
    let dst = outer.join("dst");
    std::fs::create_dir_all(outer.join("outside")).unwrap();
    std::fs::write(outer.join("outside/canary.txt"), b"canary").unwrap();
    let rx = Node::start(lax_receiver_config(&dst)).await.unwrap();
    let mut c = conn(&rx).await;

    let mut hostile: Vec<String> = [
        "../x",
        "..\\x",
        "a/../x",
        "a/b/../../../x",
        "/etc/passwd",
        "\\\\server\\share\\x",
        "C:\\Windows\\x.txt",
        "..",
        ".",
        "",
        "/",
        "....//x",
        "a//b",
        "nul",
        "CON.txt",
        "x\u{0}y",
        "con:",
        "a\u{202e}txt.exe",
        "~/x",
        "$(touch pwned)",
        "%2e%2e/x",
        "..%2fx",
        ".hidden",
        " leading",
        "trailing.",
        "name with\nnewline",
        "a/./b",
        "../../outside/canary.txt",
        "outside/../../outside/new.txt",
        "...",
        "a/.../b",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    hostile.push("é".repeat(400));
    hostile.push(format!("{}/{}", "d".repeat(150), "f".repeat(150)));

    for name in &hostile {
        let (prepared, uploaded) = push(&mut c, name, b"abc").await;
        assert!(
            prepared == 400 || (prepared == 200 && uploaded == 200),
            "{name:?}: prepare {prepared}, upload {uploaded}"
        );
    }

    // Every file that exists is inside the save folder, and the canary is untouched.
    for f in tree(outer.path()) {
        assert!(
            f.starts_with("dst/") || f == "outside/canary.txt",
            "escaped the save folder: {f}"
        );
    }
    assert_eq!(
        std::fs::read(outer.join("outside/canary.txt")).unwrap(),
        b"canary"
    );
    assert!(tree(&dst).iter().all(|f| !f.ends_with(".panoply-part")));
    assert!(!tree(&dst).is_empty(), "the harmless names were accepted");
}

#[cfg(unix)]
#[tokio::test]
async fn links_in_the_save_folder_are_never_followed() {
    use std::os::unix::fs::symlink;
    let outer = TempDir::new("links");
    let dst = outer.join("dst");
    let outside = outer.join("outside");
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("victim.txt"), b"victim").unwrap();
    symlink(&outside, dst.join("linkdir")).unwrap();
    symlink(outside.join("victim.txt"), dst.join("linkfile.txt")).unwrap();
    symlink(outside.join("not-yet.txt"), dst.join("dangling.txt")).unwrap();

    let rx = Node::start(lax_receiver_config(&dst)).await.unwrap();
    let mut c = conn(&rx).await;

    // A folder link in the way: refused, nothing written through it.
    let (prepared, uploaded) = push(&mut c, "linkdir/pwn.txt", b"abc").await;
    assert_eq!(prepared, 200);
    assert_ne!(uploaded, 200);
    assert!(!outside.join("pwn.txt").exists());

    // A file link under the wanted name: the new file gets another name.
    for name in ["linkfile.txt", "dangling.txt"] {
        let (prepared, uploaded) = push(&mut c, name, b"new").await;
        assert_eq!((prepared, uploaded), (200, 200), "{name}");
    }
    assert_eq!(
        std::fs::read(outside.join("victim.txt")).unwrap(),
        b"victim"
    );
    assert!(!outside.join("not-yet.txt").exists());
    assert_eq!(std::fs::read(dst.join("linkfile (1).txt")).unwrap(), b"new");
    assert_eq!(std::fs::read(dst.join("dangling (1).txt")).unwrap(), b"new");
}

#[tokio::test]
async fn a_spoofed_trusted_fingerprint_is_not_auto_accepted() {
    let dst = TempDir::new("dst");
    let seen: Arc<Mutex<Vec<(bool, bool)>>> = Arc::default();
    let seen2 = seen.clone();
    let mut cfg = loopback_config("R");
    let mut rc = ReceiveConfig::new(
        dst.path(),
        handler_fn(move |req: IncomingRequest| {
            seen2.lock().unwrap().push((req.verified, req.trusted));
            Box::pin(async { Decision::Decline })
        }),
    );
    rc.mode = ReceiveMode::AutoAcceptTrusted;
    cfg.receive = Some(rc);
    let rx = Node::start(cfg).await.unwrap();

    // A real device is trusted.
    let real = sender("Real").await;
    rx.trust().trust(real.fingerprint(), "Real").unwrap();

    // An attacker claims the real device's fingerprint from a port where nothing answers.
    let mut c = conn(&rx).await;
    let (code, _) = c
        .call(
            hyper::Method::POST,
            "/api/localsend/v2/prepare-upload",
            http::full(prepare_body("Real", real.fingerprint(), 9, "evil.txt", 3)),
        )
        .await
        .unwrap();
    assert_eq!(
        code, 403,
        "declined by the prompt, not accepted automatically"
    );
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        [(false, true)],
        "asked, and flagged unverified"
    );
    assert!(tree(dst.path()).is_empty());
}

#[tokio::test]
async fn a_borrowed_alias_is_flagged() {
    let dst = TempDir::new("dst");
    let conflict = Arc::new(AtomicUsize::new(0));
    let c2 = conflict.clone();
    let mut cfg = loopback_config("R");
    cfg.receive = Some(ReceiveConfig::new(
        dst.path(),
        handler_fn(move |req: IncomingRequest| {
            if req.alias_conflict {
                c2.fetch_add(1, Ordering::SeqCst);
            }
            Box::pin(async { Decision::Decline })
        }),
    ));
    let rx = Node::start(cfg).await.unwrap();
    rx.trust().trust(&"b".repeat(64), "Pixel 9").unwrap();
    let mut c = conn(&rx).await;
    let _ = c
        .call(
            hyper::Method::POST,
            "/api/localsend/v2/prepare-upload",
            http::full(prepare_body("Pixel 9", &"c".repeat(64), 9, "a.txt", 1)),
        )
        .await
        .unwrap();
    assert_eq!(conflict.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn executables_are_marked_risky() {
    let dst = TempDir::new("dst");
    let risky = Arc::new(Mutex::new(Vec::new()));
    let r2 = risky.clone();
    let mut cfg = loopback_config("R");
    cfg.receive = Some(ReceiveConfig::new(
        dst.path(),
        handler_fn(move |req: IncomingRequest| {
            r2.lock()
                .unwrap()
                .extend(req.files.iter().map(|f| (f.name.clone(), f.risky)));
            Box::pin(async { Decision::Decline })
        }),
    ));
    let rx = Node::start(cfg).await.unwrap();
    let mut c = conn(&rx).await;
    for n in ["setup.exe", "photo.jpg"] {
        let _ = c
            .call(
                hyper::Method::POST,
                "/api/localsend/v2/prepare-upload",
                http::full(prepare_body("X", &"d".repeat(64), 9, n, 1)),
            )
            .await;
    }
    assert_eq!(
        risky.lock().unwrap().as_slice(),
        [
            ("setup.exe".to_string(), true),
            ("photo.jpg".to_string(), false)
        ]
    );
}

#[tokio::test]
async fn a_flood_of_requests_is_rate_limited() {
    let dst = TempDir::new("dst");
    let rx = receiver_decline_default(dst.path()).await;
    let mut c = conn(&rx).await;
    let mut limited = false;
    for _ in 0..45 {
        let (code, _) = c
            .call(
                hyper::Method::POST,
                "/api/localsend/v2/prepare-upload",
                http::full(prepare_body("Flood", &"e".repeat(64), 9, "a.txt", 1)),
            )
            .await
            .unwrap();
        if code == 429 {
            limited = true;
            break;
        }
    }
    assert!(
        limited,
        "the default limit is 30 requests a minute per address"
    );
}

async fn receiver_decline_default(dir: &std::path::Path) -> Node {
    let mut cfg = loopback_config("R");
    cfg.receive = Some(ReceiveConfig::new(
        dir,
        panoply_share::decline_all_handler(),
    ));
    Node::start(cfg).await.unwrap()
}

#[tokio::test]
async fn unanswered_prompts_lock_the_sender_out() {
    let dst = TempDir::new("dst");
    let mut cfg = loopback_config("R");
    let mut rc = ReceiveConfig::new(
        dst.path(),
        handler_fn(|_| {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Decision::Decline
            })
        }),
    );
    rc.request_timeout = Duration::from_millis(150);
    cfg.receive = Some(rc);
    let rx = Node::start(cfg).await.unwrap();
    let mut c = conn(&rx).await;
    let mut codes = Vec::new();
    for _ in 0..4 {
        let (code, _) = c
            .call(
                hyper::Method::POST,
                "/api/localsend/v2/prepare-upload",
                http::full(prepare_body("Pest", &"f".repeat(64), 9, "a.txt", 1)),
            )
            .await
            .unwrap();
        codes.push(code);
    }
    assert_eq!(codes, [403, 403, 403, 429]);
}

#[tokio::test]
async fn oversized_and_extra_data_is_refused() {
    let dst = TempDir::new("dst");
    let rx = Node::start(lax_receiver_config(dst.path())).await.unwrap();
    let mut c = conn(&rx).await;
    // Declares 3 bytes, sends 100.
    let (code, body) = c
        .call(
            hyper::Method::POST,
            "/api/localsend/v2/prepare-upload",
            http::full(prepare_body("M", &"a".repeat(64), 1, "a.txt", 3)),
        )
        .await
        .unwrap();
    assert_eq!(code, 200);
    let r: PrepareUploadResponse = serde_json::from_slice(&body).unwrap();
    let (up, _) = c
        .call(
            hyper::Method::POST,
            &format!(
                "/api/localsend/v2/upload?sessionId={}&fileId=f&token={}",
                r.session_id, r.files["f"]
            ),
            http::full(vec![7u8; 100]),
        )
        .await
        .unwrap_or((0, Default::default()));
    assert!(up == 400 || up == 0, "got {up}");
    assert!(tree(dst.path()).is_empty());
}

/// A tiny deterministic generator for the fuzz inputs.
fn next(x: &mut u64) -> u64 {
    *x ^= *x << 13;
    *x ^= *x >> 7;
    *x ^= *x << 17;
    *x
}

#[tokio::test]
async fn fuzzed_bodies_never_crash_the_server_or_write_files() {
    let dst = TempDir::new("dst");
    let rx = receiver_decline_lax(dst.path()).await;
    let mut seed = 0x1234_5678_9abc_def1u64;
    let valid = prepare_body("Fuzz", &"a".repeat(64), 1, "a.txt", 3);
    for i in 0..300 {
        let mut body = valid.clone();
        match next(&mut seed) % 5 {
            0 => {
                // Random bytes.
                body = (0..(next(&mut seed) % 400))
                    .map(|_| next(&mut seed) as u8)
                    .collect();
            }
            1 => {
                // Flip some bytes of a valid body.
                for _ in 0..(1 + next(&mut seed) % 4) {
                    let at = (next(&mut seed) as usize) % body.len();
                    body[at] = next(&mut seed) as u8;
                }
            }
            2 => body.truncate((next(&mut seed) as usize) % body.len()),
            3 => {
                // Hostile numbers and ids.
                let size = ["-1", "18446744073709551615", "1e999", "null", "\"3\""]
                    [(next(&mut seed) % 5) as usize];
                body = format!(
                    "{{\"info\":{{\"alias\":\"x\",\"version\":\"2.1\",\"fingerprint\":\"f\"}},\"files\":{{\"f\":{{\"id\":\"f\",\"fileName\":\"a\",\"size\":{size},\"fileType\":\"t\"}}}}}}"
                )
                .into_bytes();
            }
            _ => {
                body = br#"{"info":{"alias":"x","version":"9.0","fingerprint":"f"},"files":{}}"#
                    .to_vec();
            }
        }
        // A fresh connection each time: a malformed request may close the previous one.
        let mut c = conn(&rx).await;
        let r = c
            .call(
                hyper::Method::POST,
                "/api/localsend/v2/prepare-upload",
                http::full(body),
            )
            .await;
        if let Ok((code, _)) = r {
            assert!(matches!(code, 400 | 403 | 429), "iteration {i}: {code}");
        }
    }
    // Still alive and answering.
    let mut c = conn(&rx).await;
    let (code, _) = c
        .call(hyper::Method::GET, "/api/localsend/v2/info", http::empty())
        .await
        .unwrap();
    assert_eq!(code, 200);
    assert!(tree(dst.path()).is_empty());
}

async fn receiver_decline_lax(dir: &std::path::Path) -> Node {
    let mut cfg = loopback_config("R");
    let mut rc = ReceiveConfig::new(dir, panoply_share::decline_all_handler());
    rc.max_requests_per_minute = 100_000;
    cfg.receive = Some(rc);
    Node::start(cfg).await.unwrap()
}

#[tokio::test]
async fn a_request_larger_than_the_disk_is_refused_up_front() {
    let dst = TempDir::new("dst");
    let rx = Node::start(lax_receiver_config(dst.path())).await.unwrap();
    let mut c = conn(&rx).await;
    let (code, body) = c
        .call(
            hyper::Method::POST,
            "/api/localsend/v2/prepare-upload",
            http::full(prepare_body("M", &"a".repeat(64), 1, "huge.bin", 1 << 62)),
        )
        .await
        .unwrap();
    assert_eq!(code, 500);
    assert!(String::from_utf8_lossy(&body).contains("not enough disk space"));
    // No session was left behind.
    let (code, _) = c
        .call(
            hyper::Method::POST,
            "/api/localsend/v2/prepare-upload",
            http::full(prepare_body("M", &"a".repeat(64), 1, "ok.txt", 3)),
        )
        .await
        .unwrap();
    assert_eq!(code, 200);
}
