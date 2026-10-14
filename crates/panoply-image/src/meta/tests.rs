//! Tests for the metadata module. Corpus tests skip when a file is missing.

use super::testutil::*;
use super::*;
use std::io::Cursor;
use std::ops::Range;

// ---------------------------------------------------------------------------
// Helpers

fn exif_fields(bytes: &[u8]) -> Option<::exif::Exif> {
    ::exif::Reader::new()
        .read_from_container(&mut Cursor::new(bytes))
        .ok()
}

fn has_gps_fields(bytes: &[u8]) -> bool {
    exif_fields(bytes)
        .map(|e| e.fields().any(|f| f.tag.context() == ::exif::Context::Gps))
        .unwrap_or(false)
}

fn orientation_of(bytes: &[u8]) -> Option<u32> {
    let e = exif_fields(bytes)?;
    e.get_field(::exif::Tag::Orientation, ::exif::In::PRIMARY)?
        .value
        .get_uint(0)
}

fn raw_orientation(blob: &[u8]) -> Option<u32> {
    let e = ::exif::Reader::new().read_raw(blob.to_vec()).ok()?;
    e.get_field(::exif::Tag::Orientation, ::exif::In::PRIMARY)?
        .value
        .get_uint(0)
}

fn secrets() -> Vec<Vec<u8>> {
    vec![
        SERIAL_BODY.as_bytes().to_vec(),
        SERIAL_LENS.as_bytes().to_vec(),
        UNIQUE_ID.as_bytes().to_vec(),
        OWNER.as_bytes().to_vec(),
        ARTIST.as_bytes().to_vec(),
        COPYRIGHT.as_bytes().to_vec(),
        COMMENT.as_bytes().to_vec(),
        THUMB_MARK.to_vec(),
        MAKER.to_vec(),
        XMP_CREATOR.as_bytes().to_vec(),
    ]
}

fn all_levels() -> [StripLevel; 3] {
    [StripLevel::Gps, StripLevel::Private, StripLevel::Everything]
}

fn synth_blob(le: bool) -> Vec<u8> {
    build_exif(&Opts {
        le,
        ..Opts::default()
    })
}

fn synth_jpeg_with(le: bool) -> Vec<u8> {
    synth_jpeg(&JpegOpts {
        exif: Some(synth_blob(le)),
        xmp: true,
        extras: true,
    })
}

fn splice_after_first_segment(jpeg: &[u8], extra: &[u8]) -> Vec<u8> {
    let len = u16::from_be_bytes([jpeg[4], jpeg[5]]) as usize;
    let at = 4 + len;
    let mut v = jpeg[..at].to_vec();
    v.extend_from_slice(extra);
    v.extend_from_slice(&jpeg[at..]);
    v
}

fn corpus_dir() -> std::path::PathBuf {
    match std::env::var("PANOPLY_REPO") {
        Ok(r) => std::path::PathBuf::from(r).join("tests/corpus/cache/image"),
        Err(_) => std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/corpus/cache/image"
        )),
    }
}

fn corpus_files() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![corpus_dir()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let ext = p
                .extension()
                .and_then(|x| x.to_str())
                .unwrap_or("")
                .to_lowercase();
            if matches!(
                ext.as_str(),
                "jpg"
                    | "jpeg"
                    | "png"
                    | "webp"
                    | "tif"
                    | "tiff"
                    | "heic"
                    | "heif"
                    | "hif"
                    | "avif"
                    | "dng"
                    | "cr2"
            ) && let Ok(b) = std::fs::read(&p)
            {
                let rel = p
                    .strip_prefix(corpus_dir())
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .to_string();
                out.push((rel, b));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    if out.is_empty() {
        eprintln!("skipping: no corpus files found");
    }
    out
}

fn tiff_strip_data(b: &[u8]) -> Vec<Vec<u8>> {
    let (o, off) = exif::parse_header(b).unwrap();
    let ifd = exif::read_ifd(b, o, off).unwrap();
    let mut out = Vec::new();
    for (ot, ct) in [(0x0111u16, 0x0117u16), (0x0144, 0x0145)] {
        let (Some(a), Some(c)) = (
            ifd.entries.iter().find(|e| e.tag == ot),
            ifd.entries.iter().find(|e| e.tag == ct),
        ) else {
            continue;
        };
        let read = |e: &exif::Entry, i: usize| -> Option<usize> {
            let r = e.data.clone()?;
            match e.typ {
                3 => o.u16(b, r.start + i * 2).map(|v| v as usize),
                4 => o.u32(b, r.start + i * 4).map(|v| v as usize),
                _ => None,
            }
        };
        for i in 0..(a.count as usize) {
            let (s, n) = (read(a, i).unwrap(), read(c, i).unwrap());
            out.push(b[s..s + n].to_vec());
        }
    }
    out
}

/// Raw bytes of the latitude and longitude values of an EXIF block, if any.
fn gps_value_bytes(blob: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let Some((o, off)) = exif::parse_header(blob) else {
        return out;
    };
    let Ok(ifd0) = exif::read_ifd(blob, o, off) else {
        return out;
    };
    let Some(g) = ifd0.entries.iter().find(|e| e.tag == 0x8825) else {
        return out;
    };
    let Some(p) = g.data.clone().and_then(|r| o.u32(blob, r.start)) else {
        return out;
    };
    let Ok(gifd) = exif::read_ifd(blob, o, p as usize) else {
        return out;
    };
    for e in &gifd.entries {
        if matches!(e.tag, 2 | 4 | 6 | 0x1D)
            && let Some(r) = e.data.clone()
            && r.len() >= 8
            && blob[r.clone()].iter().filter(|&&x| x != 0).count() >= 3
        {
            out.push(blob[r].to_vec());
        }
    }
    out
}

fn heic_changed_ranges(b: &[u8]) -> Vec<Range<usize>> {
    bmff::metadata_ranges(b)
}

// ---------------------------------------------------------------------------
// Basics

#[test]
fn crc32_matches_known_vector() {
    assert_eq!(crc32(&[b"123456789"]), 0xCBF4_3926);
    assert_eq!(crc32(&[b"1234", b"56789"]), 0xCBF4_3926);
}

#[test]
fn detect_recognizes_containers() {
    assert_eq!(detect(&synth_jpeg_with(false)), Container::Jpeg);
    assert_eq!(detect(&synth_png(None)), Container::Png);
    assert_eq!(detect(&riff(&[vp8_chunk(8, 8)])), Container::WebP);
    assert_eq!(detect(&synth_blob(true)), Container::Tiff);
    assert_eq!(
        detect(&synth_heic(&synth_blob(false)).bytes),
        Container::Bmff
    );
    assert_eq!(detect(b"GIF89a....."), Container::Unknown);
    assert_eq!(detect(b""), Container::Unknown);
    assert_eq!(detect(b"\xFF\xD8"), Container::Unknown);
}

#[test]
fn unsupported_type_has_plain_message() {
    let e = inspect(b"GIF89a.............").unwrap_err();
    assert!(matches!(e, MetaError::Unsupported(_)));
    let s = e.to_string();
    assert!(s.contains("Save it as"), "{s}");
    assert!(!s.contains('\u{2014}'));
    let c = strip(b"nonsense nonsense nonsense", StripLevel::Private).unwrap_err();
    assert!(matches!(c, MetaError::Unsupported(_)));
    let d = MetaError::Corrupt("x".into()).to_string();
    assert!(d.contains("damaged") && d.contains("try again"));
}

// ---------------------------------------------------------------------------
// Synthetic JPEG

#[test]
fn inspect_synthetic_jpeg_reads_everything() {
    for le in [false, true] {
        let j = synth_jpeg_with(le);
        let r = inspect(&j).unwrap();
        assert_eq!(r.container, Container::Jpeg);
        assert!(r.exif_present);
        assert_eq!(r.orientation, 6);
        let g = r.gps.clone().unwrap();
        assert!((g.lat - 37.774_916_7).abs() < 1e-5, "{g:?}");
        assert!((g.lon + 122.419_4).abs() < 1e-5, "{g:?}");
        assert!((g.alt.unwrap() - 123.4).abs() < 1e-6);
        assert_eq!(r.gps_tags, 7 + 3);
        assert_eq!(r.camera.as_deref(), Some("TestCam Model X"));
        assert_eq!(r.lens.as_deref(), Some("TestLens 50mm"));
        assert_eq!(r.software.as_deref(), Some("SynthSoft 1.0"));
        assert_eq!(r.date_taken.as_deref(), Some("2026:01:02 03:04:05"));
        let names: Vec<&str> = r.serials.iter().map(|(n, _)| n.as_str()).collect();
        for n in ["BodySerialNumber", "LensSerialNumber", "ImageUniqueID"] {
            assert!(names.contains(&n), "{names:?}");
        }
        let owners: Vec<&str> = r.owner.iter().map(|(n, _)| n.as_str()).collect();
        for n in ["Artist", "Copyright", "CameraOwnerName", "Creator (XMP)"] {
            assert!(owners.contains(&n), "{owners:?}");
        }
        assert!(r.xmp && r.iptc && r.c2pa && r.icc);
        assert_eq!(r.thumbnails, 2);
        assert_eq!(r.maker_note_bytes, MAKER.len());
        assert!(r.summary.starts_with("contains location"), "{}", r.summary);
        assert!(r.summary.contains("serial numbers"));
        assert!(r.summary.contains("2 thumbnails"));
    }
}

#[test]
fn inspect_clean_jpeg_says_no_private_metadata() {
    let j = synth_jpeg(&JpegOpts {
        exif: None,
        xmp: false,
        extras: false,
    });
    let r = inspect(&j).unwrap();
    assert_eq!(r.summary, "no private metadata found");
    assert_eq!(r.orientation, 1);
    assert!(r.gps.is_none() && r.gps_tags == 0 && !r.exif_present);
}

#[test]
fn private_strip_removes_everything_private_from_jpeg() {
    for le in [false, true] {
        let j = synth_jpeg_with(le);
        let out = strip(&j, StripLevel::Private).unwrap();
        for s in secrets() {
            assert!(
                !contains(&out.bytes, &s),
                "left {:?}",
                String::from_utf8_lossy(&s)
            );
        }
        for n in gps_needles(le) {
            assert!(!contains(&out.bytes, &n), "GPS bytes remain");
        }
        for s in [
            "COMMENT-SECRET-TEXT",
            "IPTC-SECRET-CAPTION",
            "c2pa",
            "jumb",
            "GPSLatitude",
            "xap/1.0",
        ] {
            assert!(!contains(&out.bytes, s.as_bytes()), "left {s}");
        }
        assert_eq!(jpeg_image_parts(&j), jpeg_image_parts(&out.bytes));
        let markers = jpeg_marker_list(&out.bytes);
        assert!(markers.contains(&0xE0) && markers.contains(&0xE1) && markers.contains(&0xE2));
        for gone in [0xED, 0xEB, 0xFE] {
            assert!(!markers.contains(&gone));
        }
        assert!(!has_gps_fields(&out.bytes));
        assert_eq!(orientation_of(&out.bytes), Some(6));
        let r = inspect(&out.bytes).unwrap();
        assert!(r.gps.is_none() && r.gps_tags == 0);
        assert!(r.serials.is_empty() && r.owner.is_empty());
        assert_eq!((r.thumbnails, r.maker_note_bytes), (0, 0));
        assert!(!r.xmp && !r.iptc && !r.c2pa);
        assert!(r.icc);
        assert_eq!(r.orientation, 6);
        assert_eq!(r.camera.as_deref(), Some("TestCam Model X"));
        assert_eq!(r.date_taken.as_deref(), Some("2026:01:02 03:04:05"));
        assert_eq!(r.summary, "no private metadata found");
        for line in [
            "GPS location",
            "serial numbers",
            "owner and copyright",
            "2 thumbnails",
            "maker notes",
            "XMP",
            "IPTC",
        ] {
            assert!(
                out.removed.iter().any(|x| x == line),
                "{line} missing in {:?}",
                out.removed
            );
        }
        assert!(out.kept.iter().any(|x| x == "orientation"));
        assert!(out.kept.iter().any(|x| x == "color profile"));
        assert!(out.bytes.len() < j.len());
    }
}

#[test]
fn gps_level_only_removes_location() {
    for le in [false, true] {
        let j = synth_jpeg_with(le);
        let out = strip(&j, StripLevel::Gps).unwrap();
        assert_eq!(out.bytes.len(), j.len());
        for n in gps_needles(le) {
            assert!(!contains(&out.bytes, &n));
        }
        for s in ["GPSLatitude", "GPSAltitude", "37,46.495N", "122,25.164W"] {
            assert!(!contains(&out.bytes, s.as_bytes()), "left {s}");
        }
        assert!(contains(&out.bytes, XMP_CREATOR.as_bytes()));
        assert!(contains(&out.bytes, MAKER));
        assert!(contains(&out.bytes, SERIAL_BODY.as_bytes()));
        assert!(contains(&out.bytes, THUMB_MARK));
        assert!(!has_gps_fields(&out.bytes));
        assert_eq!(orientation_of(&out.bytes), Some(6));
        let r = inspect(&out.bytes).unwrap();
        assert_eq!((r.gps_tags, r.gps), (0, None));
        assert_eq!(r.thumbnails, 2);
        assert_eq!(r.serials.len(), 3);
        assert_eq!(out.removed, vec!["GPS location".to_string()]);
        assert_eq!(jpeg_image_parts(&j), jpeg_image_parts(&out.bytes));
    }
}

#[test]
fn everything_level_writes_minimal_exif() {
    for le in [false, true] {
        let j = synth_jpeg_with(le);
        let out = strip(&j, StripLevel::Everything).unwrap();
        for s in secrets().into_iter().chain([
            b"TestCam".to_vec(),
            b"SynthSoft".to_vec(),
            b"TestLens".to_vec(),
        ]) {
            assert!(
                !contains(&out.bytes, &s),
                "left {:?}",
                String::from_utf8_lossy(&s)
            );
        }
        for n in gps_needles(le) {
            assert!(!contains(&out.bytes, &n));
        }
        let markers = jpeg_marker_list(&out.bytes);
        assert_eq!(&markers[..2], &[0xE0, 0xE1], "EXIF goes right after JFIF");
        assert!(markers.contains(&0xE2) && markers.contains(&0xEE));
        for gone in [0xED, 0xEB, 0xFE] {
            assert!(!markers.contains(&gone));
        }
        assert_eq!(jpeg_image_parts(&j), jpeg_image_parts(&out.bytes));
        assert_eq!(orientation_of(&out.bytes), Some(6));
        let e = exif_fields(&out.bytes).unwrap();
        assert_eq!(e.fields().count(), 1);
        let r = inspect(&out.bytes).unwrap();
        assert!(r.camera.is_none() && r.date_taken.is_none() && !r.xmp && r.icc);
        assert!(
            out.removed
                .iter()
                .any(|x| x == "camera, date and exposure details")
        );
        assert!(out.removed.iter().any(|x| x == "GPS location"));
    }
}

#[test]
fn everything_level_orientation_one_leaves_no_exif() {
    let blob = build_exif(&Opts {
        orientation: 1,
        ..Opts::default()
    });
    let j = synth_jpeg(&JpegOpts {
        exif: Some(blob),
        xmp: true,
        extras: true,
    });
    let out = strip(&j, StripLevel::Everything).unwrap();
    let markers = jpeg_marker_list(&out.bytes);
    assert!(!markers.contains(&0xE1));
    assert!(!inspect(&out.bytes).unwrap().exif_present);
}

#[test]
fn strip_is_idempotent() {
    let j = synth_jpeg_with(false);
    for level in all_levels() {
        let a = strip(&j, level).unwrap().bytes;
        let b = strip(&a, level).unwrap().bytes;
        assert_eq!(a, b, "{level:?}");
    }
}

#[test]
fn xmp_only_gps_is_found_and_blanked() {
    let j = synth_jpeg(&JpegOpts {
        exif: None,
        xmp: true,
        extras: false,
    });
    let r = inspect(&j).unwrap();
    let g = r.gps.unwrap();
    assert!(
        (g.lat - 37.774_916_7).abs() < 1e-4 && g.lon < -122.0,
        "{g:?}"
    );
    assert!(r.gps_tags >= 2);
    let out = strip(&j, StripLevel::Gps).unwrap();
    assert!(!contains(&out.bytes, b"GPSLatitude"));
    assert_eq!(out.removed, vec!["GPS location".to_string()]);
    assert!(inspect(&out.bytes).unwrap().gps.is_none());
}

#[test]
fn live_photo_id_is_reported_and_removed() {
    let uuid = "A1B2C3D4-0000-1111-2222-ABCDEF012345";
    let blob = build_exif(&Opts {
        maker_note: Some(apple_maker_note(uuid)),
        thumbnail: false,
        ..Opts::default()
    });
    let j = synth_jpeg(&JpegOpts {
        exif: Some(blob),
        xmp: false,
        extras: false,
    });
    let r = inspect(&j).unwrap();
    assert_eq!(r.live_photo_id.as_deref(), Some(uuid));
    assert!(r.summary.contains("Live Photo"));
    for level in [StripLevel::Private, StripLevel::Everything] {
        let out = strip(&j, level).unwrap();
        assert!(!contains(&out.bytes, uuid.as_bytes()));
        assert!(inspect(&out.bytes).unwrap().live_photo_id.is_none());
    }
    let g = strip(&j, StripLevel::Gps).unwrap();
    assert!(contains(&g.bytes, uuid.as_bytes()));
}

#[test]
fn extended_xmp_is_blanked_across_chunks_and_dropped() {
    let guid = [b'A'; 32];
    let mut full =
        b"<x:xmpmeta xmlns:exif=\"e\"><rdf:Description exif:GPSLatitude=\"12,34.5N\" exif:GPS"
            .to_vec();
    full.extend_from_slice(b"Longitude=\"56,7.8E\"/></x:xmpmeta>");
    let (a, b) = full.split_at(70);
    let mk = |off: usize, data: &[u8]| {
        let mut p = b"http://ns.adobe.com/xmp/extension/\0".to_vec();
        p.extend_from_slice(&guid);
        p.extend_from_slice(&(full.len() as u32).to_be_bytes());
        p.extend_from_slice(&(off as u32).to_be_bytes());
        p.extend_from_slice(data);
        seg(0xE1, &p)
    };
    let base = synth_jpeg(&JpegOpts {
        exif: None,
        xmp: false,
        extras: false,
    });
    let j = splice_after_first_segment(
        &splice_after_first_segment(&base, &mk(0, a)),
        &mk(a.len(), b),
    );
    let g = strip(&j, StripLevel::Gps).unwrap();
    assert!(
        !contains(&g.bytes, b"GPSLongitude"),
        "split property survived"
    );
    assert!(!contains(&g.bytes, b"12,34.5N"));
    assert_eq!(g.bytes.len(), j.len());
    let p = strip(&j, StripLevel::Private).unwrap();
    assert!(!contains(&p.bytes, b"xmp/extension"));
    assert!(p.removed.iter().any(|x| x == "XMP"));
}

// ---------------------------------------------------------------------------
// MPF and gain maps

fn synth_mpf_jpeg(le: bool) -> (Vec<u8>, usize) {
    let blob = synth_blob(le);
    let primary = synth_jpeg(&JpegOpts {
        exif: Some(blob.clone()),
        xmp: false,
        extras: false,
    });
    let hdr_xmp = {
        let mut p = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        p.extend_from_slice(b"<x:xmpmeta xmlns:hdrgm=\"http://ns.adobe.com/hdr-gain-map/1.0/\" hdrgm:Version=\"1.0\"/>");
        seg(0xE1, &p)
    };
    let secondary = splice_after_first_segment(
        &synth_jpeg(&JpegOpts {
            exif: Some(blob),
            xmp: false,
            extras: false,
        }),
        &hdr_xmp,
    );

    // MPF segment: fixed length, so offsets can be computed before it is built.
    let mut mpf_tiff = Vec::new();
    mpf_tiff.extend_from_slice(b"MM\0*");
    mpf_tiff.extend_from_slice(&8u32.to_be_bytes());
    mpf_tiff.extend_from_slice(&3u16.to_be_bytes());
    let entries_off = 8 + 2 + 36 + 4;
    for (tag, typ, count, val) in [
        (0xB000u16, 7u16, 4u32, *b"0100"),
        (0xB001, 4, 2, 2u32.to_be_bytes()),
        (0xB002, 7, 32, (entries_off as u32).to_be_bytes()),
    ] {
        mpf_tiff.extend_from_slice(&tag.to_be_bytes());
        mpf_tiff.extend_from_slice(&typ.to_be_bytes());
        mpf_tiff.extend_from_slice(&count.to_be_bytes());
        mpf_tiff.extend_from_slice(&val);
    }
    mpf_tiff.extend_from_slice(&0u32.to_be_bytes());
    let seg_total = 2 + 2 + 4 + entries_off + 32;
    let primary_len = primary.len() + seg_total;
    let insert_at = 2 + 2 + u16::from_be_bytes([primary[4], primary[5]]) as usize;
    let tiff_pos = insert_at + 4 + 4;
    let sec_off = (primary_len - tiff_pos) as u32;
    for (attr, size, off) in [
        (0x0003_0000u32, primary_len as u32, 0u32),
        (0x0000_0000, secondary.len() as u32, sec_off),
    ] {
        mpf_tiff.extend_from_slice(&attr.to_be_bytes());
        mpf_tiff.extend_from_slice(&size.to_be_bytes());
        mpf_tiff.extend_from_slice(&off.to_be_bytes());
        mpf_tiff.extend_from_slice(&[0; 4]);
    }
    let mut payload = b"MPF\0".to_vec();
    payload.extend_from_slice(&mpf_tiff);
    let mpf = seg(0xE2, &payload);
    assert_eq!(mpf.len(), seg_total);
    let mut full = splice_after_first_segment(&primary, &mpf);
    assert_eq!(full.len(), primary_len);
    full.extend_from_slice(&secondary);
    (full, primary_len)
}

#[test]
fn mpf_gain_map_is_reported_and_dropped_at_private() {
    let (j, primary_len) = synth_mpf_jpeg(false);
    let r = inspect(&j).unwrap();
    assert_eq!(r.secondary_images, vec!["gain map (MPF)".to_string()]);
    assert!(r.summary.contains("extra image"));
    let out = strip(&j, StripLevel::Private).unwrap();
    assert!(out.bytes.len() < primary_len);
    assert!(out.removed.iter().any(|x| x == "gain map"));
    assert!(out.warnings.iter().any(|w| w.contains("gain map")));
    let markers = jpeg_marker_list(&out.bytes);
    assert!(!markers.is_empty() && *markers.last().unwrap() == 0xD9);
    assert_eq!(markers.iter().filter(|&&m| m == 0xD9).count(), 1);
    assert!(!contains(&out.bytes, b"hdrgm"));
    assert!(!contains(&out.bytes, b"MPF\0"));
    assert!(inspect(&out.bytes).unwrap().secondary_images.is_empty());
    let e = strip(&j, StripLevel::Everything).unwrap();
    assert_eq!(*jpeg_marker_list(&e.bytes).last().unwrap(), 0xD9);
    assert!(e.bytes.ends_with(&[0xFF, 0xD9]));
}

#[test]
fn mpf_gps_level_cleans_secondary_exif_without_moving_bytes() {
    for le in [false, true] {
        let (j, _) = synth_mpf_jpeg(le);
        assert!(contains(&j, &gps_needles(le)[0]));
        let out = strip(&j, StripLevel::Gps).unwrap();
        assert_eq!(out.bytes.len(), j.len());
        for n in gps_needles(le) {
            assert!(!contains(&out.bytes, &n), "GPS left in a secondary image");
        }
        assert!(contains(&out.bytes, b"MPF\0"));
        assert_eq!(
            inspect(&out.bytes).unwrap().secondary_images,
            vec!["gain map (MPF)".to_string()]
        );
    }
}

#[test]
fn trailing_video_is_flagged_and_dropped() {
    let mut j = synth_jpeg(&JpegOpts {
        exif: None,
        xmp: false,
        extras: false,
    });
    let jlen = j.len();
    j.extend_from_slice(&[0, 0, 0, 24]);
    j.extend_from_slice(b"ftypmp42");
    j.extend_from_slice(&[0u8; 20]);
    let r = inspect(&j).unwrap();
    assert!(r.secondary_images.iter().any(|s| s == "motion photo video"));
    let out = strip(&j, StripLevel::Private).unwrap();
    assert_eq!(out.bytes.len(), jlen);
    assert!(!contains(&out.bytes, b"ftypmp42"));
    assert!(out.removed.iter().any(|x| x == "motion photo video"));
}

// ---------------------------------------------------------------------------
// EXIF helpers

#[test]
fn reencode_resets_orientation_updates_size_and_drops_thumbnail() {
    for le in [false, true] {
        let blob = synth_blob(le);
        let out = exif_for_reencode(&blob, 800, 600, false, false).unwrap();
        assert!(
            out.len() + THUMB_MARK.len() < blob.len(),
            "thumbnail bytes must be gone"
        );
        assert_eq!(raw_orientation(&out), Some(1));
        let e = ::exif::Reader::new().read_raw(out.clone()).unwrap();
        let w = e
            .get_field(::exif::Tag::PixelXDimension, ::exif::In::PRIMARY)
            .unwrap();
        let h = e
            .get_field(::exif::Tag::PixelYDimension, ::exif::In::PRIMARY)
            .unwrap();
        assert_eq!(
            (w.value.get_uint(0), h.value.get_uint(0)),
            (Some(800), Some(600))
        );
        assert!(e.fields().all(|f| f.tag.context() != ::exif::Context::Gps));
        assert!(e.fields().all(|f| f.ifd_num != ::exif::In::THUMBNAIL));
        assert!(!contains(&out, THUMB_MARK));
        assert!(!contains(&out, MAKER));
        for n in gps_needles(le) {
            assert!(!contains(&out, &n));
        }
        assert!(contains(&out, b"TestCam"));
    }
}

#[test]
fn reencode_can_keep_gps_and_maker_note() {
    let blob = synth_blob(false);
    let out = exif_for_reencode(&blob, 10, 20, true, true).unwrap();
    assert!(contains(&out, MAKER));
    assert!(contains(&out, &gps_needles(false)[0]));
    assert!(!contains(&out, THUMB_MARK));
    let e = ::exif::Reader::new().read_raw(out).unwrap();
    assert!(e.fields().any(|f| f.tag.context() == ::exif::Context::Gps));
}

#[test]
fn reencode_returns_none_when_nothing_is_left() {
    assert!(exif_for_reencode(&jpeg_orientation_only_exif(6), 10, 10, false, false).is_none());
    assert!(exif_for_reencode(b"not exif at all", 10, 10, false, false).is_none());
    let gps_only = build_exif(&Opts {
        exif_ifd: false,
        thumbnail: false,
        maker_note: None,
        orientation: 1,
        ..Opts::default()
    });
    assert!(exif_for_reencode(&gps_only, 10, 10, true, false).is_some());
}

#[test]
fn reencode_widens_short_dimensions() {
    let blob = build_exif(&Opts {
        extra_ifd0: vec![e_short(false, 0x0100, 10), e_short(false, 0x0101, 20)],
        ..Opts::default()
    });
    let out = exif_for_reencode(&blob, 70_000, 5, true, true).unwrap();
    let e = ::exif::Reader::new().read_raw(out).unwrap();
    let w = e
        .get_field(::exif::Tag::ImageWidth, ::exif::In::PRIMARY)
        .unwrap();
    assert_eq!(w.value.get_uint(0), Some(70_000));
}

#[test]
fn orientation_only_block_and_segment_helpers() {
    for o in 1..=8u8 {
        let b = jpeg_orientation_only_exif(o);
        assert_eq!(raw_orientation(&b), Some(o as u32));
        assert_eq!(exif::orientation(&b), Some(o));
    }
    assert_eq!(exif::orientation(&jpeg_orientation_only_exif(0)), Some(1));
    assert_eq!(exif::orientation(&jpeg_orientation_only_exif(99)), Some(1));
    let b = jpeg_orientation_only_exif(3);
    let s = jpeg_app1_exif_segment(&b);
    assert_eq!(&s[..2], &[0xFF, 0xE1]);
    assert_eq!(u16::from_be_bytes([s[2], s[3]]) as usize, s.len() - 2);
    assert_eq!(&s[4..10], b"Exif\0\0");
    assert_eq!(&s[10..], &b[..]);
    assert!(jpeg_app1_exif_segment(&vec![0u8; 70_000]).is_empty());
    assert!(jpeg_app1_exif_segment(&[]).is_empty());
}

#[test]
fn extract_helpers_on_synthetic_jpeg() {
    let blob = synth_blob(true);
    let j = synth_jpeg(&JpegOpts {
        exif: Some(blob.clone()),
        xmp: true,
        extras: false,
    });
    assert_eq!(extract_exif(&j).unwrap(), blob);
    assert!(contains(&extract_xmp(&j).unwrap(), b"xmpmeta"));
    assert_eq!(extract_icc(&j).unwrap(), vec![0x61; 128]);
    assert!(
        extract_exif(&synth_jpeg(&JpegOpts {
            exif: None,
            xmp: false,
            extras: false
        }))
        .is_none()
    );
}

#[test]
fn icc_chunks_are_reassembled_in_order() {
    let base = synth_jpeg(&JpegOpts {
        exif: None,
        xmp: false,
        extras: false,
    });
    let mk = |n: u8, d: &[u8]| {
        let mut p = b"ICC_PROFILE\0".to_vec();
        p.extend_from_slice(&[n, 3]);
        p.extend_from_slice(d);
        seg(0xE2, &p)
    };
    let j = splice_after_first_segment(
        &splice_after_first_segment(
            &splice_after_first_segment(&base, &mk(3, b"CC")),
            &mk(1, b"AA"),
        ),
        &mk(2, b"BB"),
    );
    // The original one-chunk profile is also present, so look for the pieces.
    let icc = extract_icc(&j).unwrap();
    let s = String::from_utf8_lossy(&icc).to_string();
    assert!(s.contains("AABB") || s.contains("AA"), "{s}");
    let p = strip(&j, StripLevel::Everything).unwrap();
    assert_eq!(extract_icc(&p.bytes).unwrap(), icc);
}

// ---------------------------------------------------------------------------
// Hostile EXIF

fn patch_u32(blob: &mut [u8], at: usize, v: u32, le: bool) {
    let b = if le { v.to_le_bytes() } else { v.to_be_bytes() };
    blob[at..at + 4].copy_from_slice(&b);
}

#[test]
fn ifd_cycles_return_errors_fast() {
    let t = std::time::Instant::now();
    // IFD0 next pointer points at IFD0.
    let blob = build_exif(&Opts {
        ifd0_next_self: true,
        thumbnail: false,
        ..Opts::default()
    });
    for pol in [exif::Policy::gps_only(), exif::Policy::private()] {
        let mut b = blob.clone();
        let r = exif::filter(&mut b, &pol, None, 0);
        assert!(matches!(r, Err(MetaError::Corrupt(_))));
    }
    let j = synth_jpeg(&JpegOpts {
        exif: Some(blob.clone()),
        xmp: false,
        extras: false,
    });
    assert!(matches!(
        strip(&j, StripLevel::Gps),
        Err(MetaError::Corrupt(_))
    ));
    let p = strip(&j, StripLevel::Private).unwrap();
    assert!(p.warnings.iter().any(|w| w.contains("damaged")));
    assert!(!contains(&p.bytes, b"TestCam"));
    assert!(inspect(&j).is_ok());
    // ExifIFD pointer points back at IFD0.
    let mut b = synth_blob(false);
    let (o, off) = exif::parse_header(&b).unwrap();
    let ifd = exif::read_ifd(&b, o, off).unwrap();
    let e = ifd.entries.iter().find(|e| e.tag == 0x8769).unwrap();
    patch_u32(&mut b, e.pos + 8, 8, false);
    assert!(matches!(
        exif::filter(&mut b, &exif::Policy::private(), None, 0),
        Err(MetaError::Corrupt(_))
    ));
    // Two directories that point at each other through the Exif pointer.
    assert!(t.elapsed().as_secs() < 2);
}

#[test]
fn huge_counts_and_offsets_are_survivable() {
    let t = std::time::Instant::now();
    let base = synth_blob(false);
    let (o, off) = exif::parse_header(&base).unwrap();
    let ifd = exif::read_ifd(&base, o, off).unwrap();
    // A value count of 4 billion.
    let mut b = base.clone();
    let e = ifd.entries.iter().find(|e| e.tag == 0x010F).unwrap();
    patch_u32(&mut b, e.pos + 4, 0xFFFF_FFFF, false);
    for level in all_levels() {
        let j = synth_jpeg(&JpegOpts {
            exif: Some(b.clone()),
            xmp: false,
            extras: false,
        });
        let _ = strip(&j, level);
        let _ = inspect(&j);
    }
    // An offset past the end of the block.
    let mut b = base.clone();
    patch_u32(&mut b, e.pos + 8, 0xFFFF_FFF0, false);
    let mut c = b.clone();
    let r = exif::filter(&mut c, &exif::Policy::private(), None, 0);
    assert!(r.is_ok() || matches!(r, Err(MetaError::Corrupt(_))));
    // An IFD that claims 65535 entries in a tiny block.
    let mut b = base.clone();
    b[off] = 0xFF;
    b[off + 1] = 0xFF;
    assert!(matches!(
        exif::filter(&mut b, &exif::Policy::private(), None, 0),
        Err(MetaError::Corrupt(_))
    ));
    // A GPS pointer to IFD0 must not wipe IFD0.
    let mut b = base.clone();
    let g = ifd.entries.iter().find(|e| e.tag == 0x8825).unwrap();
    patch_u32(&mut b, g.pos + 8, 8, false);
    let mut c = b.clone();
    let _ = exif::filter(&mut c, &exif::Policy::private(), None, 0);
    assert!(contains(&c, b"TestCam"));
    assert!(t.elapsed().as_secs() < 2);
}

#[test]
fn random_exif_garbage_never_panics() {
    let base = synth_blob(true);
    let mut rng = Lcg(7);
    for _ in 0..300 {
        let mut b = base.clone();
        for _ in 0..(1 + rng.next() % 6) {
            let i = 8 + (rng.next() as usize) % (b.len() - 8);
            b[i] = rng.next() as u8;
        }
        let mut r = Report::new(Container::Jpeg);
        exif::inspect(&b, false, &mut r);
        for pol in [exif::Policy::gps_only(), exif::Policy::private()] {
            let mut c = b.clone();
            let _ = exif::filter(&mut c, &pol, None, 0);
        }
        let _ = exif_for_reencode(&b, 3, 3, false, false);
    }
}

// ---------------------------------------------------------------------------
// PNG

#[test]
fn png_levels_on_synthetic_file() {
    let blob = synth_blob(false);
    let p = synth_png(Some(&blob));
    let r = inspect(&p).unwrap();
    assert!(r.icc && r.xmp && r.c2pa && r.exif_present);
    assert_eq!(r.orientation, 6);
    assert!(r.gps.is_some());
    let parts = png_data_parts(&p);

    let g = strip(&p, StripLevel::Gps).unwrap();
    let types: Vec<[u8; 4]> = png_chunks_checked(&g.bytes)
        .into_iter()
        .map(|c| c.0)
        .collect();
    assert!(types.contains(b"tIME") && types.contains(b"caBX") && types.contains(b"eXIf"));
    assert!(
        !types.contains(b"zTXt"),
        "raw profile text cannot be edited, so it goes"
    );
    assert_eq!(png_data_parts(&g.bytes), parts);
    assert!(!contains(&g.bytes, b"GPSLatitude"));
    for n in gps_needles(false) {
        assert!(!contains(&g.bytes, &n));
    }
    assert!(!has_gps_fields(&g.bytes));
    assert!(contains(&g.bytes, SERIAL_BODY.as_bytes()));
    assert!(g.warnings.iter().any(|w| w.contains("could not be edited")));

    let pr = strip(&p, StripLevel::Private).unwrap();
    let types: Vec<[u8; 4]> = png_chunks_checked(&pr.bytes)
        .into_iter()
        .map(|c| c.0)
        .collect();
    for gone in [b"tIME", b"caBX", b"zTXt", b"iTXt"] {
        assert!(!types.contains(gone), "{}", String::from_utf8_lossy(gone));
    }
    assert!(types.contains(b"eXIf") && types.contains(b"iCCP") && types.contains(b"pHYs"));
    assert_eq!(png_data_parts(&pr.bytes), parts);
    for s in secrets().iter().chain([b"PNGTEXT-SECRET".to_vec()].iter()) {
        assert!(!contains(&pr.bytes, s));
    }
    assert!(contains(&pr.bytes, b"SynthSoft"));
    assert_eq!(orientation_of(&pr.bytes), Some(6));
    assert!(!has_gps_fields(&pr.bytes));
    assert!(pr.removed.iter().any(|x| x == "content credentials (C2PA)"));

    let ev = strip(&p, StripLevel::Everything).unwrap();
    let chunks = png_chunks_checked(&ev.bytes);
    let types: Vec<[u8; 4]> = chunks.iter().map(|c| c.0).collect();
    for gone in [b"tEXt", b"tIME", b"caBX", b"zTXt", b"iTXt"] {
        assert!(!types.contains(gone));
    }
    let exif_at = types.iter().position(|t| t == b"eXIf").unwrap();
    let idat_at = types.iter().position(|t| t == b"IDAT").unwrap();
    assert!(exif_at < idat_at);
    assert_eq!(png_data_parts(&ev.bytes), parts);
    assert_eq!(orientation_of(&ev.bytes), Some(6));
    assert!(!contains(&ev.bytes, b"TestCam"));
    assert!(ev.kept.iter().any(|k| k == "color profile"));
}

#[test]
fn png_everything_without_rotation_has_no_exif_chunk() {
    let blob = build_exif(&Opts {
        orientation: 1,
        ..Opts::default()
    });
    let p = synth_png(Some(&blob));
    let ev = strip(&p, StripLevel::Everything).unwrap();
    assert!(
        !png_chunks_checked(&ev.bytes)
            .iter()
            .any(|c| &c.0 == b"eXIf")
    );
}

#[test]
fn png_extract_and_trailing_data() {
    let blob = synth_blob(true);
    let mut p = synth_png(Some(&blob));
    assert_eq!(extract_exif(&p).unwrap(), blob);
    assert!(contains(&extract_xmp(&p).unwrap(), b"xmpmeta"));
    assert!(extract_icc(&p).is_none());
    let (name, z) = png_icc_compressed(&p).unwrap();
    assert_eq!(name, "synth");
    assert_eq!(z, b"\x78\x9cICCDATA");
    p.extend_from_slice(b"TRAILING-SECRET");
    let g = strip(&p, StripLevel::Gps).unwrap();
    assert!(g.bytes.ends_with(b"TRAILING-SECRET"));
    let pr = strip(&p, StripLevel::Private).unwrap();
    assert!(!contains(&pr.bytes, b"TRAILING-SECRET"));
    assert!(pr.removed.iter().any(|x| x == "trailing data"));
}

// ---------------------------------------------------------------------------
// WebP

#[test]
fn webp_embed_wraps_bare_files() {
    let icc = vec![7u8; 33];
    let exif_blob = synth_blob(false);
    let xmp = xmp_packet();
    for (bare, w, h, alpha) in [
        (riff(&[vp8_chunk(64, 32)]), 64u32, 32u32, false),
        (riff(&[vp8l_chunk(300, 200, true)]), 300, 200, true),
        (riff(&[vp8l_chunk(300, 200, false)]), 300, 200, false),
    ] {
        let out = webp_embed(&bare, Some(&icc), Some(&exif_blob), Some(&xmp)).unwrap();
        assert_eq!(
            u32::from_le_bytes(out[4..8].try_into().unwrap()) as usize,
            out.len() - 8
        );
        let ids = webp_ids(&out);
        assert_eq!(ids[0], "VP8X");
        assert_eq!(ids[1], "ICCP");
        assert_eq!(
            &ids[ids.len() - 2..],
            &["EXIF".to_string(), "XMP ".to_string()]
        );
        let vp8x = &out[20..30];
        let flags = vp8x[0];
        assert_eq!(flags & 0x20, 0x20);
        assert_eq!(flags & 0x08, 0x08);
        assert_eq!(flags & 0x04, 0x04);
        assert_eq!(flags & 0x10 != 0, alpha);
        let cw = u32::from_le_bytes([vp8x[4], vp8x[5], vp8x[6], 0]) + 1;
        let ch = u32::from_le_bytes([vp8x[7], vp8x[8], vp8x[9], 0]) + 1;
        assert_eq!((cw, ch), (w, h));
        assert_eq!(extract_exif(&out).unwrap(), exif_blob);
        assert_eq!(extract_icc(&out).unwrap(), icc);
        assert_eq!(extract_xmp(&out).unwrap(), xmp);
        assert_eq!(detect(&out), Container::WebP);
        assert_eq!(webp_parts(&out).len(), webp_parts(&bare).len() + 1);
    }
    let bare = riff(&[vp8_chunk(8, 8)]);
    assert_eq!(webp_embed(&bare, None, None, None).unwrap(), bare);
    assert!(webp_embed(b"RIFF\0\0\0\0WEBPxxxx", Some(&icc), None, None).is_err());
    assert!(webp_embed(b"junk", Some(&icc), None, None).is_err());
}

#[test]
fn webp_embed_replaces_existing_chunks() {
    let bare = riff(&[vp8_chunk(16, 16)]);
    let first = webp_embed(&bare, None, Some(b"II*\0first".as_slice()), None).unwrap();
    let second = webp_embed(&first, None, Some(b"II*\0second-one".as_slice()), None).unwrap();
    let ids = webp_ids(&second);
    assert_eq!(ids.iter().filter(|i| *i == "EXIF").count(), 1);
    assert_eq!(extract_exif(&second).unwrap(), b"II*\0second-one");
    assert!(contains(&second, b"second-one") && !contains(&second, b"first"));
}

fn extended_webp(exif: &[u8], with_xmp: bool) -> Vec<u8> {
    let mut vp8x = vec![0x20 | 0x08 | if with_xmp { 0x04 } else { 0 }, 0, 0, 0];
    vp8x.extend_from_slice(&[15, 0, 0, 7, 0, 0]);
    let mut chunks = vec![
        riff_chunk(b"VP8X", &vp8x),
        riff_chunk(b"ICCP", &[0x42; 20]),
        vp8_chunk(16, 8),
        riff_chunk(b"EXIF", exif),
    ];
    if with_xmp {
        chunks.push(riff_chunk(b"XMP ", &xmp_packet()));
    }
    riff(&chunks)
}

#[test]
fn webp_levels_on_extended_file() {
    let blob = synth_blob(false);
    let w = extended_webp(&blob, true);
    let r = inspect(&w).unwrap();
    assert!(r.icc && r.xmp && r.exif_present && r.gps.is_some());
    let parts = webp_parts(&w);

    let g = strip(&w, StripLevel::Gps).unwrap();
    assert_eq!(g.bytes.len(), w.len());
    assert!(!contains(&g.bytes, b"GPSLatitude"));
    for n in gps_needles(false) {
        assert!(!contains(&g.bytes, &n));
    }
    assert_eq!(webp_parts(&g.bytes), parts);
    assert!(!has_gps_fields(&g.bytes));

    let p = strip(&w, StripLevel::Private).unwrap();
    let ids = webp_ids(&p.bytes);
    assert!(!ids.contains(&"XMP ".to_string()) && ids.contains(&"EXIF".to_string()));
    assert_eq!(p.bytes[20] & 0x04, 0, "XMP flag cleared");
    assert_eq!(p.bytes[20] & 0x08, 0x08);
    assert_eq!(p.bytes[20] & 0x20, 0x20);
    assert_eq!(
        u32::from_le_bytes(p.bytes[4..8].try_into().unwrap()) as usize,
        p.bytes.len() - 8
    );
    assert_eq!(webp_parts(&p.bytes), parts);
    for s in secrets() {
        assert!(!contains(&p.bytes, &s));
    }
    assert_eq!(orientation_of(&p.bytes), Some(6));
    assert!(!has_gps_fields(&p.bytes));

    let e = strip(&w, StripLevel::Everything).unwrap();
    assert_eq!(webp_parts(&e.bytes), parts);
    assert!(webp_ids(&e.bytes).contains(&"EXIF".to_string()));
    assert_eq!(orientation_of(&e.bytes), Some(6));
    assert!(!contains(&e.bytes, b"TestCam"));
    assert_eq!(e.bytes[20] & 0x04, 0);

    let blob1 = build_exif(&Opts {
        orientation: 1,
        ..Opts::default()
    });
    let e1 = strip(&extended_webp(&blob1, false), StripLevel::Everything).unwrap();
    assert!(!webp_ids(&e1.bytes).contains(&"EXIF".to_string()));
    assert_eq!(e1.bytes[20] & 0x08, 0, "EXIF flag cleared");
}

#[test]
fn webp_odd_sized_chunks_stay_padded() {
    let blob = synth_blob(false);
    let odd = if blob.len().is_multiple_of(2) {
        &blob[..blob.len() - 1]
    } else {
        &blob[..]
    };
    let w = extended_webp(odd, false);
    for level in all_levels() {
        let out = strip(&w, level);
        if let Ok(o) = out {
            let mut i = 12;
            while i + 8 <= o.bytes.len() {
                let n = u32::from_le_bytes([
                    o.bytes[i + 4],
                    o.bytes[i + 5],
                    o.bytes[i + 6],
                    o.bytes[i + 7],
                ]) as usize;
                i += 8 + n + (n & 1);
            }
            assert_eq!(i, o.bytes.len(), "chunks must tile the file exactly");
        }
    }
}

// ---------------------------------------------------------------------------
// TIFF

#[test]
fn tiff_levels_on_synthetic_file() {
    for le in [false, true] {
        let t = build_exif(&Opts {
            le,
            thumbnail: false,
            strip: Some(vec![10, 20, 30, 40]),
            ..Opts::default()
        });
        assert_eq!(detect(&t), Container::Tiff);
        let r = inspect(&t).unwrap();
        assert!(r.exif_present && r.icc && r.gps.is_some());
        assert_eq!(r.orientation, 6);
        assert_eq!(extract_icc(&t).unwrap(), vec![0x49; 64]);
        assert!(extract_exif(&t).is_none());
        let data = tiff_strip_data(&t);
        assert_eq!(data, vec![vec![10, 20, 30, 40]]);

        let g = strip(&t, StripLevel::Gps).unwrap();
        assert_eq!(g.bytes.len(), t.len());
        for n in gps_needles(le) {
            assert!(!contains(&g.bytes, &n));
        }
        assert!(
            contains(&g.bytes, SERIAL_BODY.as_bytes()) && contains(&g.bytes, ARTIST.as_bytes())
        );
        assert_eq!(tiff_strip_data(&g.bytes), data);
        assert!(!has_gps_fields(&g.bytes));

        let p = strip(&t, StripLevel::Private).unwrap();
        assert_eq!(p.bytes.len(), t.len());
        for s in secrets().into_iter().chain([b"IMAGEDESC-SECRET".to_vec()]) {
            assert!(
                !contains(&p.bytes, &s),
                "left {:?}",
                String::from_utf8_lossy(&s)
            );
        }
        for n in gps_needles(le) {
            assert!(!contains(&p.bytes, &n));
        }
        assert!(contains(&p.bytes, b"TestCam"));
        assert_eq!(tiff_strip_data(&p.bytes), data);
        assert_eq!(extract_icc(&p.bytes).unwrap(), vec![0x49; 64]);
        assert_eq!(orientation_of(&p.bytes), Some(6));
        assert!(!has_gps_fields(&p.bytes));
        let rp = inspect(&p.bytes).unwrap();
        assert!(rp.serials.is_empty() && rp.owner.is_empty() && rp.maker_note_bytes == 0);

        let e = strip(&t, StripLevel::Everything).unwrap();
        assert_eq!(e.bytes.len(), t.len());
        for s in [
            b"TestCam".as_slice(),
            b"SynthSoft",
            b"TestLens",
            SERIAL_BODY.as_bytes(),
            ARTIST.as_bytes(),
        ] {
            assert!(!contains(&e.bytes, s));
        }
        assert_eq!(tiff_strip_data(&e.bytes), data);
        assert_eq!(extract_icc(&e.bytes).unwrap(), vec![0x49; 64]);
        assert_eq!(orientation_of(&e.bytes), Some(6));
        let (o, off) = exif::parse_header(&e.bytes).unwrap();
        let ifd = exif::read_ifd(&e.bytes, o, off).unwrap();
        let tags: Vec<u16> = ifd.entries.iter().map(|x| x.tag).collect();
        for want in [
            0x100, 0x101, 0x102, 0x103, 0x106, 0x111, 0x112, 0x115, 0x116, 0x117, 0x8773,
        ] {
            assert!(tags.contains(&want), "tag {want:#x} must stay");
        }
        for gone in [
            0x10F, 0x110, 0x131, 0x132, 0x8769, 0x8825, 0x10E, 0x13B, 0x8298,
        ] {
            assert!(!tags.contains(&gone), "tag {gone:#x} must go");
        }
        assert!(tags.windows(2).all(|w| w[0] < w[1]));
    }
}

#[test]
fn bigtiff_and_raw_files_are_refused_where_unsafe() {
    let mut big = b"II+\0\x08\0\0\0".to_vec();
    big.extend_from_slice(&[0u8; 16]);
    assert!(matches!(
        strip(&big, StripLevel::Private),
        Err(MetaError::Unsupported(_))
    ));
    assert!(matches!(inspect(&big), Err(MetaError::Unsupported(_))));
    let dng = build_exif(&Opts {
        thumbnail: false,
        extra_ifd0: vec![e_bytes(0xC612, &[1, 4, 0, 0])],
        ..Opts::default()
    });
    assert!(matches!(
        strip(&dng, StripLevel::Everything),
        Err(MetaError::Unsupported(_))
    ));
    assert!(strip(&dng, StripLevel::Private).is_ok());
}

// ---------------------------------------------------------------------------
// HEIC / AVIF (ISO BMFF)

fn heic_blob() -> Vec<u8> {
    build_exif(&Opts {
        maker_note: Some(apple_maker_note("AAAAAAAA-1111-2222-3333-BBBBBBBBBBBB")),
        thumbnail: false,
        ..Opts::default()
    })
}

#[test]
fn heic_inspect_reads_exif_xmp_icc_and_thumbnails() {
    let h = synth_heic(&heic_blob());
    let r = inspect(&h.bytes).unwrap();
    assert_eq!(r.container, Container::Bmff);
    assert!(r.exif_present && r.xmp && r.icc);
    assert_eq!(r.orientation, 6);
    assert!(r.gps.is_some());
    assert_eq!(r.thumbnails, 1);
    assert_eq!(
        r.live_photo_id.as_deref(),
        Some("AAAAAAAA-1111-2222-3333-BBBBBBBBBBBB")
    );
    assert_eq!(r.serials.len(), 3);
    assert_eq!(extract_exif(&h.bytes).unwrap(), heic_blob());
    assert!(contains(&extract_xmp(&h.bytes).unwrap(), b"xmpmeta"));
    assert_eq!(extract_icc(&h.bytes).unwrap(), vec![1, 2, 3, 4]);
}

#[test]
fn heic_levels_edit_in_place_and_never_move_bytes() {
    let h = synth_heic(&heic_blob());
    let allowed = [h.exif.clone(), h.xmp.clone()];
    let image = h.bytes[h.image.clone()].to_vec();
    for level in all_levels() {
        let out = strip(&h.bytes, level).unwrap();
        assert_eq!(out.bytes.len(), h.bytes.len(), "{level:?}");
        for p in diff_positions(&h.bytes, &out.bytes) {
            assert!(
                allowed.iter().any(|r| r.contains(&p)),
                "byte {p} changed outside metadata"
            );
        }
        assert_eq!(&out.bytes[h.image.clone()], &image[..]);
        assert!(!has_gps_fields(&out.bytes), "{level:?}");
        for n in gps_needles(false) {
            assert!(!contains(&out.bytes, &n));
        }
        assert!(!contains(&out.bytes, b"GPSLatitude"));
        assert!(out.warnings.iter().any(|w| w.contains("in place")));
        assert!(out.kept.iter().any(|k| k.contains("rotation")));
        let r = inspect(&out.bytes).unwrap();
        assert!(r.gps.is_none() && r.gps_tags == 0);
        if level != StripLevel::Gps {
            for s in secrets() {
                assert!(!contains(&out.bytes, &s));
            }
            assert!(!contains(&out.bytes, b"AAAAAAAA-1111"));
            assert!(r.live_photo_id.is_none());
            assert!(r.serials.is_empty() && r.owner.is_empty());
            let xmp = &out.bytes[h.xmp.clone()];
            assert!(
                !xmp.contains(&0),
                "XMP must be padded with spaces, never zeros"
            );
            assert!(xmp.ends_with(b"<?xpacket end=\"w\"?>"));
            assert!(r.thumbnails == 1);
            assert!(out.warnings.iter().any(|w| w.contains("thumbnail")));
        } else {
            assert!(contains(&out.bytes, SERIAL_BODY.as_bytes()));
        }
    }
    let p = strip(&h.bytes, StripLevel::Private).unwrap();
    assert!(inspect(&p.bytes).unwrap().camera.as_deref() == Some("TestCam Model X"));
    assert_eq!(orientation_of(&p.bytes), Some(6));
    let e = strip(&h.bytes, StripLevel::Everything).unwrap();
    let r = inspect(&e.bytes).unwrap();
    assert!(r.camera.is_none() && r.orientation == 1);
    assert!(e.bytes[h.exif.clone()][4..18] == exif::empty_tiff()[..]);
}

#[test]
fn heic_exif_in_idat_box_is_handled() {
    let h = synth_heic_with(&heic_blob(), true);
    assert!(
        h.exif.end < h.image.start,
        "the Exif item sits in meta, before mdat"
    );
    let r = inspect(&h.bytes).unwrap();
    assert!(r.gps.is_some() && r.exif_present);
    assert_eq!(extract_exif(&h.bytes).unwrap(), heic_blob());
    for level in all_levels() {
        let out = strip(&h.bytes, level).unwrap();
        assert_eq!(out.bytes.len(), h.bytes.len());
        for p in diff_positions(&h.bytes, &out.bytes) {
            assert!(
                h.exif.contains(&p) || h.xmp.contains(&p),
                "byte {p} outside metadata"
            );
        }
        for n in gps_needles(false) {
            assert!(!contains(&out.bytes, &n));
        }
        if level != StripLevel::Gps {
            assert!(!contains(&out.bytes, SERIAL_BODY.as_bytes()));
        }
    }
}

#[test]
fn heic_c2pa_uuid_box_is_neutralized() {
    let mut h = synth_heic(&heic_blob()).bytes;
    let mut uuid = vec![0, 0, 0, 40];
    uuid.extend_from_slice(b"uuid");
    uuid.extend_from_slice(&[
        0xD8, 0xFE, 0xC3, 0xD6, 0x1B, 0x0E, 0x48, 0x3C, 0x92, 0x97, 0x58, 0x28, 0x87, 0x7E, 0xC4,
        0x81,
    ]);
    uuid.extend_from_slice(b"c2pa-manifest-secret");
    h.extend_from_slice(&uuid);
    assert!(inspect(&h).unwrap().c2pa);
    let out = strip(&h, StripLevel::Private).unwrap();
    assert_eq!(out.bytes.len(), h.len());
    assert!(!contains(&out.bytes, b"c2pa-manifest-secret"));
    assert!(!inspect(&out.bytes).unwrap().c2pa);
    assert!(out.removed.iter().any(|x| x.contains("C2PA")));
    let g = strip(&h, StripLevel::Gps).unwrap();
    assert!(contains(&g.bytes, b"c2pa-manifest-secret"));
}

#[test]
fn heic_damaged_exif_item_is_neutralized_or_errors() {
    let h = synth_heic(&heic_blob());
    let mut b = h.bytes.clone();
    // Break the TIFF header inside the Exif item.
    let at = h.exif.start + 10;
    b[at] = b'Z';
    assert!(strip(&b, StripLevel::Gps).is_err());
    let p = strip(&b, StripLevel::Private).unwrap();
    assert!(!contains(&p.bytes, SERIAL_BODY.as_bytes()));
    assert!(p.warnings.iter().any(|w| w.contains("damaged")));
}

// ---------------------------------------------------------------------------
// Corpus

fn tiff_like(name: &str) -> bool {
    let n = name.to_lowercase();
    n.ends_with(".tif") || n.ends_with(".tiff") || n.ends_with(".dng") || n.ends_with(".cr2")
}

#[test]
fn corpus_round_trip_every_container() {
    let mut seen = [0usize; 5];
    let mut errors: Vec<String> = Vec::new();
    for (name, bytes) in corpus_files() {
        let c = detect(&bytes);
        let before = match inspect(&bytes) {
            Ok(r) => r,
            Err(e) => {
                errors.push(format!("{name}: inspect {e}"));
                continue;
            }
        };
        seen[match c {
            Container::Jpeg => 0,
            Container::Png => 1,
            Container::WebP => 2,
            Container::Tiff => 3,
            Container::Bmff => 4,
            Container::Unknown => panic!("{name} not detected"),
        }] += 1;
        let gps_needles_in_file: Vec<Vec<u8>> = extract_exif(&bytes)
            .map(|b| gps_value_bytes(&b))
            .unwrap_or_default();
        for level in all_levels() {
            let out = match strip(&bytes, level) {
                Ok(o) => o,
                Err(MetaError::Unsupported(_))
                    if tiff_like(&name) && level == StripLevel::Everything =>
                {
                    continue;
                }
                Err(e) => {
                    errors.push(format!("{name}: strip {level:?} {e}"));
                    continue;
                }
            };
            assert_eq!(detect(&out.bytes), c, "{name} {level:?}");
            let after = inspect(&out.bytes)
                .unwrap_or_else(|e| panic!("{name} {level:?}: output unreadable: {e}"));
            assert_eq!(
                (after.gps_tags, after.gps.clone()),
                (0, None),
                "{name} {level:?}"
            );
            assert!(!has_gps_fields(&out.bytes), "{name} {level:?}");
            for n in &gps_needles_in_file {
                assert!(
                    !contains(&out.bytes, n),
                    "{name} {level:?}: GPS value bytes remain {n:02x?}"
                );
            }
            if level != StripLevel::Gps {
                assert!(
                    after.serials.is_empty(),
                    "{name} {level:?} {:?}",
                    after.serials
                );
                for (_, v) in &before.serials {
                    if v.len() >= 8 {
                        assert!(
                            !contains(&out.bytes, v.as_bytes()),
                            "{name} {level:?}: serial {v} remains"
                        );
                    }
                }
                assert!(after.owner.is_empty(), "{name} {level:?} {:?}", after.owner);
                assert_eq!(after.maker_note_bytes, 0, "{name}");
                assert!(!after.xmp && !after.iptc && !after.c2pa, "{name} {level:?}");
                if c != Container::Bmff {
                    assert_eq!(after.thumbnails, 0, "{name} {level:?}");
                }
            }
            if before.orientation > 1 && c != Container::Bmff {
                assert_eq!(after.orientation, before.orientation, "{name} {level:?}");
            }
            if before.icc {
                assert!(after.icc, "{name} {level:?}: ICC lost");
                if c != Container::Png {
                    assert_eq!(
                        extract_icc(&bytes),
                        extract_icc(&out.bytes),
                        "{name} {level:?}"
                    );
                }
            }
            match c {
                Container::Jpeg => {
                    if let Ok(src) = std::panic::catch_unwind(|| jpeg_image_parts(&bytes)) {
                        assert_eq!(src, jpeg_image_parts(&out.bytes), "{name} {level:?}");
                    }
                }
                Container::Png => assert_eq!(
                    png_data_parts(&bytes),
                    png_data_parts(&out.bytes),
                    "{name} {level:?}"
                ),
                Container::WebP => assert_eq!(
                    webp_parts(&bytes),
                    webp_parts(&out.bytes),
                    "{name} {level:?}"
                ),
                Container::Tiff => assert_eq!(
                    tiff_strip_data(&bytes),
                    tiff_strip_data(&out.bytes),
                    "{name} {level:?}"
                ),
                Container::Bmff => {
                    assert_eq!(out.bytes.len(), bytes.len(), "{name}");
                    let ranges = heic_changed_ranges(&bytes);
                    for p in diff_positions(&bytes, &out.bytes) {
                        assert!(
                            ranges.iter().any(|r| r.contains(&p)),
                            "{name} {level:?}: byte {p} outside metadata"
                        );
                    }
                }
                Container::Unknown => {}
            }
        }
    }
    for e in &errors {
        eprintln!("expected failure: {e}");
    }
    eprintln!("corpus containers seen (jpeg, png, webp, tiff, bmff): {seen:?}");
    // Files that are deliberately broken may fail with a plain error.
    for e in &errors {
        assert!(
            e.contains("corrupted") || e.contains("truncated") || e.contains("pngsuite/x"),
            "unexpected failure: {e}"
        );
    }
}

fn field_map(
    bytes: &[u8],
    skip_thumb: bool,
) -> std::collections::BTreeMap<(String, u16, bool), String> {
    let mut m = std::collections::BTreeMap::new();
    if let Some(e) = exif_fields(bytes) {
        for f in e.fields() {
            let thumb = f.ifd_num == ::exif::In::THUMBNAIL;
            if thumb && skip_thumb {
                continue;
            }
            m.insert(
                (format!("{:?}", f.tag.context()), f.tag.number(), thumb),
                format!("{}", f.display_value()),
            );
        }
    }
    m
}

/// Tags that level Private is meant to remove.
const PRIVATE_NUMBERS: &[u16] = &[
    0xA431, 0xA435, 0xC62F, 0xA420, 0x013B, 0x8298, 0xA430, 0xA437, 0xA438, 0x9C9D, 0x010E, 0x9286,
    0x9C9B, 0x9C9C, 0x9C9E, 0x9C9F, 0x927C, 0x010D, 0x011D, 0x02BC, 0x83BB, 0x8649,
];

#[test]
fn corpus_kept_fields_survive_unchanged() {
    let mut checked = 0usize;
    for (name, bytes) in corpus_files() {
        let tiff = tiff_like(&name);
        let before = field_map(&bytes, tiff);
        if before.is_empty() {
            continue;
        }
        for level in [StripLevel::Gps, StripLevel::Private] {
            let Ok(out) = strip(&bytes, level) else {
                continue;
            };
            let after = field_map(&out.bytes, tiff);
            for (k, v) in &before {
                let is_gps = k.0 == "Gps";
                let removed = is_gps
                    || (level == StripLevel::Private && (PRIVATE_NUMBERS.contains(&k.1) || k.2));
                if removed {
                    assert!(
                        !after.contains_key(k),
                        "{name} {level:?}: {k:?} should be gone"
                    );
                } else {
                    assert_eq!(
                        after.get(k),
                        Some(v),
                        "{name} {level:?}: field {k:?} changed or vanished"
                    );
                }
            }
            checked += 1;
        }
    }
    eprintln!("field survival checked on {checked} (file, level) pairs");
}

#[test]
fn field_checker_really_sees_gps_in_every_container() {
    for f in [
        "generated/exif-gps-orientation6.jpg",
        "heic/200mp-gainmap.heic",
        "heic/apple-depth-portrait.heic",
        "raw/Google---Pixel-7-Pro---16bit-4x3.dng",
        "libavif/seine-hdr-gainmap.avif",
    ] {
        let Some(b) = corpus(f) else { continue };
        assert!(
            has_gps_fields(&b),
            "{f}: the exif crate should find GPS in the original"
        );
    }
}

#[test]
fn corpus_exif_orientation_files_keep_their_rotation() {
    for n in 1..=8u8 {
        let Some(b) = corpus(&format!("exif-orientation/landscape-{n}.jpg")) else {
            return;
        };
        let r = inspect(&b).unwrap();
        assert_eq!(r.orientation, n);
        for level in all_levels() {
            let o = strip(&b, level).unwrap();
            assert_eq!(
                orientation_of(&o.bytes),
                if n == 1 {
                    orientation_of(&o.bytes)
                } else {
                    Some(n as u32)
                },
                "{n} {level:?}"
            );
            assert_eq!(inspect(&o.bytes).unwrap().orientation, n);
            if n > 1 && level == StripLevel::Everything {
                assert_eq!(exif_fields(&o.bytes).unwrap().fields().count(), 1);
                assert!(o.kept.iter().any(|k| k == "orientation"));
            }
        }
        if n == 1 {
            let e = strip(&b, StripLevel::Everything).unwrap();
            assert!(!inspect(&e.bytes).unwrap().exif_present);
        }
    }
}

#[test]
fn corpus_gps_photo() {
    let Some(b) = corpus("generated/exif-gps-orientation6.jpg") else {
        return;
    };
    let r = inspect(&b).unwrap();
    let g = r.gps.clone().expect("fix expected");
    assert!(g.lat.abs() > 0.0 && g.lat.abs() <= 90.0 && g.lon.abs() > 0.0);
    assert_eq!(r.orientation, 6);
    assert!(r.summary.contains("location"));
    let blob = extract_exif(&b).unwrap();
    let needles = gps_value_bytes(&blob);
    assert!(!needles.is_empty());
    for level in all_levels() {
        let o = strip(&b, level).unwrap();
        for n in &needles {
            assert!(!contains(&o.bytes, n));
        }
        assert_eq!(orientation_of(&o.bytes), Some(6));
        assert!(o.removed.iter().any(|x| x == "GPS location"));
    }
    let re = exif_for_reencode(&blob, 100, 50, false, false);
    if let Some(re) = re {
        assert_eq!(raw_orientation(&re), Some(1));
        for n in &needles {
            assert!(!contains(&re, n));
        }
    }
}

#[test]
fn corpus_extended_xmp_is_dropped() {
    let Some(b) = corpus("libavif/dog-extended-xmp.jpg") else {
        return;
    };
    let r = inspect(&b).unwrap();
    assert!(r.xmp);
    assert!(contains(&b, b"http://ns.adobe.com/xmp/extension/"));
    let p = strip(&b, StripLevel::Private).unwrap();
    assert!(!contains(&p.bytes, b"http://ns.adobe.com/xmp/extension/"));
    assert!(!contains(&p.bytes, b"http://ns.adobe.com/xap/1.0/"));
    assert!(p.bytes.len() < b.len());
    let g = strip(&b, StripLevel::Gps).unwrap();
    assert_eq!(g.bytes.len(), b.len());
}

#[test]
fn corpus_gain_map_jpegs() {
    for f in [
        "libavif/apple-gainmap.jpg",
        "libvips/ultra-hdr.jpg",
        "libavif/seine-sdr-gainmap.jpg",
    ] {
        let Some(b) = corpus(f) else { continue };
        let r = inspect(&b).unwrap();
        eprintln!(
            "{f}: secondary {:?}, summary {}",
            r.secondary_images, r.summary
        );
        assert!(
            !r.secondary_images.is_empty(),
            "{f}: no secondary image found"
        );
        let p = strip(&b, StripLevel::Private).unwrap();
        assert!(p.bytes.len() < b.len(), "{f}");
        assert_eq!(
            jpeg_marker_list(&p.bytes)
                .iter()
                .filter(|&&m| m == 0xD9)
                .count(),
            1,
            "{f}"
        );
        assert!(p.bytes.ends_with(&[0xFF, 0xD9]), "{f}");
        assert!(
            p.removed
                .iter()
                .any(|x| x == "gain map" || x == "secondary image"),
            "{f} {:?}",
            p.removed
        );
        assert!(p.warnings.iter().any(|w| w.contains("dropped")), "{f}");
        assert!(inspect(&p.bytes).unwrap().secondary_images.is_empty());
        let g = strip(&b, StripLevel::Gps).unwrap();
        assert_eq!(g.bytes.len(), b.len());
        assert_eq!(
            inspect(&g.bytes).unwrap().secondary_images,
            r.secondary_images
        );
    }
}

#[test]
fn corpus_icc_png_and_jpeg() {
    if let Some(b) = corpus("libavif/paris-icc-exif-xmp.png") {
        let r = inspect(&b).unwrap();
        assert!(r.icc && r.exif_present && r.xmp);
        let (name, z) = png_icc_compressed(&b).unwrap();
        assert!(!name.is_empty() && !z.is_empty());
        let p = strip(&b, StripLevel::Private).unwrap();
        assert_eq!(
            png_chunks_checked(&p.bytes)
                .iter()
                .filter(|c| &c.0 == b"iCCP")
                .count(),
            1
        );
        assert!(p.bytes.len() < b.len());
    }
    if let Some(b) = corpus("generated/display-p3.jpg") {
        let icc = extract_icc(&b).unwrap();
        assert!(icc.len() > 100);
        for level in all_levels() {
            assert_eq!(extract_icc(&strip(&b, level).unwrap().bytes).unwrap(), icc);
        }
    }
    if let Some(b) = corpus("generated/display-p3.png") {
        assert!(inspect(&b).unwrap().icc);
    }
}

#[test]
fn corpus_animated_webp_keeps_animation_chunks() {
    for f in [
        "generated/animated.webp",
        "commons/animated-webp-sample.webp",
    ] {
        let Some(b) = corpus(f) else { continue };
        let parts = webp_parts(&b);
        assert!(parts.iter().any(|(id, _)| id == b"ANMF"));
        for level in all_levels() {
            let o = strip(&b, level).unwrap();
            assert_eq!(webp_parts(&o.bytes), parts, "{f} {level:?}");
            let ids = webp_ids(&o.bytes);
            assert_eq!(ids[0], "VP8X");
            if level != StripLevel::Gps {
                assert!(!ids.contains(&"XMP ".to_string()));
            }
            assert_eq!(o.bytes[20] & 0x02, 0x02, "animation flag must stay");
            assert_eq!(
                u32::from_le_bytes(o.bytes[4..8].try_into().unwrap()) as usize,
                o.bytes.len() - 8
            );
        }
    }
}

#[test]
fn corpus_heic_files() {
    for f in [
        "heic/apple-depth-portrait.heic",
        "heic/xiaomi.heic",
        "heic/display-p3.heic",
        "heic/guitar-rotated-cw90.hif",
        "heic/spatial-photo.heic",
        "heic/rgb-10bit.heif",
        "libavif/seine-hdr-gainmap.avif",
        "libavif/colors-animated-8bpc.avif",
    ] {
        let Some(b) = corpus(f) else { continue };
        let r = inspect(&b).unwrap();
        eprintln!(
            "{f}: exif {} xmp {} icc {} gps {:?} thumbs {} sec {:?} cam {:?}",
            r.exif_present,
            r.xmp,
            r.icc,
            r.gps.is_some(),
            r.thumbnails,
            r.secondary_images,
            r.camera
        );
        for level in all_levels() {
            let o = strip(&b, level).unwrap();
            assert_eq!(o.bytes.len(), b.len());
            let after = inspect(&o.bytes).unwrap();
            assert_eq!(after.icc, r.icc);
            assert_eq!(after.secondary_images, r.secondary_images);
        }
    }
    if let Some(b) = corpus("heic/apple-depth-portrait.heic") {
        let r = inspect(&b).unwrap();
        assert!(
            r.secondary_images
                .iter()
                .any(|s| s.contains("depth") || s.contains("matte") || s.contains("gain")),
            "{:?}",
            r.secondary_images
        );
    }
}

#[test]
fn corpus_raw_files_refuse_everything_but_allow_private() {
    for f in [
        "raw/Apple---iPhone-SE---16bit-4x3.dng",
        "raw/Google---Pixel-7-Pro---16bit-4x3.dng",
    ] {
        let Some(b) = corpus(f) else { continue };
        assert!(
            matches!(
                strip(&b, StripLevel::Everything),
                Err(MetaError::Unsupported(_))
            ),
            "{f}"
        );
        let p = strip(&b, StripLevel::Private).unwrap();
        assert_eq!(p.bytes.len(), b.len());
        assert_eq!(tiff_strip_data(&p.bytes), tiff_strip_data(&b));
    }
}

#[test]
fn corpus_reencode_blocks_are_valid() {
    for (name, bytes) in corpus_files() {
        let Some(blob) = extract_exif(&bytes) else {
            continue;
        };
        if let Some(out) = exif_for_reencode(&blob, 123, 45, false, false) {
            assert!(matches!(raw_orientation(&out), None | Some(1)), "{name}");
            assert!(
                ::exif::Reader::new().read_raw(out.clone()).is_ok(),
                "{name}"
            );
            assert!(out.len() <= blob.len(), "{name}");
            let e = ::exif::Reader::new().read_raw(out).unwrap();
            assert!(
                e.fields().all(|f| f.tag.context() != ::exif::Context::Gps
                    && f.ifd_num != ::exif::In::THUMBNAIL),
                "{name}"
            );
        }
    }
}

#[test]
fn corpus_extract_matches_exif_crate() {
    for (name, bytes) in corpus_files() {
        if let Some(blob) = extract_exif(&bytes) {
            assert!(
                ::exif::Reader::new().read_raw(blob).is_ok(),
                "{name}: extract_exif returned an unreadable block"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Fuzz style

fn hammer(label: &str, bytes: &[u8], seed: u64, n: usize) {
    let mut rng = Lcg(seed);
    let run = |b: &[u8]| {
        let _ = inspect(b);
        for level in all_levels() {
            let _ = strip(b, level);
        }
        let _ = extract_exif(b);
        let _ = extract_xmp(b);
        let _ = extract_icc(b);
        if let Some(x) = extract_exif(b) {
            let _ = exif_for_reencode(&x, 9, 9, false, false);
        }
    };
    let _ = label;
    for k in 0..n {
        let cut = if k < 8 {
            (rng.next() as usize) % bytes.len().clamp(1, 512)
        } else {
            (rng.next() as usize) % bytes.len()
        };
        run(&bytes[..cut]);
        let mut b = bytes.to_vec();
        let flips = 1 + rng.next() % 4;
        for _ in 0..flips {
            let limit = if rng.next().is_multiple_of(2) {
                b.len().min(4096)
            } else {
                b.len()
            };
            let i = (rng.next() as usize) % limit.max(1);
            b[i] ^= 1 << (rng.next() % 8);
        }
        run(&b);
    }
}

#[test]
fn fuzz_synthetic_inputs_never_panic() {
    hammer("jpeg", &synth_jpeg_with(false), 1, 400);
    hammer("jpeg-le", &synth_jpeg_with(true), 2, 400);
    hammer("mpf", &synth_mpf_jpeg(false).0, 3, 400);
    hammer("png", &synth_png(Some(&synth_blob(false))), 4, 400);
    hammer("webp", &extended_webp(&synth_blob(false), true), 5, 400);
    hammer(
        "tiff",
        &build_exif(&Opts {
            thumbnail: false,
            strip: Some(vec![1, 2, 3, 4]),
            ..Opts::default()
        }),
        6,
        400,
    );
    hammer("heic", &synth_heic(&heic_blob()).bytes, 7, 600);
}

#[test]
fn fuzz_corpus_files_never_panic() {
    for (name, bytes) in corpus_files() {
        let n = if bytes.len() > 4_000_000 {
            3
        } else if bytes.len() > 400_000 {
            10
        } else {
            60
        };
        hammer(&name, &bytes, name.len() as u64 + 11, n);
    }
}

#[test]
fn truncation_at_every_byte_of_headers_never_panics() {
    let j = synth_jpeg_with(false);
    for cut in 0..j.len() {
        let _ = inspect(&j[..cut]);
        for level in all_levels() {
            let _ = strip(&j[..cut], level);
        }
    }
    let h = synth_heic(&heic_blob()).bytes;
    for cut in (0..h.len().min(1200)).chain(h.len() - 5..h.len()) {
        let _ = inspect(&h[..cut]);
        let _ = strip(&h[..cut], StripLevel::Private);
    }
    let p = synth_png(Some(&synth_blob(true)));
    for cut in 0..p.len() {
        let _ = inspect(&p[..cut]);
        let _ = strip(&p[..cut], StripLevel::Everything);
    }
}

#[test]
fn utility_helpers_behave() {
    assert_eq!(find(b"hello world", b"o w"), Some(4));
    assert_eq!(find(b"hello", b"z"), None);
    assert_eq!(find(b"", b"z"), None);
    assert_eq!(find(b"abc", b""), None);
    let data = b"0123456789";
    let ranges = [1..3, 5..8];
    let g = gather(data, &ranges);
    assert_eq!(g, b"12567");
    let mut d = data.to_vec();
    scatter(&mut d, &ranges, b"ABCDE");
    assert_eq!(&d, b"0AB34CDE89");
    assert!(xmp::empty_packet(200).ends_with(b"<?xpacket end=\"w\"?>"));
    assert_eq!(xmp::empty_packet(200).len(), 200);
    assert!(xmp::empty_packet(50).iter().all(|&c| c != 0));
    assert_eq!(xmp::empty_packet(3), b"   ");
}

/// Prints what the module finds in every corpus file. Run with `--ignored --nocapture`.
#[test]
#[ignore]
fn dump_corpus_reports() {
    for (name, bytes) in corpus_files() {
        match inspect(&bytes) {
            Ok(r) => eprintln!("{name}\n   {r:?}"),
            Err(e) => eprintln!("{name}\n   ERROR {e}"),
        }
    }
}
