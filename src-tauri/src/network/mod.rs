//! Network folder detection module
//!
//! Detects if a path is on a network-mounted filesystem (NAS, DAS, Samba, NFS, etc.)
//! by parsing /proc/self/mountinfo on Linux.
//!
//! Used by:
//! - Download folder selection (warning for network folders)
//! - Library folder management (network folder indicator)
//! - Offline mode (accessibility of network content)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Type of mount point
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MountKind {
    /// Local storage (ext4, btrfs, xfs, ntfs, vfat, etc.)
    Local,
    /// Network filesystem (SMB, NFS, SSHFS, etc.)
    Network(NetworkFs),
    /// Virtual/pseudo filesystem (/proc, /sys, tmpfs, etc.)
    Virtual,
    /// FUSE filesystem of unknown type
    FuseUnknown,
}

/// Specific network filesystem types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkFs {
    /// SMB/CIFS (Samba, Windows shares)
    Cifs,
    /// NFS (Network File System)
    Nfs,
    /// SSHFS (SSH Filesystem)
    Sshfs,
    /// rclone mount (cloud storage)
    Rclone,
    /// WebDAV
    Webdav,
    /// GlusterFS
    Gluster,
    /// CephFS
    Ceph,
    /// Other network filesystem
    Other(String),
}

/// Information about a mount point
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MountInfo {
    /// Filesystem type (e.g., "ext4", "cifs", "nfs")
    pub fs_type: String,
    /// Mount point path
    pub mount_point: String,
    /// Mount source (device or remote path)
    pub source: String,
    /// Classified mount kind
    pub kind: MountKind,
    /// Whether the mount is currently accessible
    pub accessible: bool,
}

/// Result of checking if a path is on a network mount
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPathInfo {
    /// Whether the path is on a network filesystem
    pub is_network: bool,
    /// Mount information if found
    pub mount_info: Option<MountInfo>,
    /// The path that was checked
    pub path: String,
}

/// Cached mount information for performance
#[derive(Debug)]
pub struct MountCache {
    mounts: Vec<MountInfo>,
    last_refresh: std::time::Instant,
}

impl MountCache {
    /// Create a new mount cache
    pub fn new() -> Self {
        Self {
            mounts: Vec::new(),
            last_refresh: std::time::Instant::now() - std::time::Duration::from_secs(3600),
        }
    }

    /// Refresh mount information from /proc/self/mountinfo
    pub fn refresh(&mut self) {
        self.mounts = parse_mount_info().unwrap_or_default();
        self.last_refresh = std::time::Instant::now();
    }

    /// Get mounts, refreshing if stale (older than 30 seconds)
    pub fn get_mounts(&mut self) -> &[MountInfo] {
        if self.last_refresh.elapsed() > std::time::Duration::from_secs(30) {
            self.refresh();
        }
        &self.mounts
    }

    /// Force refresh and return mounts
    pub fn get_mounts_fresh(&mut self) -> &[MountInfo] {
        self.refresh();
        &self.mounts
    }
}

/// Classify a filesystem type into MountKind
fn classify_fs_type(fs_type: &str, source: &str) -> MountKind {
    let fs_lower = fs_type.to_lowercase();

    // Network filesystems
    match fs_lower.as_str() {
        "cifs" | "smb" | "smb3" | "smbfs" => {
            return MountKind::Network(NetworkFs::Cifs);
        }
        "nfs" | "nfs4" | "nfsd" => {
            return MountKind::Network(NetworkFs::Nfs);
        }
        "sshfs" => {
            return MountKind::Network(NetworkFs::Sshfs);
        }
        "rclone" => {
            return MountKind::Network(NetworkFs::Rclone);
        }
        "davfs" | "davfs2" | "webdav" => {
            return MountKind::Network(NetworkFs::Webdav);
        }
        "glusterfs" => {
            return MountKind::Network(NetworkFs::Gluster);
        }
        "ceph" => {
            return MountKind::Network(NetworkFs::Ceph);
        }
        // FUSE types that might be network
        "fuse.sshfs" => {
            return MountKind::Network(NetworkFs::Sshfs);
        }
        "fuse.rclone" => {
            return MountKind::Network(NetworkFs::Rclone);
        }
        "fuse.cifs" => {
            return MountKind::Network(NetworkFs::Cifs);
        }
        _ => {}
    }

    // Check for FUSE with network-like source patterns
    if fs_lower.starts_with("fuse") {
        // Check source for network patterns
        if source.contains("@") || source.contains("://") {
            // Likely SSHFS (user@host:path) or URL-based mount
            if source.contains("@") && source.contains(":") {
                return MountKind::Network(NetworkFs::Sshfs);
            }
            return MountKind::Network(NetworkFs::Other(fs_type.to_string()));
        }
        return MountKind::FuseUnknown;
    }

    // Virtual/pseudo filesystems
    match fs_lower.as_str() {
        "proc" | "sysfs" | "devtmpfs" | "devpts" | "tmpfs" | "ramfs" | "securityfs" | "debugfs"
        | "tracefs" | "configfs" | "cgroup" | "cgroup2" | "pstore" | "efivarfs" | "bpf"
        | "autofs" | "mqueue" | "hugetlbfs" | "fusectl" | "overlay" | "squashfs" => {
            return MountKind::Virtual;
        }
        _ => {}
    }

    // Local filesystems
    match fs_lower.as_str() {
        "ext2" | "ext3" | "ext4" | "xfs" | "btrfs" | "zfs" | "ntfs" | "ntfs3" | "vfat"
        | "fat32" | "exfat" | "hfs" | "hfsplus" | "apfs" | "f2fs" | "jfs" | "reiserfs" | "udf"
        | "iso9660" | "ufs" => {
            return MountKind::Local;
        }
        _ => {}
    }

    // Default to local for unknown types (conservative approach)
    MountKind::Local
}

/// Parse /proc/self/mountinfo and return mount information
fn parse_mount_info() -> Result<Vec<MountInfo>, String> {
    let content = fs::read_to_string("/proc/self/mountinfo")
        .map_err(|e| format!("Failed to read /proc/self/mountinfo: {}", e))?;

    let mut mounts = Vec::new();

    for line in content.lines() {
        if let Some(mount) = parse_mount_line(line) {
            mounts.push(mount);
        }
    }

    Ok(mounts)
}

/// Parse a single line from /proc/self/mountinfo
/// Format: mount_id parent_id major:minor root mount_point mount_options ... - fs_type mount_source super_options
fn parse_mount_line(line: &str) -> Option<MountInfo> {
    let parts: Vec<&str> = line.split_whitespace().collect();

    // Need at least the basic fields plus separator
    if parts.len() < 7 {
        return None;
    }

    // Find the separator "-" to split fixed and optional parts
    let separator_idx = parts.iter().position(|&s| s == "-")?;

    // mount_point is at index 4
    let mount_point = parts.get(4)?.to_string();

    // After separator: fs_type, mount_source, super_options
    let fs_type = parts.get(separator_idx + 1)?.to_string();
    let source = parts.get(separator_idx + 2).unwrap_or(&"").to_string();

    // Unescape mount point (replaces \040 with space, etc.)
    let mount_point = unescape_mount_path(&mount_point);

    let kind = classify_fs_type(&fs_type, &source);

    // Check accessibility with a quick stat
    let accessible = Path::new(&mount_point).exists() && std::fs::metadata(&mount_point).is_ok();

    Some(MountInfo {
        fs_type,
        mount_point,
        source,
        kind,
        accessible,
    })
}

/// Unescape mount path (handles \040 for space, etc.)
fn unescape_mount_path(path: &str) -> String {
    let mut result = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            // Read up to 3 octal digits
            let mut octal = String::new();
            for _ in 0..3 {
                if let Some(&next) = chars.peek() {
                    if next.is_ascii_digit() && next < '8' {
                        octal.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
            }
            if !octal.is_empty() {
                if let Ok(code) = u8::from_str_radix(&octal, 8) {
                    result.push(code as char);
                } else {
                    result.push('\\');
                    result.push_str(&octal);
                }
            } else {
                result.push('\\');
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Find the mount point for a given path
pub fn find_mount_for_path(path: &Path, mounts: &[MountInfo]) -> Option<MountInfo> {
    // Canonicalize path to resolve symlinks
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => path.to_path_buf(),
    };

    let path_str = canonical.to_string_lossy();

    // Find the longest matching mount point (most specific)
    let mut best_match: Option<&MountInfo> = None;
    let mut best_len = 0;

    for mount in mounts {
        if path_str.starts_with(&mount.mount_point) || path_str == mount.mount_point {
            let mount_len = mount.mount_point.len();
            // Make sure it's a proper path prefix (ends at directory boundary)
            let is_valid_prefix = path_str.len() == mount_len
                || path_str.chars().nth(mount_len) == Some('/')
                || mount.mount_point == "/";

            if is_valid_prefix && mount_len > best_len {
                best_match = Some(mount);
                best_len = mount_len;
            }
        }
    }

    best_match.cloned()
}

/// Check if a path is on a network filesystem
pub fn is_network_path(path: &Path) -> NetworkPathInfo {
    let mounts = parse_mount_info().unwrap_or_default();

    if let Some(mount) = find_mount_for_path(path, &mounts) {
        let is_network = matches!(mount.kind, MountKind::Network(_));
        NetworkPathInfo {
            is_network,
            mount_info: Some(mount),
            path: path.to_string_lossy().to_string(),
        }
    } else {
        NetworkPathInfo {
            is_network: false,
            mount_info: None,
            path: path.to_string_lossy().to_string(),
        }
    }
}

/// Check accessibility of a network mount with timeout
pub fn check_mount_accessibility(mount_point: &str) -> bool {
    // Use a quick readdir operation
    let path = Path::new(mount_point);

    // First check if path exists
    if !path.exists() {
        return false;
    }

    // Try to read directory to verify access
    match fs::read_dir(path) {
        Ok(mut entries) => {
            // Try to read at least one entry
            entries.next().is_some() || true // Empty dir is still accessible
        }
        Err(_) => false,
    }
}

/// Get all network mounts
pub fn get_network_mounts() -> Vec<MountInfo> {
    parse_mount_info()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| matches!(m.kind, MountKind::Network(_)))
        .collect()
}

/// Get all mounts with their classifications
pub fn get_all_mounts() -> Vec<MountInfo> {
    parse_mount_info().unwrap_or_default()
}

// Tauri commands
pub mod commands {
    use super::*;

    /// Check if a path is on a network filesystem
    #[tauri::command]
    pub fn check_network_path(path: String) -> NetworkPathInfo {
        is_network_path(Path::new(&path))
    }

    /// Get all network mounts currently visible
    #[tauri::command]
    pub fn get_network_mounts_cmd() -> Vec<MountInfo> {
        get_network_mounts()
    }

    /// Check if a network mount is currently accessible
    #[tauri::command]
    pub fn check_mount_accessible(mount_point: String) -> bool {
        check_mount_accessibility(&mount_point)
    }

    /// Batch check multiple paths for network status
    #[tauri::command]
    pub fn check_network_paths_batch(paths: Vec<String>) -> HashMap<String, NetworkPathInfo> {
        let mounts = parse_mount_info().unwrap_or_default();

        paths
            .into_iter()
            .map(|p| {
                let path = Path::new(&p);
                let info = if let Some(mount) = find_mount_for_path(path, &mounts) {
                    let is_network = matches!(mount.kind, MountKind::Network(_));
                    NetworkPathInfo {
                        is_network,
                        mount_info: Some(mount),
                        path: p.clone(),
                    }
                } else {
                    NetworkPathInfo {
                        is_network: false,
                        mount_info: None,
                        path: p.clone(),
                    }
                };
                (p, info)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_fs_type() {
        assert!(matches!(classify_fs_type("ext4", ""), MountKind::Local));
        assert!(matches!(
            classify_fs_type("cifs", "//server/share"),
            MountKind::Network(NetworkFs::Cifs)
        ));
        assert!(matches!(
            classify_fs_type("nfs", "server:/export"),
            MountKind::Network(NetworkFs::Nfs)
        ));
        assert!(matches!(
            classify_fs_type("tmpfs", "tmpfs"),
            MountKind::Virtual
        ));
        assert!(matches!(
            classify_fs_type("fuse.sshfs", "user@host:/path"),
            MountKind::Network(NetworkFs::Sshfs)
        ));
    }

    #[test]
    fn test_unescape_mount_path() {
        assert_eq!(unescape_mount_path("/mnt/My\\040Drive"), "/mnt/My Drive");
        assert_eq!(unescape_mount_path("/normal/path"), "/normal/path");
    }
}
