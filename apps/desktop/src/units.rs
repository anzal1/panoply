//! Sizes and times in plain words. Decimal units, as file managers show them.

/// "12.3 MB", "880 KB", "412 bytes".
pub fn format_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", b / 1e9)
    } else if bytes >= 10_000_000 {
        format!("{:.0} MB", b / 1e6)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", b / 1e6)
    } else if bytes >= 1000 {
        format!("{:.0} KB", b / 1e3)
    } else if bytes == 1 {
        "1 byte".to_string()
    } else {
        format!("{bytes} bytes")
    }
}

/// Parse "50kb", "1.5 MB", "2mb", "200000" into bytes. Decimal units, case blind.
pub fn parse_size(text: &str) -> Option<u64> {
    let t: String = text
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    if t.is_empty() {
        return None;
    }
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let n: f64 = num.parse().ok()?;
    if !n.is_finite() || n < 0.0 {
        return None;
    }
    let mult = match unit {
        "" | "b" | "bytes" => 1.0,
        "k" | "kb" => 1e3,
        "m" | "mb" => 1e6,
        "g" | "gb" => 1e9,
        _ => return None,
    };
    Some((n * mult).round() as u64)
}

/// Bytes as typed text for a size field: "50kb", "2mb", "1234".
pub fn size_input_text(bytes: u64) -> String {
    if bytes >= 1_000_000 && bytes.is_multiple_of(1_000_000) {
        format!("{}mb", bytes / 1_000_000)
    } else if bytes >= 1000 && bytes.is_multiple_of(1000) {
        format!("{}kb", bytes / 1000)
    } else {
        bytes.to_string()
    }
}

/// The size unit a name or label speaks of: `under_mb`, `max_kb`, "Under (MB)".
pub fn unit_in(text: &str) -> Option<f64> {
    let t = text.to_ascii_lowercase();
    let words: Vec<&str> = t
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    words.iter().rev().find_map(|w| match *w {
        "kb" => Some(1e3),
        "mb" => Some(1e6),
        "gb" => Some(1e9),
        _ => None,
    })
}

/// "2 min ago", "3 h ago", "Yesterday", "5 d ago", from seconds since the epoch.
pub fn relative_time(now: u64, then: u64) -> String {
    let d = now.saturating_sub(then);
    match d {
        0..=44 => "Just now".to_string(),
        45..=3599 => format!("{} min ago", (d + 30) / 60),
        3600..=86_399 => format!("{} h ago", d / 3600),
        86_400..=172_799 => "Yesterday".to_string(),
        _ => format!("{} d ago", d / 86_400),
    }
}

/// Seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_read_like_a_file_manager() {
        assert_eq!(format_bytes(412), "412 bytes");
        assert_eq!(format_bytes(880_000), "880 KB");
        assert_eq!(format_bytes(4_200_000), "4.2 MB");
        assert_eq!(format_bytes(312_000), "312 KB");
    }

    #[test]
    fn sizes_parse_with_units() {
        assert_eq!(parse_size("50kb"), Some(50_000));
        assert_eq!(parse_size(" 1.5 MB "), Some(1_500_000));
        assert_eq!(parse_size("200000"), Some(200_000));
        assert_eq!(parse_size("2 gb"), Some(2_000_000_000));
        assert_eq!(parse_size("fifty"), None);
        assert_eq!(parse_size("12 parsecs"), None);
        assert_eq!(parse_size(""), None);
    }

    #[test]
    fn size_text_round_trips() {
        for b in [50_000u64, 2_000_000, 1234, 1_500_000] {
            assert_eq!(parse_size(&size_input_text(b)), Some(b));
        }
        assert_eq!(size_input_text(50_000), "50kb");
    }

    #[test]
    fn units_are_found_in_option_names() {
        assert_eq!(unit_in("under_mb"), Some(1e6));
        assert_eq!(unit_in("Largest size (KB)"), Some(1e3));
        assert_eq!(unit_in("max_size"), None);
    }

    #[test]
    fn relative_times() {
        assert_eq!(relative_time(1000, 990), "Just now");
        assert_eq!(relative_time(1000, 1000 - 120), "2 min ago");
        assert_eq!(relative_time(100_000, 100_000 - 7200), "2 h ago");
        assert_eq!(relative_time(300_000, 300_000 - 100_000), "Yesterday");
        assert_eq!(relative_time(1_000_000, 1_000_000 - 400_000), "4 d ago");
    }
}
