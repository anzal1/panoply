//! The `panoply share` commands: list, send, receive, qr.

use std::io::{IsTerminal, Write};
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::{Args, Subcommand};

use crate::error::{Error, Result};
use crate::identity::Identity;
use crate::node::{Device, Direction, Event, Node, NodeConfig, TransferKind, default_data_dir};
use crate::proto::DEFAULT_PORT;
use crate::qr::{QrEvent, QrOptions, QrOutcome, QrShare, terminal_qr};
use crate::receive::{
    BoxFuture, Decision, IncomingRequest, ReceiveConfig, ReceiveMode, handler_fn,
};
use crate::send::SendOptions;

/// Options every share command understands.
#[derive(Args, Clone)]
pub struct NetArgs {
    /// Name other devices see. Default: this computer's name
    #[arg(long)]
    alias: Option<String>,
    /// Port for the server and discovery (53317 is what LocalSend uses)
    #[arg(long, default_value_t = DEFAULT_PORT)]
    port: u16,
    /// Folder for this device's certificate and trust list
    #[arg(long, value_name = "DIR")]
    data_dir: Option<PathBuf>,
    /// Address to bind (default: all interfaces)
    #[arg(long, value_name = "IP")]
    bind: Option<IpAddr>,
    /// Do not use multicast discovery (give an address instead)
    #[arg(long)]
    no_discovery: bool,
}

/// `panoply share ...`
#[derive(Subcommand)]
pub enum ShareCommand {
    /// List devices on the network that can receive files
    List {
        /// Seconds to listen for answers
        #[arg(long, default_value_t = 3)]
        wait: u64,
        /// Also probe every address of the local subnet (for networks that drop multicast)
        #[arg(long)]
        scan: bool,
        /// Network options
        #[command(flatten)]
        net: NetArgs,
    },
    /// Send files or folders: panoply share send "Anzal's iPhone" photo.jpg Trip/
    Send {
        /// Device name, fingerprint start, or address (192.168.1.34 or 192.168.1.34:53317)
        device: String,
        /// Files and folders to send
        #[arg(required_unless_present = "text")]
        files: Vec<PathBuf>,
        /// Send this text message instead of files
        #[arg(long)]
        text: Option<String>,
        /// PIN the receiver asked for
        #[arg(long)]
        pin: Option<String>,
        /// Hash files first so the receiver can verify them
        #[arg(long)]
        hash: bool,
        /// Seconds to look for the device
        #[arg(long, default_value_t = 4)]
        wait: u64,
        /// Network options
        #[command(flatten)]
        net: NetArgs,
    },
    /// Receive files from other devices until stopped with Ctrl-C
    Receive {
        /// Folder to save into
        #[arg(long, value_name = "DIR", default_value = "Panoply Received")]
        dir: PathBuf,
        /// Require this PIN from senders
        #[arg(long)]
        pin: Option<String>,
        /// Accept every request without asking
        #[arg(long, short = 'y')]
        yes: bool,
        /// Accept devices you chose to trust without asking
        #[arg(long)]
        auto_accept_trusted: bool,
        /// Save each sender's files in a folder named after it
        #[arg(long)]
        subfolders: bool,
        /// Network options
        #[command(flatten)]
        net: NetArgs,
    },
    /// Share files with a phone browser: prints a link and a QR code
    Qr {
        /// Files and folders to offer for download
        files: Vec<PathBuf>,
        /// Also let the other device upload into this folder
        #[arg(long, value_name = "DIR")]
        upload_dir: Option<PathBuf>,
        /// Ask for this PIN on the page
        #[arg(long)]
        pin: Option<String>,
        /// Serve HTTPS (browsers warn about the self-signed certificate)
        #[arg(long)]
        https: bool,
        /// Close after this many minutes
        #[arg(long, default_value_t = 10)]
        minutes: u64,
        /// Port for the page (default: a random free one)
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Address to bind (default: all interfaces)
        #[arg(long, value_name = "IP")]
        bind: Option<IpAddr>,
        /// Draw the QR code for a light terminal (default suits a dark one)
        #[arg(long)]
        light_terminal: bool,
        /// Folder for the HTTPS certificate
        #[arg(long, value_name = "DIR")]
        data_dir: Option<PathBuf>,
    },
}

/// Run a share command. Builds its own small tokio runtime.
pub fn run(cmd: ShareCommand) -> ExitCode {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("cannot start: {e}");
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(dispatch(cmd)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Error::Cancelled) => {
            eprintln!("Cancelled.");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

async fn dispatch(cmd: ShareCommand) -> Result<()> {
    match cmd {
        ShareCommand::List { wait, scan, net } => list(wait, scan, net).await,
        ShareCommand::Send {
            device,
            files,
            text,
            pin,
            hash,
            wait,
            net,
        } => send(device, files, text, pin, hash, wait, net).await,
        ShareCommand::Receive {
            dir,
            pin,
            yes,
            auto_accept_trusted,
            subfolders,
            net,
        } => receive(dir, pin, yes, auto_accept_trusted, subfolders, net).await,
        ShareCommand::Qr {
            files,
            upload_dir,
            pin,
            https,
            minutes,
            port,
            bind,
            light_terminal,
            data_dir,
        } => {
            qr(
                files,
                upload_dir,
                pin,
                https,
                minutes,
                port,
                bind,
                light_terminal,
                data_dir,
            )
            .await
        }
    }
}

fn this_computer() -> String {
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

fn node_config(net: &NetArgs, ephemeral: bool) -> NodeConfig {
    let mut c = NodeConfig::new(net.alias.clone().unwrap_or_else(this_computer));
    c.port = net.port;
    c.multicast_port = net.port;
    c.ephemeral_if_busy = ephemeral;
    c.data_dir = Some(net.data_dir.clone().unwrap_or_else(default_data_dir));
    c.bind = net.bind.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    c.discovery = !net.no_discovery;
    c
}

fn human(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
}

fn rate(bytes: u64, d: Duration) -> String {
    let s = d.as_secs_f64().max(0.001);
    format!("{}/s", human((bytes as f64 / s) as u64))
}

fn device_line(d: &Device) -> String {
    format!(
        "{:<24} {:<8} {:<22} {:<5} {}",
        d.alias(),
        d.info.kind().as_str(),
        d.socket_addr(),
        d.info.scheme_or_default().as_str(),
        &d.fingerprint()[..d.fingerprint().len().min(8)]
    )
}

async fn list(wait: u64, scan: bool, net: NetArgs) -> Result<()> {
    let node = Node::start(node_config(&net, true)).await?;
    if scan {
        node.scan().await;
    }
    tokio::time::sleep(Duration::from_secs(wait)).await;
    let devices = node.devices();
    if devices.is_empty() {
        println!("No devices found.");
        if !scan {
            println!("If the other device is on this network, try --scan.");
        }
    } else {
        for d in devices {
            println!("{}", device_line(&d));
        }
    }
    node.shutdown();
    Ok(())
}

async fn find_device(node: &Node, query: &str, wait: u64) -> Result<Device> {
    let deadline = Instant::now() + Duration::from_secs(wait);
    loop {
        match node.find(query).await {
            Ok(d) => return Ok(d),
            Err(Error::NoSuchDevice(_)) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

async fn send(
    device: String,
    files: Vec<PathBuf>,
    text: Option<String>,
    pin: Option<String>,
    hash: bool,
    wait: u64,
    net: NetArgs,
) -> Result<()> {
    let node = Node::start(node_config(&net, true)).await?;
    let target = match find_device(&node, &device, wait).await {
        Ok(d) => d,
        Err(Error::NoSuchDevice(q)) => {
            // Last resort for networks that drop multicast.
            if !net.no_discovery {
                node.scan().await;
            }
            match node.find(&q).await {
                Ok(d) => d,
                Err(_) => {
                    let known = node.devices();
                    let mut msg = format!("No device matches \"{q}\".");
                    if !known.is_empty() {
                        msg.push_str(" Found:");
                        for d in known {
                            msg.push_str(&format!("\n  {}", d.alias()));
                        }
                    }
                    return Err(Error::NoSuchDevice(msg));
                }
            }
        }
        Err(e) => return Err(e),
    };
    let interactive = std::io::stdin().is_terminal();
    let provider: Option<crate::send::PinProvider> = interactive.then(|| {
        Arc::new(|attempt: u32| -> BoxFuture<Option<String>> {
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    eprint!("{}PIN: ", if attempt > 1 { "Wrong PIN. " } else { "" });
                    let _ = std::io::stderr().flush();
                    let mut s = String::new();
                    std::io::stdin().read_line(&mut s).ok()?;
                    let s = s.trim().to_string();
                    (!s.is_empty()).then_some(s)
                })
                .await
                .ok()
                .flatten()
            })
        }) as crate::send::PinProvider
    });
    let options = SendOptions {
        pin,
        pin_provider: provider,
        hash_files: hash,
        ..SendOptions::default()
    };
    eprintln!(
        "Sending to {} ({}). Waiting for it to accept.",
        target.alias(),
        target.socket_addr()
    );
    let mut events = node.subscribe();
    let job = match text {
        Some(t) => node.send_text(target.clone(), t, options),
        None => node.send(target.clone(), files, options),
    };
    let id = job.id().to_string();
    let progress = tokio::spawn(async move {
        let tty = std::io::stderr().is_terminal();
        let mut last = Instant::now() - Duration::from_secs(1);
        while let Ok(ev) = events.recv().await {
            let Event::Transfer(t) = ev else { continue };
            if t.id != id || t.direction != Direction::Send {
                continue;
            }
            match t.kind {
                TransferKind::Progress {
                    total_done, total, ..
                } if tty => {
                    if last.elapsed() > Duration::from_millis(200) {
                        last = Instant::now();
                        let pct = (total_done * 100).checked_div(total).unwrap_or(100);
                        eprint!("\r{pct:>3}%  {} of {}   ", human(total_done), human(total));
                    }
                }
                TransferKind::Started { files, bytes } => {
                    eprintln!(
                        "Accepted: {files} file{} ({}).",
                        if files == 1 { "" } else { "s" },
                        human(bytes)
                    );
                }
                TransferKind::Finished { .. }
                | TransferKind::Failed { .. }
                | TransferKind::Cancelled => {
                    if tty {
                        eprint!("\r{:<40}\r", "");
                    }
                    return;
                }
                _ => {}
            }
        }
    });
    let canceller = job.cancel_handle();
    let mut wait = Box::pin(job.wait());
    let result = tokio::select! {
        r = &mut wait => r,
        _ = tokio::signal::ctrl_c() => {
            canceller.cancel();
            wait.await
        }
    };
    let _ = progress.await;
    let summary = result?;
    if summary.text_delivered {
        println!("Message delivered to {}.", target.alias());
    } else {
        println!(
            "Sent {} file{}, {} to {} in {:.1} s ({}). Encrypted, direct, no server.",
            summary.files,
            if summary.files == 1 { "" } else { "s" },
            human(summary.bytes),
            target.alias(),
            summary.elapsed.as_secs_f64(),
            rate(summary.bytes, summary.elapsed)
        );
    }
    node.shutdown();
    Ok(())
}

fn describe_request(r: &IncomingRequest) {
    eprintln!(
        "\n{} ({}, {}) wants to send {}",
        r.sender.alias,
        r.sender.kind().as_str(),
        r.remote,
        match &r.text {
            Some(_) => "a message".to_string(),
            None => format!(
                "{} file{} ({})",
                r.files.len(),
                if r.files.len() == 1 { "" } else { "s" },
                human(r.total_size)
            ),
        }
    );
    eprintln!(
        "  check code: {}   {}",
        r.check_code,
        if r.verified {
            "identity verified"
        } else {
            "identity NOT verified"
        }
    );
    if r.alias_conflict {
        eprintln!("  WARNING: a trusted device uses this name with a different identity.");
    }
    if let Some(t) = &r.text {
        eprintln!("  message: {t}");
    }
    for f in r.files.iter().take(8) {
        eprintln!(
            "  {}{}  {}",
            f.name,
            if f.risky { "  (can run code)" } else { "" },
            human(f.size)
        );
    }
    if r.files.len() > 8 {
        eprintln!("  and {} more", r.files.len() - 8);
    }
}

async fn receive(
    dir: PathBuf,
    pin: Option<String>,
    yes: bool,
    auto_trusted: bool,
    subfolders: bool,
    net: NetArgs,
) -> Result<()> {
    let handler = handler_fn(move |req: IncomingRequest| -> BoxFuture<Decision> {
        Box::pin(async move {
            describe_request(&req);
            if yes {
                return Decision::accept_all();
            }
            if !std::io::stdin().is_terminal() {
                eprintln!(
                    "  Declined: no terminal to ask on. Start with --yes to accept without asking."
                );
                return Decision::Decline;
            }
            let can_trust = req.verified;
            let answer = tokio::task::spawn_blocking(move || {
                eprint!(
                    "  Accept? [y]es, [n]o{}: ",
                    if can_trust {
                        ", [a]lways from this device"
                    } else {
                        ""
                    }
                );
                let _ = std::io::stderr().flush();
                let mut s = String::new();
                std::io::stdin().read_line(&mut s).ok();
                s.trim().to_ascii_lowercase()
            })
            .await
            .unwrap_or_default();
            match answer.as_str() {
                "y" | "yes" => Decision::accept_all(),
                "a" | "always" if can_trust => Decision::Accept {
                    files: None,
                    trust: true,
                },
                _ => Decision::Decline,
            }
        })
    });
    let mut rc = ReceiveConfig::new(dir.clone(), handler);
    rc.pin = pin;
    rc.subfolder_per_sender = subfolders;
    if auto_trusted {
        rc.mode = ReceiveMode::AutoAcceptTrusted;
    }
    let mut cfg = node_config(&net, false);
    cfg.receive = Some(rc);
    cfg.device_type = crate::proto::DeviceType::Desktop;
    let node = Node::start(cfg).await?;
    eprintln!(
        "Receiving as \"{}\" on port {}. Saving to {}. Press Ctrl-C to stop.",
        node.info().alias,
        node.local_addr().port(),
        dir.display()
    );
    let mut events = node.subscribe();
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            ev = events.recv() => {
                let Ok(ev) = ev else { continue };
                match ev {
                    Event::Transfer(t) if t.direction == Direction::Receive => match t.kind {
                        TransferKind::FileDone { path: Some(p), .. } => println!("Received {}", p.display()),
                        TransferKind::Text { text } => println!("Message from {}: {text}", t.peer),
                        TransferKind::Finished { files, bytes, .. } => println!(
                            "Received {files} file{} from {} ({}) to {}.",
                            if files == 1 { "" } else { "s" },
                            t.peer,
                            human(bytes),
                            dir.display()
                        ),
                        TransferKind::Failed { error } => eprintln!("Transfer from {} failed: {error}", t.peer),
                        TransferKind::Cancelled => eprintln!("Transfer from {} cancelled.", t.peer),
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }
    node.shutdown();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn qr(
    files: Vec<PathBuf>,
    upload_dir: Option<PathBuf>,
    pin: Option<String>,
    https: bool,
    minutes: u64,
    port: u16,
    bind: Option<IpAddr>,
    light_terminal: bool,
    data_dir: Option<PathBuf>,
) -> Result<()> {
    if files.is_empty() && upload_dir.is_none() {
        return Err(Error::Input(
            "give files to share, or --upload-dir to receive".into(),
        ));
    }
    let tls = if https {
        Some(Arc::new(Identity::load_or_create(
            &data_dir.unwrap_or_else(default_data_dir),
        )?))
    } else {
        None
    };
    let share = QrShare::start(QrOptions {
        files,
        upload_dir,
        pin: pin.clone(),
        tls,
        expires: Duration::from_secs(minutes.max(1) * 60),
        port,
        bind: bind.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
        ..QrOptions::default()
    })
    .await?;
    println!("{}", share.url());
    println!("{}", terminal_qr(share.url(), !light_terminal)?);
    if !share.on_lan() {
        eprintln!(
            "This link only works on this computer: no private network address was found, or it was bound to localhost."
        );
    }
    if share.scheme() == "https" {
        eprintln!(
            "HTTPS with a self-signed certificate: the browser will warn once. Plain HTTP shows no warning."
        );
    } else {
        eprintln!("Plain HTTP on your local network, protected by the secret in the link.");
    }
    if pin.is_some() {
        eprintln!("The page asks for the PIN.");
    }
    eprintln!(
        "The link closes after the first completed transfer, after {minutes} min, or on Ctrl-C."
    );
    let mut events = share.subscribe();
    let outcome = loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => { share.revoke(); }
            o = share.wait() => break o,
            ev = events.recv() => {
                match ev {
                    Ok(QrEvent::Connected { ip, user_agent }) => eprintln!("{ip} opened the page ({user_agent})"),
                    Ok(QrEvent::DownloadStarted { ip, file }) => eprintln!("{ip} is downloading {file}"),
                    Ok(QrEvent::Downloaded { ip, file, bytes }) => eprintln!("{ip} downloaded {file} ({})", human(bytes)),
                    Ok(QrEvent::Uploaded { ip, path, bytes }) => eprintln!("{ip} uploaded {} ({})", path.display(), human(bytes)),
                    _ => {}
                }
            }
        }
    };
    match outcome {
        QrOutcome::Completed => println!("Done. Link closed."),
        QrOutcome::Expired => println!("Link expired."),
        QrOutcome::Revoked => println!("Link closed."),
    }
    Ok(())
}
