//! A badly behaved module. `mode` picks the misbehaviour. Test fixture only.
wit_bindgen::generate!({ path: "wit", world: "module" });
use panoply::module::host;

struct Module;

fn spin() -> ! {
    let mut n = 0u64;
    loop {
        n = std::hint::black_box(n.wrapping_add(1));
    }
}

impl Guest for Module {
    fn run(inputs: Vec<InputFile>, options: String) -> Result<Vec<Output>, String> {
        let options: serde_json::Value = serde_json::from_str(&options).map_err(|e| e.to_string())?;
        let mode = options["mode"].as_str().unwrap_or("ok").to_string();
        let first = inputs.first().map(|f| f.handle);
        let note = |s: String| Output::Note(s);
        match mode.as_str() {
            "ok" => {
                let n = host::save(first, "ok", "txt", b"ok")?;
                Ok(vec![Output::File(n)])
            }
            "spin" => spin(),
            "poll" => {
                while !host::cancelled() {
                    std::hint::spin_loop();
                }
                Err("stopped politely".into())
            }
            "hog" => {
                let mut v: Vec<u8> = Vec::new();
                for _ in 0..64 {
                    v.extend(std::iter::repeat_n(7u8, 1024 * 1024));
                }
                Ok(vec![note(format!("held {} bytes", v.len()))])
            }
            "readfs" => {
                let mut out = Vec::new();
                for path in ["/etc/passwd", "/", "."] {
                    let r = std::fs::read(path).map(|b| b.len()).map_err(|e| e.to_string());
                    out.push(note(format!("read {path}: {r:?}")));
                }
                if let Some(f) = inputs.first() {
                    let r = std::fs::read(&f.name).map(|b| b.len()).map_err(|e| e.to_string());
                    out.push(note(format!("read by name {}: {r:?}", f.name)));
                }
                let w = std::fs::write("pwned.txt", b"x").map_err(|e| e.to_string());
                out.push(note(format!("write: {w:?}")));
                out.push(note(format!(
                    "env {} args {}",
                    std::env::vars().count(),
                    std::env::args().count()
                )));
                Ok(out)
            }
            "badhandle" => {
                let r = host::read(99, 0, 10);
                Ok(vec![note(format!("{r:?}"))])
            }
            "write_then_fail" => {
                host::save(first, "partial", "txt", b"half")?;
                Err("boom".into())
            }
            "write_then_spin" => {
                host::save(first, "partial", "txt", b"half")?;
                spin()
            }
            "unfinished" => {
                let h = host::begin_output(first, "half", "txt")?;
                host::append_output(h, b"abc")?;
                Ok(vec![])
            }
            "unlisted" => {
                host::save(first, "forgot", "txt", b"x")?;
                Ok(vec![])
            }
            "fake" => Ok(vec![Output::File("never-written.txt".into())]),
            "stream" => {
                let h = host::begin_output(first, "../../escape", "../sh")?;
                for i in 0..4 {
                    host::append_output(h, format!("chunk {i}\n").as_bytes())?;
                    host::progress((i + 1) as f32 / 4.0, &format!("chunk {i}"));
                }
                Ok(vec![Output::File(host::finish_output(h)?)])
            }
            "panic" => panic!("kaboom from the module"),
            "bigread" => {
                let r = host::read_all(inputs[0].handle);
                Ok(vec![note(format!("{:?}", r.map(|b| b.len())))])
            }
            "read" => {
                let h = inputs[0].handle;
                let a = host::read(h, 0, 4)?;
                let b = host::read(h, 4, 4)?;
                let past = host::read(h, 1_000_000, 4)?;
                Ok(vec![note(format!(
                    "{}|{}|{}",
                    String::from_utf8_lossy(&a),
                    String::from_utf8_lossy(&b),
                    past.len()
                ))])
            }
            other => Err(format!("unknown mode {other}")),
        }
    }
}

export!(Module);
