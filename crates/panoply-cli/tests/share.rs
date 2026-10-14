//! `panoply share` end to end, over loopback only.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_panoply"))
}

fn temp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("panoply-cli-share-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Lines from a pipe, as they arrive.
fn lines_of(r: impl Read + Send + 'static) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for l in BufReader::new(r).lines().map_while(Result::ok) {
            if tx.send(l).is_err() {
                break;
            }
        }
    });
    rx
}

fn expect(rx: &mpsc::Receiver<String>, prefix: &str) -> String {
    let mut seen = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(20)) {
            Ok(l) if l.starts_with(prefix) => return l,
            Ok(l) => seen.push(l),
            Err(_) => panic!("no line starting with {prefix:?}; saw {seen:?}"),
        }
    }
}

#[test]
fn send_and_receive_between_two_processes() {
    let work = temp("sr");
    let inbox = work.join("inbox");
    std::fs::write(work.join("hello.txt"), "hello from the cli").unwrap();
    std::fs::create_dir_all(work.join("tree/sub")).unwrap();
    std::fs::write(work.join("tree/sub/deep.txt"), "deep").unwrap();

    // A free port is looked up and then handed to the child, and another test's process can take
    // it in between. Try a few times; only the "cannot listen" answer is retried.
    let mut started = None;
    for _ in 0..5 {
        let port = free_port();
        let mut rx = bin()
            .args([
                "share",
                "receive",
                "--yes",
                "--no-discovery",
                "--bind",
                "127.0.0.1",
            ])
            .args(["--port", &port.to_string(), "--alias", "CliReceiver"])
            .arg("--data-dir")
            .arg(work.join("rx-data"))
            .arg("--dir")
            .arg(&inbox)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = lines_of(rx.stdout.take().unwrap());
        let stderr = lines_of(rx.stderr.take().unwrap());
        let guard = Guard(rx);
        match stderr.recv_timeout(Duration::from_secs(20)) {
            Ok(l) if l.starts_with("cannot listen") => continue,
            Ok(l) if l.starts_with("Receiving as \"CliReceiver\"") => {
                started = Some((port, stdout, stderr, guard));
                break;
            }
            Ok(_) => {
                expect(&stderr, "Receiving as \"CliReceiver\"");
                started = Some((port, stdout, stderr, guard));
                break;
            }
            Err(_) => panic!("the receiver printed nothing"),
        }
    }
    let (port, stdout, _stderr, _guard) = started.expect("no free port for the receiver");

    let out = bin()
        .args(["share", "send", &format!("127.0.0.1:{port}")])
        .arg(work.join("hello.txt"))
        .arg(work.join("tree"))
        .args([
            "--no-discovery",
            "--bind",
            "127.0.0.1",
            "--port",
            "0",
            "--alias",
            "CliSender",
            "--hash",
        ])
        .arg("--data-dir")
        .arg(work.join("tx-data"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "{text} {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.starts_with("Sent 2 files, "), "{text}");
    assert!(text.contains("to CliReceiver"), "{text}");

    expect(&stdout, "Received 2 files from CliSender");
    assert_eq!(
        std::fs::read_to_string(inbox.join("hello.txt")).unwrap(),
        "hello from the cli"
    );
    assert_eq!(
        std::fs::read_to_string(inbox.join("tree/sub/deep.txt")).unwrap(),
        "deep"
    );
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn send_to_a_missing_device_fails_plainly() {
    let work = temp("miss");
    std::fs::write(work.join("a.txt"), "x").unwrap();
    let out = bin()
        .args(["share", "send", "Nobody Here"])
        .arg(work.join("a.txt"))
        .args([
            "--no-discovery",
            "--bind",
            "127.0.0.1",
            "--port",
            "0",
            "--wait",
            "1",
        ])
        .arg("--data-dir")
        .arg(work.join("d"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("No device matches"));
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn list_with_nothing_around_says_so() {
    let work = temp("list");
    let out = bin()
        .args([
            "share",
            "list",
            "--no-discovery",
            "--bind",
            "127.0.0.1",
            "--port",
            "0",
            "--wait",
            "0",
        ])
        .arg("--data-dir")
        .arg(work.join("d"))
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("No devices found."));
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn qr_prints_a_link_and_a_code_and_serves_the_file_once() {
    let work = temp("qr");
    std::fs::write(work.join("pic.bin"), vec![42u8; 100_000]).unwrap();
    let mut qr = bin()
        .args(["share", "qr", "--bind", "127.0.0.1", "--minutes", "1"])
        .arg(work.join("pic.bin"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = lines_of(qr.stdout.take().unwrap());
    let _stderr = lines_of(qr.stderr.take().unwrap());
    let mut guard = Guard(qr);
    let url = expect(&stdout, "http://127.0.0.1:");
    // The QR code follows. Its size depends on the token: the encoder packs digit runs tighter,
    // so a 55 character link is a 29 module code (version 3) or a 33 module one (version 4),
    // never smaller. With the quiet zone and two module rows to a text row that is 19 or 21
    // rows of block characters, so 19 is the floor. Read until those rows have arrived rather
    // than until the output goes quiet, because a loaded machine can pause the child or this
    // reader for longer than any fixed quiet period.
    let mut rows = 0;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while rows < 19 {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let Ok(l) = stdout.recv_timeout(left) else {
            break;
        };
        if l.contains('█') || l.contains('▀') || l.contains('▄') {
            rows += 1;
        }
    }
    assert!(rows >= 19, "{rows} QR rows");

    let rest = url.strip_prefix("http://").unwrap();
    let (host, path) = rest.split_once('/').unwrap();
    let mut s = TcpStream::connect(host).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(
        s,
        "GET /{path}/d/0 HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut resp = Vec::new();
    s.read_to_end(&mut resp).unwrap();
    let head_end = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    assert!(String::from_utf8_lossy(&resp[..head_end]).starts_with("HTTP/1.1 200"));
    assert_eq!(&resp[head_end..], &vec![42u8; 100_000][..]);

    // The one download completed the share, so the process exits by itself.
    let status = (0..100)
        .find_map(|_| {
            std::thread::sleep(Duration::from_millis(100));
            guard.0.try_wait().unwrap()
        })
        .expect("the qr command exits after the download");
    assert!(status.success());
    let _ = std::fs::remove_dir_all(&work);
}
