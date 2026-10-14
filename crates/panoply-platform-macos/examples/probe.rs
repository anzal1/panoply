//! Prints what ImageIO says about each file given on the command line.
#[cfg(target_os = "macos")]
fn main() {
    use panoply_platform_macos::Decoder;
    for path in std::env::args().skip(1) {
        let t = std::time::Instant::now();
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => {
                println!("{path}: read error {e}");
                continue;
            }
        };
        match Decoder::open(data) {
            Ok(d) => {
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                let mut info = d.info().clone();
                let icc = info.icc.take().map(|v| v.len());
                println!("{path}\n  open {ms:.1} ms\n  {info:?}\n  icc bytes {icc:?}");
                println!(
                    "  oriented {:?} sections {:?} top orientation {:?}",
                    d.oriented_size(),
                    d.properties().section_names(),
                    d.properties().orientation()
                );
                println!("  primary source index {}", d.primary_source_index());
                for f in 0..d.info().frames.min(3) {
                    let p = d.frame_properties(f).unwrap();
                    println!(
                        "  frame {f} props: HasAlpha {:?} Depth {:?} Orientation {:?} PrimaryImage {:?} ColorModel {:?} Headroom {:?} sections {:?}",
                        p.top_value("HasAlpha"),
                        p.top_value("Depth"),
                        p.top_value("Orientation"),
                        p.top_value("PrimaryImage"),
                        p.top_value("ColorModel"),
                        p.top_value("Headroom"),
                        p.section_names()
                    );
                    println!("  frame {f}: {:?}", d.frame_geometry(f));
                }
            }
            Err(e) => println!("{path}\n  ERR {e}"),
        }
    }
}
#[cfg(not(target_os = "macos"))]
fn main() {}
