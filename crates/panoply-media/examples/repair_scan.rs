//! Print what the repair scanner finds in the media data of a file: `repair_scan broken.mp4 ok.mp4`
//! (the second file is the reference whose parameter sets are used).

use panoply_media::mp4::boxes;
use panoply_media::mp4::movie::parse_movie;
use panoply_media::mp4::repair;
use panoply_media::mp4::source::Source;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let broken = Source::open(std::path::Path::new(&args[1])).unwrap();
    let d = repair::diagnose(&broken).unwrap();
    println!("{d:?}");
    let reference = Source::open(std::path::Path::new(&args[2])).unwrap();
    let l = boxes::scan(&reference).unwrap();
    let moov = l.find(b"moov").unwrap();
    let body = reference
        .read_exact_at(moov.payload(), (moov.size - 8) as usize)
        .unwrap();
    let movie = parse_movie(&body).unwrap();
    let vt = movie.tracks.iter().find(|t| t.is_video()).unwrap();
    let params = repair::params_from_track(vt).unwrap();
    println!(
        "codec {:?} length size {} sets {}",
        params.codec,
        params.length_size,
        params.sets.len()
    );
    let (start, end, _) = d.mdat.unwrap();
    let t = std::time::Instant::now();
    let scan = repair::scan_pictures(
        &broken,
        start,
        end,
        params.codec,
        params.length_size,
        repair::hints_from_track(vt),
        &|| false,
    )
    .unwrap();
    println!(
        "{} pictures, {} picture bytes, {} other bytes, stopped at {} of {} in {:?}",
        scan.pictures.len(),
        scan.picture_bytes,
        scan.other_bytes,
        scan.stopped_at - start,
        end - start,
        t.elapsed()
    );
    for (i, (o, l)) in scan.pictures.iter().take(12).enumerate() {
        println!("  picture {i}: offset {} len {l}", o - start);
    }
    compare(&scan, start, vt);
    around(
        vt,
        movie.tracks.iter().find(|t| t.is_audio()).unwrap(),
        start,
        2577203,
    );
    // The reference's own sample sizes for comparison.
    println!(
        "reference video sample sizes: {:?}",
        vt.samples
            .iter()
            .take(6)
            .map(|s| s.size)
            .collect::<Vec<_>>()
    );
    println!(
        "reference video offsets: {:?}",
        vt.samples
            .iter()
            .take(6)
            .map(|s| s.offset - start)
            .collect::<Vec<_>>()
    );
    let a = movie.tracks.iter().find(|t| t.is_audio()).unwrap();
    println!(
        "reference audio first sizes {:?} offsets {:?}",
        a.samples.iter().take(6).map(|s| s.size).collect::<Vec<_>>(),
        a.samples
            .iter()
            .take(6)
            .map(|s| s.offset - start)
            .collect::<Vec<_>>()
    );
}

// Compare with the reference sample by sample: the scanner should find exactly the same pictures
// in the data both files share.
#[allow(dead_code)]
fn compare(scan: &repair::Scan, start: u64, vt: &panoply_media::mp4::movie::Track) {
    let want: std::collections::BTreeMap<u64, u32> = vt
        .samples
        .iter()
        .map(|s| (s.offset - start, s.size))
        .collect();
    let got: std::collections::BTreeMap<u64, u64> =
        scan.pictures.iter().map(|(o, l)| (o - start, *l)).collect();
    let mut wrong = 0;
    for (o, l) in &got {
        match want.get(o) {
            Some(w) if u64::from(*w) == *l => {}
            other => {
                wrong += 1;
                if wrong <= 8 {
                    println!("  MISMATCH at {o}: found len {l}, reference has {other:?}");
                }
            }
        }
    }
    let limit = scan.stopped_at - start;
    let missed = want
        .iter()
        .filter(|(o, s)| **o + u64::from(**s) <= limit && !got.contains_key(o))
        .count();
    println!(
        "{} pictures differ from the reference, {missed} reference pictures were not found",
        wrong
    );
}

#[allow(dead_code)]
fn around(
    vt: &panoply_media::mp4::movie::Track,
    a: &panoply_media::mp4::movie::Track,
    start: u64,
    at: u64,
) {
    let mut all: Vec<(u64, u32, &str)> = vt
        .samples
        .iter()
        .map(|s| (s.offset - start, s.size, "V"))
        .collect();
    all.extend(a.samples.iter().map(|s| (s.offset - start, s.size, "A")));
    all.sort();
    let i = all
        .iter()
        .position(|x| x.0 + u64::from(x.1) > at)
        .unwrap_or(0);
    for x in all.iter().skip(i.saturating_sub(3)).take(8) {
        println!("  ref {} at {} len {}", x.2, x.0, x.1);
    }
}
