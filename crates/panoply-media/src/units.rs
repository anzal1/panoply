//! Sizes, times and the words used for them.

use crate::error::{MediaError, Result};

/// "12.3 MB", "880 KB", "412 bytes". Decimal units, as file managers show them.
pub fn size_text(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.2} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{bytes} bytes")
    }
}

/// "1:02:03.5", "2:03", "4.2 s".
pub fn duration_text(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "unknown".into();
    }
    if seconds < 60.0 {
        return format!("{seconds:.1} s");
    }
    let total = seconds.round() as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// "8.4 Mb/s", "128 kb/s".
pub fn bitrate_text(bps: f64) -> String {
    if bps >= 1e6 {
        format!("{:.1} Mb/s", bps / 1e6)
    } else {
        format!("{:.0} kb/s", bps / 1e3)
    }
}

/// Parse "25mb", "500 kb", "1.5gb", "25MiB" or a bare number (megabytes) into bytes.
pub fn parse_size(text: &str) -> Result<u64> {
    let t = text.trim().to_ascii_lowercase();
    let split = t.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(t.len());
    let (num, unit) = (t[..split].trim(), t[split..].trim());
    let n: f64 = num.parse().map_err(|_| {
        MediaError::BadOption(format!("\"{text}\" is not a size. Try 25mb or 500kb"))
    })?;
    let mult = match unit {
        "" | "mb" | "m" => 1e6,
        "kb" | "k" => 1e3,
        "gb" | "g" => 1e9,
        "b" | "bytes" => 1.0,
        "kib" => 1024.0,
        "mib" => 1024.0 * 1024.0,
        "gib" => 1024.0 * 1024.0 * 1024.0,
        _ => {
            return Err(MediaError::BadOption(format!(
                "\"{text}\" is not a size. Try 25mb or 500kb"
            )));
        }
    };
    if !(n.is_finite() && n > 0.0) {
        return Err(MediaError::BadOption(format!(
            "\"{text}\" is not a size. Try 25mb or 500kb"
        )));
    }
    Ok((n * mult).round() as u64)
}

/// Parse a time such as "90", "1:30", "01:02:03.5", "1m30s" or "12.5s" into seconds.
pub fn parse_time(text: &str) -> Result<f64> {
    let bad = || {
        MediaError::BadOption(format!(
            "\"{text}\" is not a time. Use seconds (90), minutes and seconds (1:30) or hours (1:02:03.5)"
        ))
    };
    let t = text.trim().to_ascii_lowercase();
    if t.is_empty() {
        return Err(bad());
    }
    if t.contains(':') {
        let parts: Vec<&str> = t.split(':').collect();
        if parts.len() > 3 {
            return Err(bad());
        }
        let mut total = 0.0;
        for p in &parts {
            let v: f64 = p.trim().parse().map_err(|_| bad())?;
            if v < 0.0 || !v.is_finite() {
                return Err(bad());
            }
            total = total * 60.0 + v;
        }
        return Ok(total);
    }
    // 1h2m3.5s, 1m30s, 12.5s, 90
    let mut total = 0.0;
    let mut num = String::new();
    let mut any = false;
    for c in t.chars() {
        match c {
            '0'..='9' | '.' => num.push(c),
            'h' | 'm' | 's' => {
                let v: f64 = num.parse().map_err(|_| bad())?;
                num.clear();
                total += v * match c {
                    'h' => 3600.0,
                    'm' => 60.0,
                    _ => 1.0,
                };
                any = true;
            }
            c if c.is_whitespace() => {}
            _ => return Err(bad()),
        }
    }
    if !num.is_empty() {
        total += num.parse::<f64>().map_err(|_| bad())?;
        any = true;
    }
    if any && total.is_finite() && total >= 0.0 {
        Ok(total)
    } else {
        Err(bad())
    }
}

/// A list of keep ranges such as "0:12-1:40, 2:00-2:30" or "5-" (to the end) or "-30" (from the
/// start). Returns `(start, end)` pairs in seconds, `end = None` meaning to the end.
pub fn parse_ranges(text: &str) -> Result<Vec<(f64, Option<f64>)>> {
    let bad = |m: String| MediaError::BadOption(m);
    let mut out = Vec::new();
    for part in text.split([',', ';']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (a, b) = part.split_once('-').ok_or_else(|| {
            bad(format!(
                "\"{part}\" is not a range. Write it as start-end, for example 0:12-1:40"
            ))
        })?;
        let start = if a.trim().is_empty() {
            0.0
        } else {
            parse_time(a)?
        };
        let end = if b.trim().is_empty() {
            None
        } else {
            Some(parse_time(b)?)
        };
        if let Some(e) = end
            && e <= start
        {
            return Err(bad(format!("The range \"{part}\" ends before it starts")));
        }
        out.push((start, end));
    }
    if out.is_empty() {
        return Err(bad("Give at least one range, for example 0:12-1:40".into()));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    for w in out.windows(2) {
        let end = w[0].1.unwrap_or(f64::INFINITY);
        if end > w[1].0 {
            return Err(bad(format!(
                "The ranges overlap near {}",
                duration_text(w[1].0)
            )));
        }
    }
    Ok(out)
}

/// Turn "keep" ranges into the complement ("remove" mode), within `0..duration`.
pub fn invert_ranges(remove: &[(f64, Option<f64>)], duration: f64) -> Vec<(f64, Option<f64>)> {
    let mut keep = Vec::new();
    let mut cursor = 0.0;
    for &(s, e) in remove {
        if s > cursor + 1e-6 {
            keep.push((cursor, Some(s)));
        }
        match e {
            Some(e) => cursor = cursor.max(e),
            None => return keep,
        }
    }
    if cursor < duration - 1e-6 {
        keep.push((cursor, None));
    }
    keep
}

/// The lower-case extension of a path, without the dot.
pub fn ext_of(path: &std::path::Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// The file name for messages.
pub fn name_of(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The file stem for naming outputs.
pub fn stem_of(path: &std::path::Path) -> String {
    path.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse_and_print() {
        assert_eq!(parse_size("25mb").unwrap(), 25_000_000);
        assert_eq!(parse_size("500 KB").unwrap(), 500_000);
        assert_eq!(parse_size("1.5gb").unwrap(), 1_500_000_000);
        assert_eq!(parse_size("8").unwrap(), 8_000_000);
        assert_eq!(parse_size("25MiB").unwrap(), 26_214_400);
        assert!(parse_size("lots").is_err());
        assert!(parse_size("0mb").is_err());
        assert_eq!(size_text(880_000), "880 KB");
        assert_eq!(size_text(12_300_000), "12.3 MB");
    }

    #[test]
    fn times_parse() {
        assert_eq!(parse_time("90").unwrap(), 90.0);
        assert_eq!(parse_time("1:30").unwrap(), 90.0);
        assert_eq!(parse_time("01:02:03.5").unwrap(), 3723.5);
        assert_eq!(parse_time("1m30s").unwrap(), 90.0);
        assert_eq!(parse_time("12.5s").unwrap(), 12.5);
        assert!(parse_time("soon").is_err());
        assert!(parse_time("1:2:3:4").is_err());
        assert!(parse_time("").is_err());
    }

    #[test]
    fn ranges_parse_and_check() {
        assert_eq!(
            parse_ranges("0:12-1:40, 2:00-2:30").unwrap(),
            vec![(12.0, Some(100.0)), (120.0, Some(150.0))]
        );
        assert_eq!(parse_ranges("5-").unwrap(), vec![(5.0, None)]);
        assert_eq!(parse_ranges("-30").unwrap(), vec![(0.0, Some(30.0))]);
        assert!(parse_ranges("10-5").is_err());
        assert!(parse_ranges("0-10, 5-20").is_err());
        assert!(parse_ranges("nonsense").is_err());
    }

    #[test]
    fn removing_a_range_keeps_the_rest() {
        let keep = invert_ranges(&[(10.0, Some(20.0))], 60.0);
        assert_eq!(keep, vec![(0.0, Some(10.0)), (20.0, None)]);
        let keep = invert_ranges(&[(0.0, Some(5.0)), (50.0, None)], 60.0);
        assert_eq!(keep, vec![(5.0, Some(50.0))]);
    }
}
