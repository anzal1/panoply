//! Two Panoply nodes talking over loopback.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::*;
use panoply_share::{
    Decision, Error, ReceiveConfig, ReceiveMode, SendOptions, TransferKind, decline_all_handler,
    handler_fn,
};

#[tokio::test]
async fn sends_files_and_a_folder_with_identical_content() {
    let src = TempDir::new("src");
    let dst = TempDir::new("dst");
    std::fs::write(src.join("small.txt"), b"hello").unwrap();
    std::fs::write(
        src.join("naïve ファイル 🎉.bin"),
        blob(3 * 1024 * 1024 + 17, 1),
    )
    .unwrap();
    std::fs::create_dir_all(src.join("album/2024")).unwrap();
    std::fs::write(src.join("album/a.jpg"), blob(70_000, 2)).unwrap();
    std::fs::write(src.join("album/2024/b.jpg"), blob(5, 3)).unwrap();
    std::fs::write(src.join("album/empty.txt"), b"").unwrap();

    let rx = accepting_receiver("Receiver", dst.path()).await;
    let tx = sender("Sender").await;
    let dev = meet(&tx, &rx).await;
    assert_eq!(dev.alias(), "Receiver");

    let mut events = rx.subscribe();
    let summary = tx
        .send(
            dev,
            vec![
                src.join("small.txt"),
                src.join("naïve ファイル 🎉.bin"),
                src.join("album"),
            ],
            SendOptions {
                hash_files: true,
                ..SendOptions::default()
            },
        )
        .wait()
        .await
        .unwrap();
    assert_eq!(summary.files, 5);

    let t = wait_for(&mut events, is_finished).await;
    assert!(matches!(t.kind, TransferKind::Finished { files: 5, .. }));

    assert_eq!(
        tree(dst.path()),
        [
            "album/2024/b.jpg",
            "album/a.jpg",
            "album/empty.txt",
            "naïve ファイル 🎉.bin",
            "small.txt"
        ]
    );
    for (a, b) in [
        ("small.txt", "small.txt"),
        ("naïve ファイル 🎉.bin", "naïve ファイル 🎉.bin"),
        ("album/a.jpg", "album/a.jpg"),
        ("album/2024/b.jpg", "album/2024/b.jpg"),
    ] {
        assert_eq!(sha256_file(&src.join(a)), sha256_file(&dst.join(b)), "{a}");
    }
    // No temporary files left behind.
    assert!(
        tree(dst.path())
            .iter()
            .all(|f| !f.ends_with(".panoply-part"))
    );
}

#[tokio::test]
async fn never_overwrites_existing_files() {
    let src = TempDir::new("src");
    let dst = TempDir::new("dst");
    std::fs::write(src.join("a.txt"), b"new").unwrap();
    std::fs::write(dst.join("a.txt"), b"precious").unwrap();

    let rx = accepting_receiver("R", dst.path()).await;
    let tx = sender("S").await;
    let dev = meet(&tx, &rx).await;
    for _ in 0..2 {
        tx.send(dev.clone(), vec![src.join("a.txt")], SendOptions::default())
            .wait()
            .await
            .unwrap();
    }
    assert_eq!(std::fs::read(dst.join("a.txt")).unwrap(), b"precious");
    assert_eq!(std::fs::read(dst.join("a (1).txt")).unwrap(), b"new");
    assert_eq!(std::fs::read(dst.join("a (2).txt")).unwrap(), b"new");
}

#[tokio::test]
async fn decline_is_reported_and_leaves_nothing() {
    let src = TempDir::new("src");
    let dst = TempDir::new("dst");
    std::fs::write(src.join("a.txt"), b"x").unwrap();
    let rx = receiver("R", dst.path(), decline_all_handler()).await;
    let tx = sender("S").await;
    let dev = meet(&tx, &rx).await;
    let err = tx
        .send(dev, vec![src.join("a.txt")], SendOptions::default())
        .wait()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Rejected), "{err:?}");
    assert!(tree(dst.path()).is_empty());
}

#[tokio::test]
async fn a_node_that_is_not_receiving_declines() {
    let src = TempDir::new("src");
    std::fs::write(src.join("a.txt"), b"x").unwrap();
    let rx = sender("Quiet").await;
    let tx = sender("S").await;
    let dev = meet(&tx, &rx).await;
    let err = tx
        .send(dev, vec![src.join("a.txt")], SendOptions::default())
        .wait()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Rejected), "{err:?}");
}

#[tokio::test]
async fn partial_accept_sends_only_chosen_files() {
    let src = TempDir::new("src");
    let dst = TempDir::new("dst");
    std::fs::write(src.join("keep.txt"), b"k").unwrap();
    std::fs::write(src.join("skip.txt"), b"s").unwrap();
    let rx = receiver(
        "R",
        dst.path(),
        handler_fn(|req| {
            Box::pin(async move {
                let keep: Vec<String> = req
                    .files
                    .iter()
                    .filter(|f| f.name == "keep.txt")
                    .map(|f| f.id.clone())
                    .collect();
                Decision::Accept {
                    files: Some(keep),
                    trust: false,
                }
            })
        }),
    )
    .await;
    let tx = sender("S").await;
    let dev = meet(&tx, &rx).await;
    let s = tx
        .send(
            dev,
            vec![src.join("keep.txt"), src.join("skip.txt")],
            SendOptions::default(),
        )
        .wait()
        .await
        .unwrap();
    assert_eq!(s.files, 1);
    assert_eq!(tree(dst.path()), ["keep.txt"]);
}

#[tokio::test]
async fn text_messages_arrive_as_messages() {
    let dst = TempDir::new("dst");
    let rx = accepting_receiver("R", dst.path()).await;
    let tx = sender("S").await;
    let dev = meet(&tx, &rx).await;
    let mut events = rx.subscribe();
    let text = "line one\nzwei ünï 🎉";
    let s = tx
        .send_text(dev, text.to_string(), SendOptions::default())
        .wait()
        .await
        .unwrap();
    assert!(s.text_delivered);
    let ev = wait_for(&mut events, |t| matches!(t.kind, TransferKind::Text { .. })).await;
    let TransferKind::Text { text: got } = ev.kind else {
        unreachable!()
    };
    assert_eq!(got, text);
    assert!(
        tree(dst.path()).is_empty(),
        "a message is not saved as a file"
    );
}

#[tokio::test]
async fn pin_is_required_prompted_and_rate_limited() {
    let src = TempDir::new("src");
    let dst = TempDir::new("dst");
    std::fs::write(src.join("a.txt"), b"x").unwrap();
    let mut cfg = loopback_config("R");
    let mut rc = ReceiveConfig::new(dst.path(), panoply_share::accept_all_handler());
    rc.pin = Some("4711".into());
    cfg.receive = Some(rc);
    let rx = panoply_share::Node::start(cfg).await.unwrap();
    let tx = sender("S").await;
    let dev = meet(&tx, &rx).await;

    // No PIN and no way to ask for one.
    let err = tx
        .send(dev.clone(), vec![src.join("a.txt")], SendOptions::default())
        .wait()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::PinRequired), "{err:?}");

    // The callback supplies the right one on the second try.
    let asked = Arc::new(AtomicUsize::new(0));
    let asked2 = asked.clone();
    let opts = SendOptions {
        pin: Some("0000".into()),
        pin_provider: Some(Arc::new(move |_attempt| {
            asked2.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Some("4711".to_string()) })
        })),
        ..SendOptions::default()
    };
    tx.send(dev.clone(), vec![src.join("a.txt")], opts)
        .wait()
        .await
        .unwrap();
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(tree(dst.path()), ["a.txt"]);

    // Three wrong PINs lock the sender out, even for the right PIN.
    for _ in 0..3 {
        let o = SendOptions {
            pin: Some("9999".into()),
            ..SendOptions::default()
        };
        let _ = tx
            .send(dev.clone(), vec![src.join("a.txt")], o)
            .wait()
            .await;
    }
    let o = SendOptions {
        pin: Some("4711".into()),
        ..SendOptions::default()
    };
    let err = tx
        .send(dev, vec![src.join("a.txt")], o)
        .wait()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::TooManyRequests), "{err:?}");
}

#[tokio::test]
async fn trusted_devices_skip_the_prompt() {
    let src = TempDir::new("src");
    let dst = TempDir::new("dst");
    std::fs::write(src.join("a.txt"), b"x").unwrap();
    let prompts = Arc::new(AtomicUsize::new(0));
    let p2 = prompts.clone();
    let mut cfg = loopback_config("R");
    let mut rc = ReceiveConfig::new(
        dst.path(),
        handler_fn(move |req| {
            p2.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                assert!(req.verified, "the sender's certificate must check out");
                assert_eq!(req.check_code.split(' ').count(), 4);
                Decision::Accept {
                    files: None,
                    trust: true,
                }
            })
        }),
    );
    rc.mode = ReceiveMode::AutoAcceptTrusted;
    cfg.receive = Some(rc);
    let rx = panoply_share::Node::start(cfg).await.unwrap();
    let tx = sender("S").await;
    let dev = meet(&tx, &rx).await;
    for _ in 0..3 {
        tx.send(dev.clone(), vec![src.join("a.txt")], SendOptions::default())
            .wait()
            .await
            .unwrap();
    }
    assert_eq!(
        prompts.load(Ordering::SeqCst),
        1,
        "only first contact prompts"
    );
    assert!(rx.trust().is_trusted(tx.fingerprint()));
    assert_eq!(tree(dst.path()).len(), 3);
}

#[tokio::test]
async fn cancel_while_the_receiver_is_still_deciding() {
    let src = TempDir::new("src");
    let dst = TempDir::new("dst");
    std::fs::write(src.join("a.txt"), b"x").unwrap();
    let rx = receiver(
        "R",
        dst.path(),
        handler_fn(|_| {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Decision::Decline
            })
        }),
    )
    .await;
    let tx = sender("S").await;
    let dev = meet(&tx, &rx).await;
    let job = tx.send(dev, vec![src.join("a.txt")], SendOptions::default());
    tokio::time::sleep(Duration::from_millis(300)).await;
    job.cancel();
    let err = job.wait().await.unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err:?}");
}

#[tokio::test]
async fn a_second_request_during_a_prompt_is_busy() {
    let src = TempDir::new("src");
    let dst = TempDir::new("dst");
    std::fs::write(src.join("a.txt"), b"x").unwrap();
    let rx = receiver(
        "R",
        dst.path(),
        handler_fn(|_| {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(2)).await;
                Decision::accept_all()
            })
        }),
    )
    .await;
    let tx = sender("S").await;
    let dev = meet(&tx, &rx).await;
    let first = tx.send(dev.clone(), vec![src.join("a.txt")], SendOptions::default());
    tokio::time::sleep(Duration::from_millis(400)).await;
    let err = tx
        .send(dev, vec![src.join("a.txt")], SendOptions::default())
        .wait()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Busy), "{err:?}");
    first.wait().await.unwrap();
}

#[tokio::test]
async fn many_small_files_share_connections() {
    let src = TempDir::new("src");
    let dst = TempDir::new("dst");
    std::fs::create_dir_all(src.join("pile")).unwrap();
    for i in 0..300 {
        std::fs::write(src.join(&format!("pile/f{i:04}.txt")), format!("file {i}")).unwrap();
    }
    let rx = accepting_receiver("R", dst.path()).await;
    let tx = sender("S").await;
    let dev = meet(&tx, &rx).await;
    let s = tx
        .send(dev, vec![src.join("pile")], SendOptions::default())
        .wait()
        .await
        .unwrap();
    assert_eq!(s.files, 300);
    assert_eq!(tree(dst.path()).len(), 300);
    assert_eq!(
        std::fs::read_to_string(dst.join("pile/f0123.txt")).unwrap(),
        "file 123"
    );
}

#[tokio::test]
async fn pinned_connection_rejects_a_different_certificate() {
    let src = TempDir::new("src");
    let dst = TempDir::new("dst");
    std::fs::write(src.join("a.txt"), b"x").unwrap();
    let rx = accepting_receiver("R", dst.path()).await;
    let tx = sender("S").await;
    let mut dev = meet(&tx, &rx).await;
    // The device claims another identity than the certificate it presents.
    dev.info.fingerprint = "0".repeat(64);
    let err = tx
        .send(dev, vec![src.join("a.txt")], SendOptions::default())
        .wait()
        .await
        .unwrap_err();
    assert!(matches!(err, Error::FingerprintMismatch), "{err:?}");
    assert!(tree(dst.path()).is_empty());
}

#[tokio::test]
async fn plain_http_mode_works_end_to_end() {
    let src = TempDir::new("src");
    let dst = TempDir::new("dst");
    std::fs::write(src.join("a.txt"), b"plain").unwrap();
    let mut rc = loopback_config("R");
    rc.scheme = panoply_share::Scheme::Http;
    rc.receive = Some(ReceiveConfig::new(
        dst.path(),
        panoply_share::accept_all_handler(),
    ));
    let rx = panoply_share::Node::start(rc).await.unwrap();
    let mut sc = loopback_config("S");
    sc.scheme = panoply_share::Scheme::Http;
    let tx = panoply_share::Node::start(sc).await.unwrap();
    assert_ne!(
        rx.fingerprint().len(),
        64,
        "HTTP mode uses a random string, not a certificate hash"
    );
    let dev = meet(&tx, &rx).await;
    assert_eq!(dev.info.scheme_or_default(), panoply_share::Scheme::Http);
    tx.send(dev, vec![src.join("a.txt")], SendOptions::default())
        .wait()
        .await
        .unwrap();
    assert_eq!(std::fs::read(dst.join("a.txt")).unwrap(), b"plain");
}

#[tokio::test]
async fn receiver_can_cancel_an_upload_in_flight() {
    use panoply_share::http::{self, Conn};
    use panoply_share::proto::PrepareUploadResponse;

    let dst = TempDir::new("dst");
    let rx = accepting_receiver("R", dst.path()).await;
    let mut events = rx.subscribe();
    let mut c: Conn = http::connect(
        rx.local_addr(),
        panoply_share::Scheme::Https,
        None,
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    let prepare = serde_json::json!({
        "info": {"alias": "Raw", "version": "2.1", "fingerprint": "x", "port": 1, "protocol": "https"},
        "files": {"f": {"id": "f", "fileName": "big.bin", "size": 1000, "fileType": "x"}}
    });
    let (code, body) = c
        .call(
            hyper::Method::POST,
            "/api/localsend/v2/prepare-upload",
            http::full(serde_json::to_vec(&prepare).unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(code, 200);
    let r: PrepareUploadResponse = serde_json::from_slice(&body).unwrap();
    let path = format!(
        "/api/localsend/v2/upload?sessionId={}&fileId=f&token={}",
        r.session_id, r.files["f"]
    );

    // Send 100 of 1000 bytes, then stall; cancel from the receiving side.
    let (tx, stalled) = http::chan_body(Some(1000), 4);
    tx.send(Ok(bytes::Bytes::from(vec![9u8; 100])))
        .await
        .unwrap();
    let upload = c.call(hyper::Method::POST, &path, stalled);
    let rx2 = rx.clone();
    let sid = r.session_id.clone();
    let cancel = async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(rx2.cancel(&sid), "the session exists");
        tx // keep the body open until the receiver reacts
    };
    let (res, _tx) = tokio::join!(upload, cancel);
    // The receiver answers 409 and drops the connection; the client may see either.
    if let Ok((code, _)) = res {
        assert_eq!(code, 409);
    }
    assert!(tree(dst.path()).is_empty(), "the partial file is removed");
    wait_for(&mut events, |t| matches!(t.kind, TransferKind::Cancelled)).await;
    assert!(!rx.cancel(&r.session_id), "nothing left to cancel");

    // The receiver is free again.
    let mut c: Conn = http::connect(
        rx.local_addr(),
        panoply_share::Scheme::Https,
        None,
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    let (code, _) = c
        .call(
            hyper::Method::POST,
            "/api/localsend/v2/prepare-upload",
            http::full(serde_json::to_vec(&prepare).unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(code, 200);
}

/// Loopback throughput, run on demand: `cargo test -p panoply-share --release -- --ignored throughput --nocapture`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "a 1 GiB transfer, run on demand"]
async fn throughput() {
    let src = TempDir::new("src");
    let dst = TempDir::new("dst");
    let size: u64 = 1 << 30;
    {
        // A sparse file of zeros: reading it costs no disk, only the transfer is measured.
        let f = std::fs::File::create(src.join("big.bin")).unwrap();
        f.set_len(size).unwrap();
    }
    let rx = accepting_receiver("R", dst.path()).await;
    let tx = sender("S").await;
    let dev = meet(&tx, &rx).await;
    let t = std::time::Instant::now();
    tx.send(dev, vec![src.join("big.bin")], SendOptions::default())
        .wait()
        .await
        .unwrap();
    let secs = t.elapsed().as_secs_f64();
    println!(
        "1 GiB over loopback TLS: {:.2} s, {:.0} MB/s",
        secs,
        size as f64 / secs / 1e6
    );
    assert_eq!(std::fs::metadata(dst.join("big.bin")).unwrap().len(), size);
}

#[tokio::test]
async fn dropping_the_last_handle_stops_the_server() {
    let dst = TempDir::new("dst");
    let rx = accepting_receiver("R", dst.path()).await;
    let addr = rx.local_addr();
    let clone = rx.clone();
    assert!(tokio::net::TcpStream::connect(addr).await.is_ok());
    drop(rx);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        tokio::net::TcpStream::connect(addr).await.is_ok(),
        "a clone is still alive"
    );
    drop(clone);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        tokio::net::TcpStream::connect(addr).await.is_err(),
        "the port is closed"
    );
}
