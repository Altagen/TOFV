//! Locate a TOFV-owned `openfortivpn` through `/proc`.
//!
//! Shared by `tofv-core` (adopting a tunnel that outlived the UI) and by the
//! root helper (refusing to start a second one, and knowing what to stop).
//!
//! This crate has **no dependencies on purpose**: `tofv-helper` runs as root
//! and links it, so anything pulled in here becomes part of the privileged
//! binary's supply chain.

use std::fs;
use std::path::PathBuf;

/// Where the helper keeps its root-owned session state.
pub fn priv_dir(uid: u32) -> PathBuf {
    PathBuf::from(format!("/run/tofv/{uid}"))
}

/// Where the unprivileged side writes the ephemeral openfortivpn config.
pub fn runtime_dir(uid: u32) -> PathBuf {
    PathBuf::from(format!("/run/user/{uid}/tofv"))
}

pub fn helper_pid_path(uid: u32) -> PathBuf {
    priv_dir(uid).join("vpn.pid")
}

pub fn comm_is_openfortivpn(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|s| s.trim() == "openfortivpn")
        .unwrap_or(false)
}

/// True when the argv mentions one of *our* config paths, so we never touch
/// an `openfortivpn` some other tool started.
pub fn cmdline_belongs_to_tofv(cmdline: &[u8], uid: u32) -> bool {
    let joined = cmdline
        .split(|b| *b == 0)
        .collect::<Vec<_>>()
        .join(&b" "[..]);
    let hay = String::from_utf8_lossy(&joined);
    hay.contains(&format!("/run/user/{uid}/tofv/")) || hay.contains(&format!("/run/tofv/{uid}/"))
}

/// Scan `/proc` for an `openfortivpn` started by this user's TOFV.
pub fn scan_proc(uid: u32) -> Option<u32> {
    let dir = fs::read_dir("/proc").ok()?;
    for entry in dir.flatten() {
        let pid: u32 = match entry.file_name().to_str().and_then(|s| s.parse().ok()) {
            Some(p) => p,
            None => continue,
        };
        if !comm_is_openfortivpn(pid) {
            continue;
        }
        let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        if cmdline_belongs_to_tofv(&cmdline, uid) {
            return Some(pid);
        }
    }
    None
}

/// The helper's pid file if it still points at a live `openfortivpn`,
/// otherwise a `/proc` scan.
pub fn find_session(uid: u32) -> Option<u32> {
    if let Ok(text) = fs::read_to_string(helper_pid_path(uid)) {
        if let Ok(pid) = text.trim().parse::<u32>() {
            if comm_is_openfortivpn(pid) {
                return Some(pid);
            }
        }
    }
    scan_proc(uid)
}

/// This process's real uid, read from `/proc` to avoid a libc dependency.
pub fn current_uid() -> u32 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_marker_is_scoped_to_the_calling_user() {
        let user_conf = b"openfortivpn\0-c\0/run/user/1000/tofv/default.conf\0-v\0--no-ftm-push\0";
        assert!(cmdline_belongs_to_tofv(user_conf, 1000));
        assert!(!cmdline_belongs_to_tofv(user_conf, 1001));

        let root_conf = b"openfortivpn\0-c\0/run/tofv/1000/session.conf\0";
        assert!(cmdline_belongs_to_tofv(root_conf, 1000));
        assert!(!cmdline_belongs_to_tofv(root_conf, 1001));

        // Someone else's openfortivpn is not ours to stop.
        assert!(!cmdline_belongs_to_tofv(
            b"openfortivpn\0vpn.example.com\0",
            1000
        ));
    }

    #[test]
    fn current_uid_matches_proc_self() {
        let expected: u32 = fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find(|l| l.starts_with("Uid:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .unwrap();
        assert_eq!(current_uid(), expected);
    }
}
