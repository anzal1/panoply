//! Small statistics helpers and number formatting shared by the report code.

/// Median of the values, `0.0` for an empty slice.
pub fn median(values: &[f64]) -> f64 {
    percentile(values, 50.0)
}

/// Nearest-rank percentile, `0.0` for an empty slice. `p` is 0 to 100.
pub fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut v: Vec<f64> = values.iter().copied().filter(|x| !x.is_nan()).collect();
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let rank = ((p / 100.0) * v.len() as f64).ceil() as usize;
    v[rank.clamp(1, v.len()) - 1]
}

/// Largest value, `0.0` for an empty slice.
pub fn max(values: &[f64]) -> f64 {
    values.iter().copied().fold(0.0, f64::max)
}

/// Bytes to decimal megabytes. The budgets in FLOWS.md are decimal "MB", so a pass here is also
/// a pass under the stricter reading.
pub fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1_000_000.0
}

/// Human size with decimal units: `640 B`, `12.3 KB`, `4.1 MB`, `2.15 GB`.
pub fn human_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes < 1_000 {
        format!("{bytes} B")
    } else if bytes < 1_000_000 {
        format!("{:.1} KB", b / 1_000.0)
    } else if bytes < 1_000_000_000 {
        format!("{:.1} MB", b / 1_000_000.0)
    } else {
        format!("{:.2} GB", b / 1_000_000_000.0)
    }
}

/// Parse a size such as `64MiB`, `2GiB`, `500MB`, `1048576`. `KiB`, `MiB`, `GiB`, `TiB` are
/// powers of 1024, `KB`, `MB`, `GB`, `TB` are powers of 1000, a bare number is bytes.
pub fn parse_size(text: &str) -> Option<u64> {
    let t = text.trim();
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let value: f64 = num.parse().ok()?;
    let mult: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        "kib" | "k" => 1024.0,
        "mib" | "m" => 1024.0 * 1024.0,
        "gib" | "g" => 1024.0 * 1024.0 * 1024.0,
        "tib" | "t" => 1024.0_f64.powi(4),
        _ => return None,
    };
    let bytes = value * mult;
    (bytes.is_finite() && bytes >= 0.0).then_some(bytes as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_and_percentile() {
        assert_eq!(median(&[]), 0.0);
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), 2.0);
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0, 5.0], 95.0), 5.0);
        assert_eq!(max(&[1.0, 9.0, 3.0]), 9.0);
    }

    #[test]
    fn sizes_parse_both_ways() {
        assert_eq!(parse_size("1MiB"), Some(1_048_576));
        assert_eq!(parse_size("2GiB"), Some(2_147_483_648));
        assert_eq!(parse_size("500MB"), Some(500_000_000));
        assert_eq!(parse_size("4096"), Some(4096));
        assert_eq!(parse_size("1.5GiB"), Some(1_610_612_736));
        assert_eq!(parse_size("12 parsecs"), None);
        assert_eq!(parse_size(""), None);
    }

    #[test]
    fn bytes_read_well() {
        assert_eq!(human_bytes(640), "640 B");
        assert_eq!(human_bytes(12_300), "12.3 KB");
        assert_eq!(human_bytes(4_100_000), "4.1 MB");
        assert_eq!(human_bytes(2_147_483_648), "2.15 GB");
        assert_eq!(mb(300_000_000), 300.0);
    }
}
