//! The QR browser fallback, driven the way a phone browser drives it.

mod common;

use std::net::SocketAddr;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use common::*;
use http_body_util::BodyExt;
use panoply_share::http::{self, Conn};
use panoply_share::qr::{QrEvent, QrOptions, QrOutcome, QrShare, terminal_qr};
use panoply_share::{Identity, Scheme};

struct Reply {
    code: u16,
    headers: hyper::HeaderMap,
    body: Bytes,
}

impl Reply {
    fn header(&self, k: &str) -> &str {
        self.headers
            .get(k)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    }
}

fn addr_of(share: &QrShare) -> (SocketAddr, String) {
    let url = share.url().to_string();
    let rest = url.split("://").nth(1).unwrap();
    let (hostport, path) = rest.split_once('/').unwrap();
    (hostport.parse().unwrap(), format!("/{path}"))
}

async fn connect(share: &QrShare) -> Conn {
    let (addr, _) = addr_of(share);
    http::connect(addr, Scheme::Http, None, Duration::from_secs(5))
        .await
        .unwrap()
}

async fn get(c: &mut Conn, path: &str, headers: &[(&str, &str)]) -> Reply {
    request(c, hyper::Method::GET, path, headers, http::empty()).await
}

async fn request(
    c: &mut Conn,
    method: hyper::Method,
    path: &str,
    headers: &[(&str, &str)],
    body: http::Body,
) -> Reply {
    let resp = c.send_with(method, path, headers, body).await.unwrap();
    let (parts, body) = resp.into_parts();
    let body = body.collect().await.unwrap().to_bytes();
    Reply {
        code: parts.status.as_u16(),
        headers: parts.headers,
        body,
    }
}

fn local(files: Vec<std::path::PathBuf>) -> QrOptions {
    QrOptions {
        files,
        bind: "127.0.0.1".parse().unwrap(),
        ..QrOptions::default()
    }
}

#[tokio::test]
async fn the_page_lists_files_and_hides_everything_else() {
    let src = TempDir::new("src");
    std::fs::write(src.join("a.txt"), b"alpha").unwrap();
    std::fs::write(src.join("b <&>.txt"), b"bravo").unwrap();
    let share = QrShare::start(local(vec![src.join("a.txt"), src.join("b <&>.txt")]))
        .await
        .unwrap();
    let (_, path) = addr_of(&share);
    assert_eq!(share.token().len(), 32, "128 bit token");
    assert!(share.url().starts_with("http://127.0.0.1:"));
    let mut events = share.subscribe();
    let mut c = connect(&share).await;
    let page = get(&mut c, &path, &[("user-agent", "Mozilla/5.0 (iPhone)")]).await;
    assert_eq!(page.code, 200);
    let text = String::from_utf8(page.body.to_vec()).unwrap();
    assert!(
        text.contains("a.txt") && text.contains("b &lt;&amp;&gt;.txt"),
        "escaped names"
    );
    assert!(!text.contains("<script"), "works without JavaScript");
    assert!(text.len() < 20 * 1024);
    assert!(
        !text.contains("src=\"http") && !text.contains("href=\"http"),
        "no external requests"
    );
    assert!(
        page.header("content-security-policy")
            .contains("default-src 'none'")
    );
    assert_eq!(page.header("cache-control"), "no-store");
    assert!(text.contains("Download all as zip"));

    // A wrong token and a wrong path look the same.
    let wrong = get(&mut c, &format!("{}x", path), &[]).await;
    let root = get(&mut c, "/", &[]).await;
    let nothing = get(&mut c, &format!("{path}/nothing"), &[]).await;
    assert_eq!((wrong.code, root.code, nothing.code), (404, 404, 404));
    assert_eq!(wrong.body, root.body);
    assert_eq!(wrong.body, nothing.body);

    match events.try_recv() {
        Ok(QrEvent::Connected { user_agent, .. }) => assert!(user_agent.contains("iPhone")),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn downloads_resume_with_ranges_and_close_after_completion() {
    let src = TempDir::new("src");
    let data = blob(1_500_000, 7);
    std::fs::write(src.join("big.bin"), &data).unwrap();
    let share = QrShare::start(local(vec![src.join("big.bin")]))
        .await
        .unwrap();
    let (addr, path) = addr_of(&share);
    let mut c = connect(&share).await;

    let head = request(
        &mut c,
        hyper::Method::HEAD,
        &format!("{path}/d/0"),
        &[],
        http::empty(),
    )
    .await;
    assert_eq!(head.code, 200);
    assert_eq!(head.header("content-length"), "1500000");
    assert_eq!(head.header("accept-ranges"), "bytes");
    assert!(
        head.header("content-disposition")
            .contains("filename=\"big.bin\"")
    );

    // A dropped connection leaves the first 400,000 bytes; the browser asks for the rest.
    let first = get(
        &mut c,
        &format!("{path}/d/0"),
        &[("range", "bytes=0-399999")],
    )
    .await;
    assert_eq!(first.code, 206);
    assert_eq!(first.header("content-range"), "bytes 0-399999/1500000");
    assert_eq!(&first.body[..], &data[..400_000]);
    assert!(
        tokio::time::timeout(Duration::from_millis(300), share.wait())
            .await
            .is_err(),
        "a partial download does not complete the share"
    );
    let bad = get(
        &mut c,
        &format!("{path}/d/0"),
        &[("range", "bytes=9999999-")],
    )
    .await;
    assert_eq!(bad.code, 416);
    let etag = head.header("etag").to_string();
    let rest = get(
        &mut c,
        &format!("{path}/d/0"),
        &[("range", "bytes=400000-"), ("if-range", &etag)],
    )
    .await;
    assert_eq!(rest.code, 206);
    let mut joined = first.body.to_vec();
    joined.extend_from_slice(&rest.body);
    assert_eq!(joined, data);

    // The resumed download reached the end: the share completed and the port closes.
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), share.wait())
            .await
            .unwrap(),
        QrOutcome::Completed
    );
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(
        tokio::net::TcpStream::connect(addr).await.is_err(),
        "the port is closed within about a second of completion"
    );
}

#[tokio::test]
async fn a_stale_validator_gets_the_whole_file() {
    let src = TempDir::new("src");
    std::fs::write(src.join("a.bin"), blob(5000, 4)).unwrap();
    let share = QrShare::start(local(vec![src.join("a.bin")]))
        .await
        .unwrap();
    let (_, path) = addr_of(&share);
    let mut c = connect(&share).await;
    // The browser holds a validator for an older version: it must not splice two versions.
    let full = get(
        &mut c,
        &format!("{path}/d/0"),
        &[("range", "bytes=10-"), ("if-range", "\"stale\"")],
    )
    .await;
    assert_eq!((full.code, full.body.len()), (200, 5000));
}

#[tokio::test]
async fn zip_of_many_files_and_folders_streams_with_exact_length() {
    let src = TempDir::new("src");
    std::fs::create_dir_all(src.join("trip/day 1")).unwrap();
    std::fs::write(src.join("trip/day 1/a.jpg"), blob(300_000, 1)).unwrap();
    std::fs::write(src.join("trip/b.txt"), "grüße 🎉").unwrap();
    std::fs::write(src.join("empty.dat"), b"").unwrap();
    std::fs::write(src.join("solo.bin"), blob(1_000_001, 2)).unwrap();
    for force in [false, true] {
        let mut opts = local(vec![
            src.join("trip"),
            src.join("empty.dat"),
            src.join("solo.bin"),
        ]);
        opts.force_zip64 = force;
        let share = QrShare::start(opts).await.unwrap();
        let (_, path) = addr_of(&share);
        let mut c = connect(&share).await;
        let z = get(&mut c, &format!("{path}/zip"), &[]).await;
        assert_eq!(z.code, 200);
        assert_eq!(z.header("content-type"), "application/zip");
        assert_eq!(
            z.header("content-length").parse::<usize>().unwrap(),
            z.body.len(),
            "Content-Length is exact (zip64 = {force})"
        );
        let work = TempDir::new("zip");
        let zip_path = work.join("got.zip");
        std::fs::write(&zip_path, &z.body).unwrap();

        if have("unzip") {
            let out = Command::new("unzip")
                .arg("-t")
                .arg(&zip_path)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "unzip -t (zip64 = {force}): {}",
                String::from_utf8_lossy(&out.stdout)
            );
            let x = Command::new("unzip")
                .arg("-q")
                .arg(&zip_path)
                .arg("-d")
                .arg(work.join("x"))
                .output()
                .unwrap();
            assert!(x.status.success());
            for f in ["trip/day 1/a.jpg", "trip/b.txt", "empty.dat", "solo.bin"] {
                assert_eq!(
                    sha256_file(&work.join("x").join(f)),
                    sha256_file(&src.join(f)),
                    "{f} (zip64 = {force})"
                );
            }
        } else {
            eprintln!("SKIPPED unzip check: no unzip");
        }
        if let Some(py) = tls13_python().or_else(|| have("python3").then(|| "python3".to_string()))
        {
            let script = "import sys,zipfile;z=zipfile.ZipFile(sys.argv[1]);bad=z.testzip();names=sorted(z.namelist());print(bad);print(names)";
            let out = Command::new(py)
                .args(["-I", "-c", script])
                .arg(&zip_path)
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stdout).to_string();
            assert!(
                text.starts_with("None\n"),
                "python zipfile: {text} {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(
                text.contains("trip/day 1/a.jpg") && text.contains("empty.dat"),
                "{text}"
            );
        }
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), share.wait())
                .await
                .unwrap(),
            QrOutcome::Completed
        );
    }
}

#[tokio::test]
async fn expires_and_can_be_revoked() {
    let src = TempDir::new("src");
    std::fs::write(src.join("a.txt"), b"x").unwrap();
    let mut o = local(vec![src.join("a.txt")]);
    o.expires = Duration::from_millis(300);
    let share = QrShare::start(o).await.unwrap();
    let (addr, _) = addr_of(&share);
    assert_eq!(share.wait().await, QrOutcome::Expired);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());

    let share = QrShare::start(local(vec![src.join("a.txt")]))
        .await
        .unwrap();
    let (addr, _) = addr_of(&share);
    share.revoke();
    assert_eq!(share.wait().await, QrOutcome::Revoked);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());

    // Dropping the handle revokes too.
    let share = QrShare::start(local(vec![src.join("a.txt")]))
        .await
        .unwrap();
    let (addr, _) = addr_of(&share);
    drop(share);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());
}

#[tokio::test]
async fn pin_gates_everything_and_is_rate_limited() {
    let src = TempDir::new("src");
    std::fs::write(src.join("a.txt"), b"secret").unwrap();
    let mut o = local(vec![src.join("a.txt")]);
    o.pin = Some("2468".into());
    let share = QrShare::start(o).await.unwrap();
    let (_, path) = addr_of(&share);
    let mut c = connect(&share).await;

    let page = get(&mut c, &path, &[]).await;
    let text = String::from_utf8_lossy(&page.body).to_string();
    assert!(
        text.contains("name=pin") && !text.contains("a.txt"),
        "the page shows only the PIN form"
    );
    assert_eq!(get(&mut c, &format!("{path}/d/0"), &[]).await.code, 403);

    let form = |pin: &str| http::full(format!("pin={pin}"));
    let post = hyper::Method::POST;
    for _ in 0..5 {
        let r = request(
            &mut c,
            post.clone(),
            &format!("{path}/pin"),
            &[("content-type", "application/x-www-form-urlencoded")],
            form("0000"),
        )
        .await;
        assert_eq!(r.code, 401);
    }
    let locked = request(
        &mut c,
        post.clone(),
        &format!("{path}/pin"),
        &[],
        form("2468"),
    )
    .await;
    assert_eq!(
        locked.code, 429,
        "even the right PIN is refused during the lockout"
    );
    assert_eq!(get(&mut c, &format!("{path}/d/0"), &[]).await.code, 403);

    // A fresh share, right PIN first: the cookie unlocks the page and the download.
    let mut o = local(vec![src.join("a.txt")]);
    o.pin = Some("2468".into());
    let share = QrShare::start(o).await.unwrap();
    let (_, path) = addr_of(&share);
    let mut c = connect(&share).await;
    let ok = request(&mut c, post, &format!("{path}/pin"), &[], form("2468")).await;
    assert_eq!(ok.code, 303);
    let cookie = ok
        .header("set-cookie")
        .split(';')
        .next()
        .unwrap()
        .to_string();
    assert!(ok.header("set-cookie").contains("HttpOnly"));
    let page = get(&mut c, &path, &[("cookie", &cookie)]).await;
    assert!(String::from_utf8_lossy(&page.body).contains("a.txt"));
    let dl = get(&mut c, &format!("{path}/d/0"), &[("cookie", &cookie)]).await;
    assert_eq!((dl.code, &dl.body[..]), (200, &b"secret"[..]));
}

fn multipart(boundary: &str, parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut b = Vec::new();
    for (name, data) in parts {
        b.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"f\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes(),
        );
        b.extend_from_slice(data);
        b.extend_from_slice(b"\r\n");
    }
    b.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    b
}

#[tokio::test]
async fn upload_saves_into_the_folder_and_never_overwrites() {
    let inbox = TempDir::new("inbox");
    std::fs::write(inbox.join("photo.jpg"), b"already here").unwrap();
    let big = blob(8_000_000, 3);
    let share = QrShare::start(QrOptions {
        upload_dir: Some(inbox.0.clone()),
        bind: "127.0.0.1".parse().unwrap(),
        ..QrOptions::default()
    })
    .await
    .unwrap();
    let (_, path) = addr_of(&share);
    let mut events = share.subscribe();
    let mut c = connect(&share).await;
    let page = get(&mut c, &path, &[]).await;
    let text = String::from_utf8_lossy(&page.body).to_string();
    assert!(text.contains("enctype=\"multipart/form-data\"") && text.contains("type=file"));

    let body = multipart(
        "XyZ123",
        &[
            ("photo.jpg", b"new photo"),
            ("../../evil.txt", b"traversal"),
            ("C:\\Users\\x\\big.bin", &big),
            ("emoji 🎉.txt", "tänk".as_bytes()),
        ],
    );
    let r = request(
        &mut c,
        hyper::Method::POST,
        &format!("{path}/upload"),
        &[("content-type", "multipart/form-data; boundary=XyZ123")],
        http::full(body),
    )
    .await;
    assert_eq!(r.code, 200, "{}", String::from_utf8_lossy(&r.body));
    assert_eq!(
        std::fs::read(inbox.join("photo.jpg")).unwrap(),
        b"already here"
    );
    assert_eq!(
        std::fs::read(inbox.join("photo (1).jpg")).unwrap(),
        b"new photo"
    );
    assert_eq!(std::fs::read(inbox.join("evil.txt")).unwrap(), b"traversal");
    assert_eq!(sha256_file(&inbox.join("big.bin")), {
        let t = TempDir::new("h");
        std::fs::write(t.join("x"), &big).unwrap();
        sha256_file(&t.join("x"))
    });
    assert_eq!(
        std::fs::read_to_string(inbox.join("emoji 🎉.txt")).unwrap(),
        "tänk"
    );
    assert!(
        tree(inbox.path())
            .iter()
            .all(|f| !f.ends_with(".panoply-part"))
    );
    assert!(!inbox.path().parent().unwrap().join("evil.txt").exists());

    let mut uploaded = 0;
    while let Ok(ev) = events.try_recv() {
        if matches!(ev, QrEvent::Uploaded { .. }) {
            uploaded += 1;
        }
    }
    assert_eq!(uploaded, 4);
    assert_eq!(share.wait().await, QrOutcome::Completed);
}

#[tokio::test]
async fn upload_limits_and_broken_bodies() {
    let inbox = TempDir::new("inbox");
    let share = QrShare::start(QrOptions {
        upload_dir: Some(inbox.0.clone()),
        bind: "127.0.0.1".parse().unwrap(),
        max_upload_bytes: 1000,
        ..QrOptions::default()
    })
    .await
    .unwrap();
    let (_, path) = addr_of(&share);
    let mut c = connect(&share).await;
    let ct = [("content-type", "multipart/form-data; boundary=B")];
    let big = multipart("B", &[("a.bin", &[1u8; 5000])]);
    let r = request(
        &mut c,
        hyper::Method::POST,
        &format!("{path}/upload"),
        &ct,
        http::full(big),
    )
    .await;
    assert_eq!(r.code, 413);
    // Cut off in the middle of a file: nothing is kept.
    let mut cut = multipart("B", &[("a.bin", &[1u8; 500])]);
    cut.truncate(cut.len() - 40);
    let r = request(
        &mut c,
        hyper::Method::POST,
        &format!("{path}/upload"),
        &ct,
        http::full(cut),
    )
    .await;
    assert_eq!(r.code, 400);
    // No multipart type at all.
    let r = request(
        &mut c,
        hyper::Method::POST,
        &format!("{path}/upload"),
        &[],
        http::full("x"),
    )
    .await;
    assert_eq!(r.code, 400);
    assert!(tree(inbox.path()).is_empty(), "no partial files");
}

#[tokio::test]
async fn https_mode_serves_the_same_page() {
    let src = TempDir::new("src");
    std::fs::write(src.join("a.txt"), b"over tls").unwrap();
    let id = Arc::new(Identity::generate().unwrap());
    let mut o = local(vec![src.join("a.txt")]);
    o.tls = Some(id.clone());
    let share = QrShare::start(o).await.unwrap();
    assert!(share.url().starts_with("https://"));
    let (addr, path) = addr_of(&share);
    let mut c = http::connect(
        addr,
        Scheme::Https,
        Some(id.fingerprint()),
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    let dl = get(&mut c, &format!("{path}/d/0"), &[]).await;
    assert_eq!((dl.code, &dl.body[..]), (200, &b"over tls"[..]));
    assert_eq!(share.wait().await, QrOutcome::Completed);
}

#[test]
fn terminal_qr_round_trips_to_the_same_modules() {
    use qrcode::{Color, EcLevel, QrCode};
    let url = "http://192.168.1.20:53318/0123456789abcdef0123456789abcdef";
    let code = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::L).unwrap();
    let w = code.width();
    let quiet = 4;
    let side = w + 2 * quiet;
    let dark_at = |x: usize, y: usize| -> bool {
        x >= quiet
            && y >= quiet
            && x < quiet + w
            && y < quiet + w
            && code[(x - quiet, y - quiet)] == Color::Dark
    };
    for invert in [true, false] {
        let text = terminal_qr(url, invert).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), side.div_ceil(2));
        for (r, line) in lines.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            assert_eq!(chars.len(), side);
            for (x, ch) in chars.iter().enumerate() {
                // In inverted mode a filled block is a LIGHT module (white on a dark terminal).
                let filled_top = matches!(ch, '█' | '▀');
                let filled_bottom = matches!(ch, '█' | '▄');
                let (top, bottom) = if invert {
                    (!filled_top, !filled_bottom)
                } else {
                    (filled_top, filled_bottom)
                };
                assert_eq!(
                    top,
                    dark_at(x, 2 * r),
                    "module ({x},{}) invert={invert}",
                    2 * r
                );
                if 2 * r + 1 < side {
                    assert_eq!(
                        bottom,
                        dark_at(x, 2 * r + 1),
                        "module ({x},{}) invert={invert}",
                        2 * r + 1
                    );
                }
            }
        }
    }
}
