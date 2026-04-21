//! Network-mount detection for local library paths.
//!
//! On Linux, reads /proc/mounts (or /run/host/proc/mounts as a fallback
//! for sandboxed apps like Flatpak / Snap) and classifies a given
//! filesystem path by the fs type of its longest-matching mount point.
//!
//! The UI consumes the resulting is_network_mount flag to mark tracks
//! as unreachable when the user is under forced offline mode (cable
//! unplugged / ISP down). In that state a path that still reads
//! /home/user/music can be sitting on a CIFS share or SSHFS — the
//! heuristic the frontend originally used (string-match /mnt, /media)
//! misses those cases entirely, especially inside sandboxes where the
//! user's music folder is commonly bind-mounted from an SMB share.

use std::path::Path;

/// Filesystem types that require network to be reachable. Matched as
/// a prefix against the fs_type column of /proc/mounts so variants
/// like `fuse.sshfs` / `fuse.rclone` / `nfs4` all hit the same rule.
const NETWORK_FS_PREFIXES: &[&str] = &[
    "nfs",
    "cifs",
    "smb",
    "smbfs",
    "smb3",
    "fuse.sshfs",
    "fuse.rclone",
    "fuse.gvfs",
    "fuse.gvfsd",
    "fuse.davfs",
    "fuse.rclonefs",
    "davfs",
    "webdav",
    "9p",
    "ceph",
    "glusterfs",
    "afs",
    "afp",
];

/// Return true when `path` lives on a network-backed filesystem.
///
/// Non-Linux platforms fall through to `false` — we don't have a
/// portable story for macOS / Windows yet. The frontend still has a
/// defensive string-match heuristic for UNC paths and common mount
/// prefixes, which picks up the easy cases on those platforms.
#[cfg(target_os = "linux")]
pub fn is_network_path(path: &Path) -> bool {
    let mounts = read_mounts();
    if mounts.is_empty() {
        return false;
    }

    // Canonicalize for best matching; fall back to raw path if the
    // file already disappeared / permission denied.
    let target = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf());
    let target_str = target.to_string_lossy();

    // Longest-prefix match wins — `/` is always present but any
    // deeper mount shadows it.
    let mut best: Option<(&str, usize)> = None;
    for (mount_point, fs_type) in &mounts {
        if target_str.starts_with(mount_point) {
            let len = mount_point.len();
            if best.map(|(_, l)| l < len).unwrap_or(true) {
                best = Some((fs_type.as_str(), len));
            }
        }
    }

    match best {
        Some((fs_type, _)) => is_network_fs(fs_type),
        None => false,
    }
}

#[cfg(not(target_os = "linux"))]
pub fn is_network_path(_path: &Path) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn read_mounts() -> Vec<(String, String)> {
    // Inside Flatpak the sandbox's own /proc/mounts reflects the
    // sandbox view, which is the right lens for the app. Snap is the
    // same. Both bind-mount the host share into the sandbox, so if the
    // host mount is CIFS, the sandbox sees fuse.* or the same fs type
    // (depending on the mechanism). /run/host/proc/mounts is the
    // Flatpak escape hatch when we need the raw host view, used as a
    // fallback for cases where the sandbox doesn't expose /proc/mounts.
    for path in ["/proc/mounts", "/run/host/proc/mounts"] {
        if let Ok(contents) = std::fs::read_to_string(path) {
            return parse_mounts(&contents);
        }
    }
    Vec::new()
}

#[cfg(target_os = "linux")]
fn parse_mounts(contents: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in contents.lines() {
        let mut parts = line.split_whitespace();
        let _device = parts.next();
        let mount_point = match parts.next() {
            Some(m) => m,
            None => continue,
        };
        let fs_type = match parts.next() {
            Some(t) => t,
            None => continue,
        };
        // /proc/mounts escapes spaces as \040, tabs as \011, etc.
        // Keep the raw string — starts_with on the pattern we care
        // about is unaffected, and canonicalize will bring our input
        // into the same encoding.
        out.push((mount_point.to_string(), fs_type.to_string()));
    }
    out
}

#[cfg(target_os = "linux")]
fn is_network_fs(fs_type: &str) -> bool {
    NETWORK_FS_PREFIXES
        .iter()
        .any(|prefix| fs_type == *prefix || fs_type.starts_with(&format!("{}.", prefix)))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn nfs_variants_classify_network() {
        assert!(is_network_fs("nfs"));
        assert!(is_network_fs("nfs4"));
        assert!(is_network_fs("cifs"));
        assert!(is_network_fs("smb3"));
        assert!(is_network_fs("fuse.sshfs"));
        assert!(is_network_fs("fuse.rclone"));
    }

    #[test]
    fn local_fs_does_not_classify() {
        assert!(!is_network_fs("ext4"));
        assert!(!is_network_fs("btrfs"));
        assert!(!is_network_fs("tmpfs"));
        assert!(!is_network_fs("fuse.gocryptfs"));
    }

    #[test]
    fn parse_mounts_reads_typical_entries() {
        let sample = "\
            /dev/sda1 / ext4 rw,relatime 0 0\n\
            tmpfs /run tmpfs rw,nosuid 0 0\n\
            nas:/music /mnt/music nfs4 rw,relatime 0 0\n\
        ";
        let parsed = parse_mounts(sample);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[2].0, "/mnt/music");
        assert_eq!(parsed[2].1, "nfs4");
    }
}
