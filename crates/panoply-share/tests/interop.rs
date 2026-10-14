//! Interop with peers that are not panoply-share.
//!
//! There is no LocalSend app or headless LocalSend client that can run inside a test, so the
//! peers here are independent stand-ins: `curl` and `openssl` (different HTTP and TLS stacks)
//! and `tests/fixtures/mock_localsend.py`, a LocalSend v2 peer written from the public spec in
//! another language on OpenSSL. Tests skip with a message when a tool is missing.
//! The manual test against a real LocalSend app is in docs/specs (see the crate README).

mod common;

use std::process::Command;
use std::sync::{Arc, Mutex};

use common::*;
use panoply_share::{Decision, IncomingRequest, Node, ReceiveConfig, SendOptions, handler_fn};

macro_rules! need {
    ($cond:expr, $why:expr) => {
        if !$cond {
            eprintln!("SKIPPED: {}", $why);
            return;
        }
    };
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn certificate_fingerprint_is_the_sha256_of_the_der_as_openssl_sees_it() {
    need!(have("openssl"), "no openssl");
    let node = sender("Probe").await;
    let port = node.local_addr().port();
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "openssl s_client -connect 127.0.0.1:{port} </dev/null 2>/dev/null | openssl x509 -outform DER | openssl dgst -sha256 -r"
        ))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let hash = text.split_whitespace().next().unwrap_or("").to_string();
    assert_eq!(hash, node.fingerprint(), "openssl output: {text:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn curl_acts_as_a_stock_sender() {
    need!(have("curl"), "no curl");
    let dst = TempDir::new("dst");
    let work = TempDir::new("work");
    let seen: Arc<Mutex<Vec<(String, bool)>>> = Arc::default();
    let seen2 = seen.clone();
    let mut cfg = loopback_config("Receiver");
    cfg.receive = Some(ReceiveConfig::new(
        dst.path(),
        handler_fn(move |req: IncomingRequest| {
            seen2
                .lock()
                .unwrap()
                .push((req.sender.alias.clone(), req.verified));
            Box::pin(async { Decision::accept_all() })
        }),
    ));
    let rx = Node::start(cfg).await.unwrap();
    let base = format!(
        "https://127.0.0.1:{}/api/localsend/v2",
        rx.local_addr().port()
    );
    let curl = |args: &[&str]| -> String {
        let out = Command::new("curl")
            .args(["-sk", "--max-time", "20"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "curl {args:?}");
        String::from_utf8_lossy(&out.stdout).to_string()
    };

    let info: serde_json::Value = serde_json::from_str(&curl(&[&format!("{base}/info")])).unwrap();
    assert_eq!(info["alias"], "Receiver");
    assert_eq!(info["fingerprint"], rx.fingerprint());

    let data = blob(1_000_000, 5);
    std::fs::write(work.join("pic.bin"), &data).unwrap();
    let sha = sha256_file(&work.join("pic.bin"));
    let prepare = serde_json::json!({
        "info": {"alias": "Curl Phone", "version": "2.0", "deviceModel": "curl", "deviceType": "mobile",
                 "fingerprint": "random string", "port": 53317, "protocol": "https", "download": false},
        "files": {"id1": {"id": "id1", "fileName": "pic.bin", "size": 1_000_000, "fileType": "image/jpeg",
                          "sha256": sha, "metadata": {"modified": "2021-01-01T12:34:56Z"}}}
    });
    let resp: serde_json::Value = serde_json::from_str(&curl(&[
        "-X",
        "POST",
        "-H",
        "Content-Type: application/json",
        "-d",
        &prepare.to_string(),
        &format!("{base}/prepare-upload"),
    ]))
    .unwrap();
    let sid = resp["sessionId"].as_str().unwrap();
    let tok = resp["files"]["id1"].as_str().unwrap();
    let code = curl(&[
        "-X",
        "POST",
        "-o",
        if cfg!(windows) { "NUL" } else { "/dev/null" },
        "-w",
        "%{http_code}",
        "--data-binary",
        &format!("@{}", work.join("pic.bin").display()),
        &format!("{base}/upload?sessionId={sid}&fileId=id1&token={tok}"),
    ]);
    assert_eq!(code, "200");
    assert_eq!(sha256_file(&dst.join("pic.bin")), sha);
    // The modified time from the metadata was applied.
    let mtime = std::fs::metadata(dst.join("pic.bin"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        mtime
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        1_609_504_496
    );
    // curl runs no server, so its claimed identity cannot be verified.
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        [("Curl Phone".to_string(), false)]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn our_client_sends_to_an_independent_receiver() {
    let Some(py) = tls13_python() else {
        return eprintln!("SKIPPED: no python3 with TLS 1.3");
    };
    need!(have("openssl"), "no openssl");
    let work = TempDir::new("work");
    let out = TempDir::new("out");
    let (cert, key) = openssl_cert(work.path()).expect("make a certificate");
    let port = free_tcp_port();
    let mut mock = {
        let mut c = Command::new(&py);
        c.arg("-I")
            .arg(mock_script())
            .args(["receive", &port.to_string()]);
        c.arg(&cert).arg(&key).arg(out.path());
        c.env("MOCK_LOOPBACK", "1");
        Proc::spawn(c)
    };
    let ready = mock.expect("READY", 10);
    let mock_fp = ready.split_whitespace().nth(1).unwrap().to_string();

    let tx = sender("Panoply").await;
    let found = tx
        .scan_addrs(&["127.0.0.1:0"
            .replace(":0", &format!(":{port}"))
            .parse()
            .unwrap()])
        .await;
    assert_eq!(
        found.len(),
        1,
        "register against an OpenSSL server with a CA certificate"
    );
    let dev = found.into_iter().next().unwrap();
    assert_eq!(dev.alias(), "Mock Receiver");
    assert_eq!(
        dev.fingerprint(),
        mock_fp,
        "our SHA-256 of the DER equals the peer's"
    );

    let src = TempDir::new("src");
    std::fs::create_dir_all(src.join("pics")).unwrap();
    std::fs::write(src.join("pics/a.bin"), blob(2_500_000, 11)).unwrap();
    std::fs::write(src.join("note.txt"), "grüße").unwrap();
    let s = tx
        .send(
            dev,
            vec![src.join("pics"), src.join("note.txt")],
            SendOptions {
                hash_files: true,
                ..SendOptions::default()
            },
        )
        .wait()
        .await
        .unwrap();
    assert_eq!(s.files, 2);
    assert_eq!(
        sha256_file(&out.join("pics/a.bin")),
        sha256_file(&src.join("pics/a.bin"))
    );
    assert_eq!(
        std::fs::read_to_string(out.join("note.txt")).unwrap(),
        "grüße"
    );
    let prepare = mock.expect("PREPARE", 5);
    assert!(
        prepare.contains("Panoply") && prepare.contains("pics/a.bin"),
        "{prepare}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn an_independent_sender_sends_to_our_receiver() {
    let Some(py) = tls13_python() else {
        return eprintln!("SKIPPED: no python3 with TLS 1.3");
    };
    need!(have("openssl"), "no openssl");
    let work = TempDir::new("work");
    let dst = TempDir::new("dst");
    let (cert, key) = openssl_cert(work.path()).expect("make a certificate");
    let data = blob(3_000_000, 21);
    std::fs::write(work.join("from-phone.bin"), &data).unwrap();

    let verified: Arc<Mutex<Vec<bool>>> = Arc::default();
    let v2 = verified.clone();
    let mut cfg = loopback_config("Panoply Receiver");
    cfg.receive = Some(ReceiveConfig::new(
        dst.path(),
        handler_fn(move |req: IncomingRequest| {
            v2.lock().unwrap().push(req.verified);
            Box::pin(async { Decision::accept_all() })
        }),
    ));
    let rx = Node::start(cfg).await.unwrap();

    let mut c = Command::new(&py);
    c.arg("-I")
        .arg(mock_script())
        .args(["send", "127.0.0.1", &rx.local_addr().port().to_string()]);
    c.arg(work.join("from-phone.bin")).arg(&cert).arg(&key);
    let mut mock = Proc::spawn(c);
    assert_eq!(mock.expect("PREPARE_STATUS", 20), "PREPARE_STATUS 200");
    assert_eq!(mock.expect("UPLOAD_STATUS", 20), "UPLOAD_STATUS 200");
    assert_eq!(
        sha256_file(&dst.join("from-phone.bin")),
        sha256_file(&work.join("from-phone.bin"))
    );
    // The mock runs a TLS server whose certificate hashes to its claimed fingerprint, so the
    // connect-back check succeeds against an OpenSSL peer with a CA flagged certificate.
    assert_eq!(verified.lock().unwrap().as_slice(), [true]);
}
