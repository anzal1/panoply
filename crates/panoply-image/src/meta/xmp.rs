//! XMP packet helpers. XMP is text, so this works on bytes without a parser.

use super::{Gps, Report};

fn is_name(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.')
}

/// Overwrite every GPS property in an XMP packet with spaces, keeping the
/// packet length. Handles `exif:GPSLatitude="..."` attributes and
/// `<exif:GPSLatitude>...</exif:GPSLatitude>` elements, and vendor variants
/// whose name starts with `GPS`, such as `drone-dji:GpsLatitude`.
/// Returns how many properties were blanked.
pub(crate) fn blank_gps(b: &mut [u8]) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i + 4 <= b.len() {
        if b[i] == b':' && b[i + 1..i + 4].eq_ignore_ascii_case(b"gps") {
            let mut s = i;
            while s > 0 && is_name(b[s - 1]) {
                s -= 1;
            }
            let mut e = i + 1;
            while e < b.len() && is_name(b[e]) {
                e += 1;
            }
            if s < i {
                let before = if s > 0 { b[s - 1] } else { 0 };
                if before == b'<' {
                    if let Some(gt) = b[e..].iter().position(|&c| c == b'>') {
                        let gt = e + gt;
                        if b[gt - 1] == b'/' {
                            b[s - 1..=gt].fill(b' ');
                            count += 1;
                            i = gt + 1;
                            continue;
                        }
                        let mut close = Vec::with_capacity(e - s + 3);
                        close.extend_from_slice(b"</");
                        close.extend_from_slice(&b[s..e]);
                        close.push(b'>');
                        if let Some(c) = super::find(&b[gt + 1..], &close) {
                            let end = gt + 1 + c + close.len();
                            b[s - 1..end].fill(b' ');
                            count += 1;
                            i = end;
                            continue;
                        }
                    }
                } else if before.is_ascii_whitespace() {
                    let mut p = e;
                    while p < b.len() && b[p].is_ascii_whitespace() {
                        p += 1;
                    }
                    if p < b.len() && b[p] == b'=' {
                        p += 1;
                        while p < b.len() && b[p].is_ascii_whitespace() {
                            p += 1;
                        }
                        if p < b.len() && (b[p] == b'"' || b[p] == b'\'') {
                            let q = b[p];
                            if let Some(end) = b[p + 1..].iter().position(|&c| c == q) {
                                let end = p + 1 + end;
                                b[s..=end].fill(b' ');
                                count += 1;
                                i = end + 1;
                                continue;
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }
    count
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The value of a property by local name (for example `creator`), from an
/// attribute or from element text, including the first `rdf:li` of a list.
pub(crate) fn prop(b: &[u8], local: &str) -> Option<String> {
    let l = local.as_bytes();
    let mut i = 0;
    while i + 1 + l.len() < b.len() {
        if b[i] == b':' && b[i + 1..i + 1 + l.len()].eq_ignore_ascii_case(l) {
            let e = i + 1 + l.len();
            let boundary = e >= b.len() || !is_name(b[e]);
            let prefixed = i > 0 && is_name(b[i - 1]);
            if boundary && prefixed {
                let mut p = e;
                while p < b.len() && b[p].is_ascii_whitespace() {
                    p += 1;
                }
                if p < b.len() && b[p] == b'=' {
                    p += 1;
                    while p < b.len() && b[p].is_ascii_whitespace() {
                        p += 1;
                    }
                    if p < b.len() && (b[p] == b'"' || b[p] == b'\'') {
                        let q = b[p];
                        if let Some(end) = b[p + 1..].iter().position(|&c| c == q) {
                            let v = String::from_utf8_lossy(&b[p + 1..p + 1 + end]);
                            let v = unescape(v.trim());
                            if !v.is_empty() {
                                return Some(v);
                            }
                        }
                    }
                } else if p < b.len() && b[p] == b'>' {
                    let rest = &b[p + 1..];
                    let lt = rest.iter().position(|&c| c == b'<').unwrap_or(rest.len());
                    let text = String::from_utf8_lossy(&rest[..lt]);
                    let text = text.trim();
                    if !text.is_empty() {
                        return Some(unescape(text));
                    }
                    let window = &rest[..rest.len().min(600)];
                    if let Some(li) = super::find(window, b"<rdf:li")
                        && let Some(gt) = window[li..].iter().position(|&c| c == b'>')
                    {
                        let start = li + gt + 1;
                        if let Some(end) = window[start..].iter().position(|&c| c == b'<') {
                            let t = String::from_utf8_lossy(&window[start..start + end]);
                            let t = t.trim();
                            if !t.is_empty() {
                                return Some(unescape(t));
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// Parse "51,30.5N", "51,30,20N" or plain signed degrees.
fn coord(s: &str) -> Option<f64> {
    let s = s.trim();
    let last = s.chars().last()?;
    if last.is_ascii_alphabetic() {
        let body = &s[..s.len() - 1];
        let mut parts = body.split(',');
        let d: f64 = parts.next()?.trim().parse().ok()?;
        let m: f64 = parts
            .next()
            .map(|x| x.trim().parse().ok())
            .unwrap_or(Some(0.0))?;
        let sec: f64 = parts
            .next()
            .map(|x| x.trim().parse().ok())
            .unwrap_or(Some(0.0))?;
        let v = d + m / 60.0 + sec / 3600.0;
        let neg = matches!(last.to_ascii_uppercase(), 'S' | 'W');
        Some(if neg { -v } else { v })
    } else {
        s.parse().ok()
    }
}

fn ratio(s: &str) -> Option<f64> {
    match s.split_once('/') {
        Some((n, d)) => {
            let d: f64 = d.trim().parse().ok()?;
            if d == 0.0 {
                None
            } else {
                Some(n.trim().parse::<f64>().ok()? / d)
            }
        }
        None => s.trim().parse().ok(),
    }
}

fn push_unique(v: &mut Vec<(String, String)>, name: &str, value: String) {
    if !v.iter().any(|(n, x)| n == name && *x == value) {
        v.push((name.to_string(), value));
    }
}

/// Add what an XMP packet says to a report.
pub(crate) fn inspect(b: &[u8], r: &mut Report) {
    r.xmp = true;
    if let Some(c) = prop(b, "creator") {
        push_unique(&mut r.owner, "Creator (XMP)", c);
    }
    if r.software.is_none() {
        r.software = prop(b, "CreatorTool");
    }
    for (local, name) in [
        ("SerialNumber", "SerialNumber (XMP)"),
        ("LensSerialNumber", "LensSerialNumber (XMP)"),
        ("BodySerialNumber", "BodySerialNumber (XMP)"),
        ("ImageUniqueID", "ImageUniqueID (XMP)"),
    ] {
        if let Some(v) = prop(b, local) {
            push_unique(&mut r.serials, name, v);
        }
    }
    let mut gps_props = 0;
    // Names match without regard to case, so `GpsLatitude` is covered too.
    for local in ["GPSLatitude", "GPSLongitude", "GPSAltitude"] {
        if prop(b, local).is_some() {
            gps_props += 1;
        }
    }
    if gps_props > 0 {
        r.gps_tags += gps_props;
        if r.gps.is_none() {
            let lat = prop(b, "GPSLatitude").and_then(|s| coord(&s));
            let lon = prop(b, "GPSLongitude").and_then(|s| coord(&s));
            if let (Some(lat), Some(lon)) = (lat, lon)
                && lat.abs() <= 90.0
                && lon.abs() <= 180.0
                && !(lat == 0.0 && lon == 0.0)
            {
                let mut alt = prop(b, "GPSAltitude").and_then(|s| ratio(&s));
                if prop(b, "GPSAltitudeRef").as_deref() == Some("1") {
                    alt = alt.map(|a| -a);
                }
                r.gps = Some(Gps { lat, lon, alt });
            }
        }
    }
    if r.live_photo_id.is_none() {
        r.live_photo_id = prop(b, "ContentIdentifier");
    }
    let motion = prop(b, "MotionPhoto").is_some_and(|v| v == "1")
        || prop(b, "MicroVideo").is_some_and(|v| v == "1");
    if motion && !r.secondary_images.iter().any(|s| s == "motion photo video") {
        r.secondary_images.push("motion photo video".to_string());
    }
}

/// True when a packet holds no properties, such as the one left by a strip.
pub(crate) fn is_empty_packet(b: &[u8]) -> bool {
    super::find(b, b"rdf:").is_none()
}

/// True when the packet looks like it belongs to a gain map image.
pub(crate) fn mentions_gain_map(b: &[u8]) -> bool {
    super::find(b, b"hdrgm:").is_some() || super::find(b, b"HDRGainMap").is_some()
}

/// An empty XMP packet of exactly `len` bytes, padded with spaces (never zeros).
pub(crate) fn empty_packet(len: usize) -> Vec<u8> {
    const HEAD: &[u8] = b"<?xpacket begin=\"\xEF\xBB\xBF\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>";
    const TAIL: &[u8] = b"<?xpacket end=\"w\"?>";
    const SHORT: &[u8] = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>";
    let mut v = Vec::with_capacity(len);
    if len >= HEAD.len() + TAIL.len() {
        v.extend_from_slice(HEAD);
        v.resize(len - TAIL.len(), b' ');
        v.extend_from_slice(TAIL);
    } else if len >= SHORT.len() {
        v.extend_from_slice(SHORT);
        v.resize(len, b' ');
    } else {
        v.resize(len, b' ');
    }
    v
}
