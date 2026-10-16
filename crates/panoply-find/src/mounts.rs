//! Finding network mounts so a scan never walks into one without being told to.
//!
//! macOS: the output of `/sbin/mount`, one line per mount, file system type first inside the final
//! parentheses. Linux: `/proc/self/mountinfo`, file system type after the " - " separator. Windows is
//! handled by the Windows watcher work (drive types need the Win32 API).

use std::path::PathBuf;

const NETWORK_TYPES: &[&str] = &[
    "nfs",
    "nfs3",
    "nfs4",
    "cifs",
    "smbfs",
    "smb3",
    "afpfs",
    "webdav",
    "9p",
    "afs",
    "ceph",
    "glusterfs",
    "lustre",
    "sshfs",
    "fuse.sshfs",
    "autofs",
    "davfs",
    "fuse.rclone",
    "fuse.gvfsd-fuse",
];

fn is_network(fstype: &str) -> bool {
    NETWORK_TYPES.contains(&fstype)
}

/// Parses `mount` output from macOS or BSD: `//user@host/share on /Volumes/share (smbfs, nodev, nosuid)`.
pub fn parse_mount_output(text: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((_, rest)) = line.split_once(" on ") else {
            continue;
        };
        let Some((point, opts)) = rest.rsplit_once(" (") else {
            continue;
        };
        let fstype = opts.split([',', ')']).next().unwrap_or("").trim();
        if is_network(fstype) {
            out.push(PathBuf::from(point));
        }
    }
    out
}

/// Parses Linux `/proc/self/mountinfo`.
pub fn parse_mountinfo(text: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((pre, post)) = line.split_once(" - ") else {
            continue;
        };
        let fstype = post.split(' ').next().unwrap_or("");
        let point = pre.split(' ').nth(4);
        if let (true, Some(p)) = (is_network(fstype), point) {
            out.push(PathBuf::from(p.replace("\\040", " ")));
        }
    }
    out
}

/// Mount points of network file systems on this machine, best effort (empty when it cannot tell).
pub fn network_mounts() -> Vec<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        return std::fs::read_to_string("/proc/self/mountinfo")
            .map(|t| parse_mountinfo(&t))
            .unwrap_or_default();
    }
    #[cfg(target_os = "macos")]
    {
        return match std::process::Command::new("/sbin/mount").output() {
            Ok(o) if o.status.success() => parse_mount_output(&String::from_utf8_lossy(&o.stdout)),
            _ => Vec::new(),
        };
    }
    #[allow(unreachable_code)]
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_mount_output() {
        let t = "/dev/disk3s1s1 on / (apfs, sealed, local, read-only, journaled)\n\
                 //anzal@nas.local/media on /Volumes/media (smbfs, nodev, nosuid, mounted by anzal)\n\
                 map auto_home on /System/Volumes/Data/home (autofs, automounted, nobrowse)\n\
                 server:/export on /Volumes/export (nfs, nodev)\n";
        let m = parse_mount_output(t);
        assert_eq!(
            m,
            vec![
                PathBuf::from("/Volumes/media"),
                PathBuf::from("/System/Volumes/Data/home"),
                PathBuf::from("/Volumes/export")
            ]
        );
    }

    #[test]
    fn linux_mountinfo() {
        let t = "36 35 98:0 /mnt1 /mnt/with\\040space rw,noatime master:1 - ext4 /dev/root rw\n\
                 40 35 0:35 / /mnt/nas rw,relatime shared:2 - nfs4 nas:/vol rw,vers=4.2\n";
        assert_eq!(parse_mountinfo(t), vec![PathBuf::from("/mnt/nas")]);
    }
}
