//! Places Panoply never moves to the Trash: the folders an operating system or an app needs to run.
//!
//! The disk map lets a person pick any folder, and a folder like `~/Library` or `C:\Windows` looks like
//! just another big one. Moving it would break the system or every app. So the list lives here, below
//! the disk map, and [`crate::trash_item`] checks it too: a picker that forgot to ask still cannot
//! move one of these.
//!
//! The rules are pure functions of an [`Os`], a path written as a string and the home folder, so the
//! list for every system is tested on every system. [`refusal_for_home`] adds what only the host can
//! know: symbolic links in the parent folders and mount points.
//!
//! Inside a protected folder, a short list of cache folders is allowed (`~/Library/Caches/<app>`, a
//! Windows `Temp` folder), because that is where the space really is and the apps rebuild them.
//! [`cache_note`] returns the note to show next to those.

use std::path::{Path, PathBuf};

/// An operating system's list of protected places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    /// macOS.
    MacOs,
    /// Linux and other Unix systems with the freedesktop layout.
    Linux,
    /// Windows.
    Windows,
}

impl Os {
    /// The system this build runs on.
    pub fn host() -> Os {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }

    fn system_reason(self) -> &'static str {
        match self {
            Os::MacOs => "macOS needs this folder",
            Os::Linux => "Linux needs this folder",
            Os::Windows => "Windows needs this folder",
        }
    }

    fn case_blind(self) -> bool {
        self != Os::Linux
    }
}

/// The reason for a whole drive or volume.
pub const VOLUME_REASON: &str = "this is a whole drive, which Panoply never moves";
/// The reason for the home folder, or a folder that holds it.
pub const HOME_REASON: &str = "this is your home folder, which Panoply never moves";
/// The reason for a folder that holds the home folder (`/Users`, `/home`).
pub const HOLDS_HOME_REASON: &str = "it holds your home folder, which Panoply never moves";
/// The reason for sign-in keys.
pub const KEYS_REASON: &str = "it holds your sign-in keys, which Panoply never moves";
/// The note shown beside a cache folder that is allowed inside a protected one.
pub const CACHE_NOTE: &str = "Apps rebuild their caches, so this is safe to move";

/// A path as lowercase-if-needed components, `.` dropped and `..` applied. Backslashes count as
/// separators on Windows only (on Unix a backslash is a legal file name character).
fn comps(os: Os, path: &str) -> Vec<String> {
    let text = if os == Os::Windows {
        path.replace('\\', "/")
    } else {
        path.to_string()
    };
    let mut out: Vec<String> = Vec::new();
    for part in text.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            p => out.push(if os.case_blind() {
                p.to_lowercase()
            } else {
                p.to_string()
            }),
        }
    }
    out
}

fn is(c: &[String], want: &[&str]) -> bool {
    c.len() == want.len() && c.iter().zip(want).all(|(a, b)| a == b)
}

fn under(c: &[String], want: &[&str]) -> bool {
    c.len() >= want.len() && c.iter().zip(want).all(|(a, b)| a == b)
}

/// `c` is `base` followed by `rest` (so `home/library` is `is_at(c, &home, &["library"])`).
fn is_at(c: &[String], base: &[String], rest: &[&str]) -> bool {
    c.len() == base.len() + rest.len() && c.starts_with(base) && is(&c[base.len()..], rest)
}

fn under_at(c: &[String], base: &[String], rest: &[&str]) -> bool {
    c.len() >= base.len() + rest.len() && c.starts_with(base) && under(&c[base.len()..], rest)
}

/// Why `path` may not be moved on `os`, or `None` when it may. `home` is the home folder.
pub fn refusal_on(os: Os, path: &str, home: Option<&str>) -> Option<&'static str> {
    let c = comps(os, path);
    let home_c = home.map(|h| comps(os, h)).filter(|h| !h.is_empty());
    if let Some(r) = common_refusal(os, &c, home_c.as_deref()) {
        return Some(r);
    }
    match os {
        Os::MacOs => mac(&c, home_c.as_deref()),
        Os::Linux => linux(&c, home_c.as_deref()),
        Os::Windows => windows(&c, home_c.as_deref()),
    }
}

/// The checks that are the same everywhere: the top of a drive, the home folder and what holds it, keys.
fn common_refusal(os: Os, c: &[String], home: Option<&[String]>) -> Option<&'static str> {
    if c.is_empty() || (os == Os::Windows && c.len() == 1) {
        return Some(VOLUME_REASON);
    }
    if let Some(h) = home {
        // The home folder itself, or any folder above it.
        if h == c {
            return Some(HOME_REASON);
        }
        if h.starts_with(c) {
            return Some(HOLDS_HOME_REASON);
        }
        if under_at(c, h, &[".ssh"]) || under_at(c, h, &[".gnupg"]) {
            return Some(KEYS_REASON);
        }
    }
    None
}

fn mac(c: &[String], home: Option<&[String]>) -> Option<&'static str> {
    let sys = Os::MacOs.system_reason();
    if under(c, &["system"])
        || under(c, &["library"])
        || under(c, &["usr"])
        || under(c, &["bin"])
        || under(c, &["sbin"])
    {
        return Some(sys);
    }
    for top in [
        &["applications"][..],
        &["users"],
        &["etc"],
        &["var"],
        &["tmp"],
        &["opt"],
        &["cores"],
        &["dev"],
        &["network"],
        &["private"],
        &["private", "etc"],
        &["private", "var"],
        &["private", "tmp"],
        &["volumes"],
    ] {
        if is(c, top) {
            return Some(sys);
        }
    }
    // /Volumes/<name> is a mounted volume.
    if under(c, &["volumes"]) && c.len() == 2 {
        return Some(VOLUME_REASON);
    }
    // /Users/<name> is somebody's home folder, yours or not.
    if under(c, &["users"]) && c.len() == 2 {
        return Some(HOME_REASON);
    }
    if let Some(h) = home {
        if is_at(c, h, &["applications"]) {
            return Some(sys);
        }
        if under_at(c, h, &["library"]) && cache_note_on(Os::MacOs, c, Some(h)).is_none() {
            return Some(sys);
        }
    }
    None
}

fn linux(c: &[String], home: Option<&[String]>) -> Option<&'static str> {
    let sys = Os::Linux.system_reason();
    // Removable media mount under /run/media/<user>/<volume>: the volume is protected, its files are not.
    if under(c, &["run", "media"]) {
        return match c.len() {
            2 | 3 => Some(sys),
            4 => Some(VOLUME_REASON),
            _ => None,
        };
    }
    for tree in [
        "usr", "bin", "sbin", "lib", "lib32", "lib64", "libx32", "boot", "etc", "dev", "proc",
        "sys", "run", "root", "snap",
    ] {
        if under(c, &[tree]) {
            return Some(sys);
        }
    }
    if under(c, &["var", "lib"]) {
        return Some(sys);
    }
    for top in [
        &["home"][..],
        &["media"],
        &["mnt"],
        &["opt"],
        &["srv"],
        &["tmp"],
        &["var"],
    ] {
        if is(c, top) {
            return Some(sys);
        }
    }
    if (under(c, &["mnt"]) || under(c, &["media"])) && c.len() == 2 {
        return Some(VOLUME_REASON);
    }
    if under(c, &["home"]) && c.len() == 2 {
        return Some(HOME_REASON);
    }
    if let Some(h) = home {
        for dir in [
            &[".config"][..],
            &[".local"],
            &[".local", "share"],
            &[".cache"],
        ] {
            if is_at(c, h, dir) {
                return Some(sys);
            }
        }
    }
    None
}

fn windows(c: &[String], home: Option<&[String]>) -> Option<&'static str> {
    let sys = Os::Windows.system_reason();
    // c[0] is the drive ("c:"), c[1] the first folder on it.
    if c.len() >= 2 {
        let top = &c[1..];
        for tree in [
            "windows",
            "program files",
            "program files (x86)",
            "system volume information",
            "$recycle.bin",
            "recovery",
            "boot",
        ] {
            if under(top, &[tree]) {
                return Some(sys);
            }
        }
        for dir in [
            "programdata",
            "users",
            "perflogs",
            "documents and settings",
            "msocache",
        ] {
            if is(top, &[dir]) {
                return Some(sys);
            }
        }
        if under(top, &["users"]) && top.len() == 2 {
            return Some(HOME_REASON);
        }
    }
    if let Some(h) = home {
        for dir in [
            &["appdata"][..],
            &["appdata", "local"],
            &["appdata", "roaming"],
            &["appdata", "locallow"],
            &["appdata", "local", "temp"],
        ] {
            if is_at(c, h, dir) {
                return Some(sys);
            }
        }
    }
    None
}

/// The note to show beside an entry that is allowed only because it is a known cache inside a protected
/// folder, or `None` for an ordinary entry. A path that is refused never has a note.
pub fn cache_note(os: Os, path: &str, home: Option<&str>) -> Option<&'static str> {
    let c = comps(os, path);
    let home_c = home.map(|h| comps(os, h)).filter(|h| !h.is_empty());
    if refusal_on(os, path, home).is_some() {
        return None;
    }
    cache_note_on(os, &c, home_c.as_deref())
}

fn cache_note_on(os: Os, c: &[String], home: Option<&[String]>) -> Option<&'static str> {
    let h = home?;
    let yes = match os {
        Os::MacOs => {
            // Children of Caches and Logs, and Xcode's build folders. The folders themselves stay.
            (under_at(c, h, &["library", "caches"]) && c.len() > h.len() + 2)
                || (under_at(c, h, &["library", "logs"]) && c.len() > h.len() + 2)
                || under_at(c, h, &["library", "developer", "xcode", "deriveddata"])
                || (under_at(
                    c,
                    h,
                    &["library", "developer", "xcode", "ios devicesupport"],
                ) && c.len() > h.len() + 4)
                || (under_at(c, h, &["library", "developer", "coresimulator", "caches"])
                    && c.len() > h.len() + 4)
        }
        Os::Linux => under_at(c, h, &[".cache"]) && c.len() > h.len() + 1,
        Os::Windows => {
            (under_at(c, h, &["appdata", "local", "temp"]) && c.len() > h.len() + 3)
                || (under_at(c, h, &["appdata", "local"])
                    && c.len() > h.len() + 3
                    && c[h.len() + 2..].iter().any(|p| {
                        matches!(p.as_str(), "cache" | "caches" | "code cache" | "gpucache")
                    }))
        }
    };
    yes.then_some(CACHE_NOTE)
}

/// The current user's home folder, from the environment.
pub fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Why this machine may not move `path`, with `home` as the home folder (a test or a render can name
/// a stand-in). Checks the path as written, then with the parent folders resolved (so a link into
/// `~/Library` is caught), then whether it is the root of a mounted volume. The item itself is not
/// followed: moving a link moves only the link.
pub fn refusal_for_home(path: &Path, home: Option<&Path>) -> Option<&'static str> {
    let os = Os::host();
    let home_s = home.map(|h| h.to_string_lossy().into_owned());
    let check = |p: &Path| refusal_on(os, &p.to_string_lossy(), home_s.as_deref());
    if let Some(r) = check(path) {
        return Some(r);
    }
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name())
        && !parent.as_os_str().is_empty()
        && let Ok(real_parent) = std::fs::canonicalize(parent)
    {
        let real = real_parent.join(name);
        // The home folder may itself sit behind a link (/Users on a mounted disk, a synced profile).
        if let Some(r) = check(&real) {
            return Some(r);
        }
        if let Some(h) = home
            && let Ok(real_home) = std::fs::canonicalize(h)
            && let Some(r) = refusal_on(
                os,
                &real.to_string_lossy(),
                Some(&real_home.to_string_lossy()),
            )
        {
            return Some(r);
        }
    }
    if is_mount_point(path) {
        return Some(VOLUME_REASON);
    }
    None
}

/// [`refusal_for_home`] for the current user's home folder.
pub fn refusal(path: &Path) -> Option<&'static str> {
    refusal_for_home(path, home_dir().as_deref())
}

/// The cache note for `path` on this machine, or `None`. Asked the same ways as [`refusal_for_home`]: the
/// path as written, then with its parent folders (and the home folder) resolved, so a scan that reports
/// real paths still matches a home folder named through a link.
pub fn cache_note_for_home(path: &Path, home: Option<&Path>) -> Option<&'static str> {
    let os = Os::host();
    let home_s = home.map(|h| h.to_string_lossy().into_owned());
    if let Some(n) = cache_note(os, &path.to_string_lossy(), home_s.as_deref()) {
        return Some(n);
    }
    let (parent, name) = (path.parent()?, path.file_name()?);
    let real = std::fs::canonicalize(parent).ok()?.join(name);
    let real_home = home.and_then(|h| std::fs::canonicalize(h).ok());
    let real_home_s = real_home
        .as_deref()
        .map(|h| h.to_string_lossy().into_owned());
    cache_note(
        os,
        &real.to_string_lossy(),
        real_home_s.as_deref().or(home_s.as_deref()),
    )
}

#[cfg(unix)]
fn is_mount_point(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let (Ok(me), Some(parent)) = (std::fs::symlink_metadata(path), path.parent()) else {
        return false;
    };
    if !me.is_dir() || parent.as_os_str().is_empty() {
        return false;
    }
    std::fs::metadata(parent).is_ok_and(|p| p.dev() != me.dev())
}

#[cfg(not(unix))]
fn is_mount_point(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC_HOME: &str = "/Users/me";
    const LIN_HOME: &str = "/home/me";
    const WIN_HOME: &str = r"C:\Users\Me";

    fn mac(p: &str) -> Option<&'static str> {
        refusal_on(Os::MacOs, p, Some(MAC_HOME))
    }
    fn lin(p: &str) -> Option<&'static str> {
        refusal_on(Os::Linux, p, Some(LIN_HOME))
    }
    fn win(p: &str) -> Option<&'static str> {
        refusal_on(Os::Windows, p, Some(WIN_HOME))
    }

    #[test]
    fn macos_refuses_what_the_system_and_apps_need() {
        for p in [
            "/",
            "/System",
            "/System/Library/CoreServices",
            "/Library",
            "/Library/Application Support",
            "/Applications",
            "/Users",
            "/usr/bin/ssh",
            "/bin",
            "/private",
            "/private/var",
            "/Volumes",
            "/Users/me",
            "/Users/other",
            "/Users/me/Library",
            "/Users/me/Library/Application Support",
            "/Users/me/Library/Application Support/Code",
            "/Users/me/Library/Caches",
            "/Users/me/Library/Mobile Documents",
            "/Users/me/Applications",
            "/Users/me/.ssh",
            "/Users/me/.ssh/id_ed25519",
        ] {
            assert!(mac(p).is_some(), "{p} must be refused");
        }
        assert_eq!(mac("/Users/me/Library"), Some("macOS needs this folder"));
        assert_eq!(mac("/Applications"), Some("macOS needs this folder"));
    }

    #[test]
    fn macos_names_volumes_and_homes_plainly() {
        assert_eq!(mac("/Volumes/Backup"), Some(VOLUME_REASON));
        assert_eq!(mac("/"), Some(VOLUME_REASON));
        assert_eq!(mac("/Users/me"), Some(HOME_REASON));
        assert_eq!(mac("/Users"), Some(HOLDS_HOME_REASON));
    }

    #[test]
    fn macos_is_case_blind_like_apfs() {
        assert!(mac("/users/ME/library").is_some());
        assert!(mac("/APPLICATIONS").is_some());
    }

    #[test]
    fn macos_allows_ordinary_things_and_known_caches() {
        for p in [
            "/Users/me/Movies",
            "/Users/me/Downloads/big.dmg",
            "/Users/me/Developer/app/target",
            "/Applications/Some.app",
            "/Volumes/Backup/old/photos",
            "/Users/me/Library/Caches/com.example.app",
            "/Users/me/Library/Caches/com.example.app/blobs",
            "/Users/me/Library/Logs/DiagnosticReports",
            "/Users/me/Library/Developer/Xcode/DerivedData",
            "/Users/me/Library/Developer/Xcode/DerivedData/App-abc",
            "/Users/me/Applications/Tool.app",
            "/tmp/scratch",
            "/private/var/folders/xx/T/scratch",
        ] {
            assert_eq!(mac(p), None, "{p} may be moved");
        }
        for p in [
            "/Users/me/Library/Caches/com.example.app",
            "/Users/me/Library/Developer/Xcode/DerivedData",
            "/Users/me/Library/Logs/DiagnosticReports",
        ] {
            assert_eq!(
                cache_note(Os::MacOs, p, Some(MAC_HOME)),
                Some(CACHE_NOTE),
                "{p}"
            );
        }
        assert_eq!(
            cache_note(Os::MacOs, "/Users/me/Movies", Some(MAC_HOME)),
            None
        );
        // A refused path never carries a note.
        assert_eq!(
            cache_note(Os::MacOs, "/Users/me/Library/Caches", Some(MAC_HOME)),
            None
        );
    }

    #[test]
    fn macos_dot_dot_cannot_walk_into_a_protected_folder() {
        assert!(mac("/Users/me/Downloads/../Library").is_some());
        assert!(mac("/Users/me/./Library/").is_some());
    }

    #[test]
    fn linux_refuses_what_the_system_needs() {
        for p in [
            "/",
            "/usr",
            "/usr/lib/x86_64-linux-gnu",
            "/etc",
            "/etc/ssh/sshd_config",
            "/bin",
            "/boot",
            "/lib64",
            "/proc",
            "/sys",
            "/dev",
            "/root",
            "/var",
            "/var/lib",
            "/var/lib/dpkg",
            "/home",
            "/home/me",
            "/home/other",
            "/media",
            "/mnt",
            "/mnt/data",
            "/run/media/me",
            "/run/media/me/USB",
            "/home/me/.config",
            "/home/me/.local",
            "/home/me/.local/share",
            "/home/me/.cache",
            "/home/me/.ssh",
        ] {
            assert!(lin(p).is_some(), "{p} must be refused");
        }
        assert_eq!(lin("/usr"), Some("Linux needs this folder"));
        assert_eq!(lin("/home/me/.config"), Some("Linux needs this folder"));
        assert_eq!(lin("/mnt/data"), Some(VOLUME_REASON));
        assert_eq!(lin("/run/media/me/USB"), Some(VOLUME_REASON));
    }

    #[test]
    fn linux_is_case_sensitive_and_allows_ordinary_things() {
        assert_eq!(lin("/Usr"), None);
        for p in [
            "/home/me/Downloads/big.iso",
            "/home/me/projects/app/target",
            "/home/me/.cache/pip",
            "/home/me/.cache/pip/http",
            "/home/me/.config/some-app",
            "/mnt/data/old",
            "/run/media/me/USB/movies",
            "/tmp/scratch",
            "/var/tmp/scratch",
            "/var/cache/app",
            "/opt/app",
        ] {
            assert_eq!(lin(p), None, "{p} may be moved");
        }
        assert_eq!(
            cache_note(Os::Linux, "/home/me/.cache/pip", Some(LIN_HOME)),
            Some(CACHE_NOTE)
        );
        assert_eq!(
            cache_note(Os::Linux, "/home/me/.cache", Some(LIN_HOME)),
            None
        );
    }

    #[test]
    fn windows_refuses_what_the_system_needs() {
        for p in [
            r"C:\",
            r"C:",
            r"D:\",
            r"C:\Windows",
            r"C:\Windows\System32\drivers",
            r"C:\Program Files",
            r"C:\Program Files\App",
            r"C:\Program Files (x86)",
            r"C:\Program Files (x86)\Old App",
            r"C:\ProgramData",
            r"C:\Users",
            r"C:\Users\Me",
            r"C:\Users\Other",
            r"C:\Users\Public",
            r"C:\Users\Me\AppData",
            r"C:\Users\Me\AppData\Local",
            r"C:\Users\Me\AppData\Roaming",
            r"C:\Users\Me\AppData\LocalLow",
            r"C:\Users\Me\AppData\Local\Temp",
            r"C:\$Recycle.Bin",
            r"C:\System Volume Information",
            r"C:\Recovery",
            r"C:\Users\Me\.ssh",
        ] {
            assert!(win(p).is_some(), "{p} must be refused");
        }
        assert_eq!(win(r"C:\Windows"), Some("Windows needs this folder"));
        assert_eq!(win(r"D:\"), Some(VOLUME_REASON));
        assert_eq!(win(r"C:\Users\Me"), Some(HOME_REASON));
    }

    #[test]
    fn windows_ignores_case_and_slash_style() {
        assert!(win("c:/windows/system32").is_some());
        assert!(win(r"C:\WINDOWS").is_some());
        assert!(win(r"c:\users\me\appdata").is_some());
        assert!(win("C:/Program Files/App").is_some());
    }

    #[test]
    fn windows_allows_ordinary_things_and_known_caches() {
        for p in [
            r"C:\Users\Me\Downloads\big.iso",
            r"C:\Users\Me\Videos",
            r"D:\Backups\old",
            r"C:\Users\Me\AppData\Roaming\SomeApp",
            r"C:\Users\Me\AppData\Local\Temp\build-123",
            r"C:\Users\Me\AppData\Local\Google\Chrome\User Data\Default\Cache",
            r"C:\Users\Me\AppData\Local\Google\Chrome\User Data\Default\Cache\Cache_Data",
        ] {
            assert_eq!(win(p), None, "{p} may be moved");
        }
        for p in [
            r"C:\Users\Me\AppData\Local\Temp\build-123",
            r"C:\Users\Me\AppData\Local\Google\Chrome\User Data\Default\Cache",
        ] {
            assert_eq!(
                cache_note(Os::Windows, p, Some(WIN_HOME)),
                Some(CACHE_NOTE),
                "{p}"
            );
        }
        assert_eq!(
            cache_note(Os::Windows, r"C:\Users\Me\Videos", Some(WIN_HOME)),
            None
        );
    }

    #[test]
    fn without_a_home_the_system_rules_still_hold() {
        assert!(refusal_on(Os::MacOs, "/System", None).is_some());
        assert!(refusal_on(Os::Linux, "/etc", None).is_some());
        assert!(refusal_on(Os::Windows, r"C:\Windows", None).is_some());
        assert!(refusal_on(Os::MacOs, "/Users/me/Movies", None).is_none());
    }

    #[test]
    fn the_host_check_catches_a_link_into_a_protected_folder() {
        let dir = std::env::temp_dir().join(format!("panoply-protect-{}", std::process::id()));
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("Library")).unwrap();
        std::fs::create_dir_all(home.join("Movies")).unwrap();
        // The folder as written, with the stand-in home.
        let reason = refusal_for_home(&home.join("Library"), Some(&home));
        if Os::host() == Os::MacOs {
            assert_eq!(reason, Some("macOS needs this folder"));
        }
        assert_eq!(
            refusal_for_home(&home, Some(&home)),
            Some(HOME_REASON),
            "the home folder"
        );
        assert_eq!(refusal_for_home(&home.join("Movies"), Some(&home)), None);
        #[cfg(unix)]
        {
            // A link whose parent leads into a protected place is refused through the link.
            let link = dir.join("shortcut");
            std::os::unix::fs::symlink(&home, &link).unwrap();
            assert_eq!(
                refusal_for_home(&link.join("Movies"), Some(&home)),
                None,
                "an ordinary folder through a link is fine"
            );
            assert_eq!(
                refusal_for_home(&link, Some(&home)),
                None,
                "the link itself can be moved: only the link goes"
            );
            if Os::host() == Os::MacOs {
                assert!(refusal_for_home(&link.join("Library"), Some(&home)).is_some());
            }
        }
        // A cache folder inside the stand-in home carries its note when the home is named through a link.
        if Os::host() == Os::MacOs {
            let cache = home.join("Library/Caches/com.example.app");
            std::fs::create_dir_all(&cache).unwrap();
            #[cfg(unix)]
            {
                let via = dir.join("shortcut");
                assert_eq!(cache_note_for_home(&cache, Some(&via)), Some(CACHE_NOTE));
                assert_eq!(refusal_for_home(&cache, Some(&via)), None);
            }
        }
        // The root of a mounted volume (the root of the system drive) is refused.
        assert!(refusal_for_home(Path::new("/"), None).is_some() || cfg!(windows));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
