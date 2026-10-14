//! Runs one ID module from the command line without the host, for trying it on a photo.
//!
//!     cargo run --release -p panoply-id --example id -- passport photo.jpg country=US output=both
//!     cargo run --release -p panoply-id --example id -- sheet photo.jpg paper=a4 size=35x45
//!     cargo run --release -p panoply-id --example id -- signature sig.jpg preset=ssc-signature
//!
//! Options are key=value after the file. Results go next to the photo, or into the folder in `OUT`.

use std::path::PathBuf;

use panoply_id::modules::{Op, modules};
use panoply_image::ops::FsIo;
use serde_json::{Map, Value, json};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: id passport|sheet|signature FILE [key=value ...]");
        std::process::exit(2);
    }
    let op = match args[0].as_str() {
        "passport" => Op::PassportPhoto,
        "sheet" => Op::PrintSheet,
        "signature" => Op::SignatureCleaner,
        other => {
            eprintln!("unknown module {other}");
            std::process::exit(2);
        }
    };
    let mut files = vec![PathBuf::from(&args[1])];
    let mut opts = Map::new();
    for a in &args[2..] {
        match a.split_once('=') {
            Some((k, v)) => {
                let val = if let Ok(n) = v.parse::<f64>() {
                    json!(n)
                } else if v == "true" || v == "false" {
                    json!(v == "true")
                } else {
                    json!(v)
                };
                opts.insert(k.to_string(), val);
            }
            None => files.push(PathBuf::from(a)),
        }
    }
    let io = match std::env::var_os("OUT") {
        Some(d) => FsIo::in_dir(d),
        None => FsIo::new(),
    };
    let m = modules().into_iter().find(|m| m.op == op).expect("module");
    let t = std::time::Instant::now();
    match m.run(&io, &files, &Value::Object(opts)) {
        Ok(out) => {
            for l in io.take_notes() {
                println!("{l}");
            }
            for p in out {
                println!("wrote {}", p.display());
            }
            eprintln!("{:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
