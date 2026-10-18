//! The LocalSend v2 wire types, written from the public protocol specification
//! (github.com/localsend/protocol). Field names and casing are the spec's.

use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Default port for both the multicast group and the HTTP(S) server.
pub const DEFAULT_PORT: u16 = 53317;
/// The multicast group devices announce on. It sits in 224.0.0.0/24 because some Android
/// devices refuse other groups.
pub const MULTICAST_GROUP: std::net::Ipv4Addr = std::net::Ipv4Addr::new(224, 0, 0, 167);
/// The protocol version we announce. Any `2.x` peer is accepted.
pub const PROTOCOL_VERSION: &str = "2.1";
/// URL prefix of every API endpoint.
pub const API_PREFIX: &str = "/api/localsend/v2";

/// What kind of device this is. Only affects icons; unknown values read as `Desktop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DeviceType {
    /// A phone or tablet.
    Mobile,
    /// A laptop or desktop computer.
    #[default]
    Desktop,
    /// A browser.
    Web,
    /// A device with no screen.
    Headless,
    /// A server.
    Server,
}

impl DeviceType {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceType::Mobile => "mobile",
            DeviceType::Desktop => "desktop",
            DeviceType::Web => "web",
            DeviceType::Headless => "headless",
            DeviceType::Server => "server",
        }
    }
}

impl Serialize for DeviceType {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DeviceType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "mobile" => DeviceType::Mobile,
            "web" => DeviceType::Web,
            "headless" => DeviceType::Headless,
            "server" => DeviceType::Server,
            _ => DeviceType::Desktop,
        })
    }
}

/// `http` or `https`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    /// Plain HTTP (the fingerprint is then a random string).
    Http,
    /// TLS with a self-signed certificate (the fingerprint is its SHA-256).
    #[default]
    Https,
}

impl Scheme {
    /// `"http"` or `"https"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
        }
    }
}

/// The device description used by announcements, `register`, `prepare-upload` and `info`.
///
/// `port` and `protocol` are optional because the spec's `register` response example omits
/// them; read them with [`DeviceInfo::port_or_default`] and [`DeviceInfo::scheme_or_default`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// Display name.
    pub alias: String,
    /// Protocol version as `major.minor`.
    pub version: String,
    /// Hardware or OS label, for example `macOS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_model: Option<String>,
    /// Device kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_type: Option<DeviceType>,
    /// SHA-256 of the certificate in HTTPS mode, a random string in HTTP mode.
    pub fingerprint: String,
    /// Server port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// `http` or `https`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<Scheme>,
    /// Whether the device serves the download (reverse transfer) API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download: Option<bool>,
    /// Multicast only: a reply is expected when true. Older apps spell it `announcement`.
    #[serde(
        default,
        alias = "announcement",
        skip_serializing_if = "Option::is_none"
    )]
    pub announce: Option<bool>,
}

impl DeviceInfo {
    /// The port, or the protocol default when the message left it out.
    pub fn port_or_default(&self) -> u16 {
        self.port.unwrap_or(DEFAULT_PORT)
    }

    /// The scheme, or HTTPS when the message left it out.
    pub fn scheme_or_default(&self) -> Scheme {
        self.protocol.unwrap_or_default()
    }

    /// The device kind, `Desktop` when absent.
    pub fn kind(&self) -> DeviceType {
        self.device_type.unwrap_or_default()
    }

    /// True when the peer speaks protocol major version 2.
    pub fn version_compatible(&self) -> bool {
        self.version.split('.').next() == Some("2")
    }

    /// Clamp and clean the fields a peer controls before they reach a UI or a log.
    pub fn sanitized(mut self) -> Self {
        self.alias = clean_text(&self.alias, 64);
        if self.alias.is_empty() {
            self.alias = "Unknown device".into();
        }
        self.device_model = self.device_model.map(|m| clean_text(&m, 64));
        self.fingerprint = clean_text(&self.fingerprint, 128);
        self
    }
}

/// Remove control characters and cut to `max` characters.
pub fn clean_text(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Timestamps of a file, ISO 8601 in UTC.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FileTimes {
    /// Last modification time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    /// Last access time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accessed: Option<String>,
}

/// One file in a `prepare-upload` request (and in `prepare-download` responses).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMeta {
    /// Key of the file in the request map.
    pub id: String,
    /// File name. A sender may use `/` separated segments for a folder tree.
    pub file_name: String,
    /// Size in bytes.
    pub size: u64,
    /// MIME type, for example `image/jpeg`.
    pub file_type: String,
    /// Hex SHA-256 of the content, when the sender computed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Base64 preview data, or the text itself for a `text/plain` message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// Timestamps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<FileTimes>,
}

/// Body of `POST /prepare-upload`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrepareUploadRequest {
    /// The sender.
    pub info: DeviceInfo,
    /// Files keyed by file id.
    pub files: HashMap<String, FileMeta>,
}

/// Answer to `POST /prepare-upload`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareUploadResponse {
    /// Identifies the session in later calls.
    pub session_id: String,
    /// Upload token per accepted file id.
    pub files: HashMap<String, String>,
}

/// Answer to `POST /prepare-download`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareDownloadResponse {
    /// The serving device.
    pub info: DeviceInfo,
    /// Identifies the session in later calls.
    pub session_id: String,
    /// Files keyed by file id.
    pub files: HashMap<String, FileMeta>,
}

/// Lowercase hex of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 15) as usize] as char);
    }
    s
}

/// Panoply announces itself as "Panoply macOS", "Panoply Windows" and so on in `deviceModel`. Other
/// LocalSend apps never do, and only Panoply shows the four word check code, so a screen that tells
/// someone to compare it asks first whether the other device is Panoply. A device can claim the name
/// without being one; then the two codes simply do not match, which is the safe outcome.
pub const PANOPLY_MODEL_PREFIX: &str = "Panoply ";

/// The label of this operating system as people spell it.
pub fn os_label() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    }
}

/// True when the device says it is a Panoply app.
pub fn is_panoply(info: &DeviceInfo) -> bool {
    info.device_model
        .as_deref()
        .is_some_and(|m| m.starts_with(PANOPLY_MODEL_PREFIX))
}

/// True for a 64 character hex string, the shape of a certificate fingerprint.
pub fn is_fingerprint(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `n` random bytes as lowercase hex (ids, tokens, HTTP-mode fingerprints).
pub fn random_hex(n: usize) -> String {
    let mut buf = vec![0u8; n];
    getrandom::fill(&mut buf).expect("the OS random source is available");
    hex(&buf)
}

/// Split seconds since the Unix epoch into (year, month, day, hour, minute, second), UTC.
pub fn civil(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        (rem % 3600 / 60) as u32,
        (rem % 60) as u32,
    )
}

/// Format seconds since the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn iso8601(secs: i64) -> String {
    let (y, m, d, hh, mm, ss) = civil(secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Parse `YYYY-MM-DDTHH:MM:SS[.fff]Z` (or a `+00:00` offset) to Unix seconds.
pub fn parse_iso8601(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !(b[10] == b'T' || b[10] == b' ') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_round_trip() {
        for secs in [
            0i64,
            1_609_504_496,
            951_782_400,
            4_102_444_800,
            1_800_000_000,
        ] {
            assert_eq!(parse_iso8601(&iso8601(secs)), Some(secs));
        }
        assert_eq!(iso8601(1_609_504_496), "2021-01-01T12:34:56Z");
    }

    #[test]
    fn unknown_device_type_is_desktop() {
        let t: DeviceType = serde_json::from_str("\"toaster\"").unwrap();
        assert_eq!(t, DeviceType::Desktop);
    }
}
