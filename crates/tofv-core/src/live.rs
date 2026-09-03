//! Detect a leftover `openfortivpn` started by TOFV after the UI died.

use std::fs;
use std::path::{Path, PathBuf};

/// A TOFV-owned `openfortivpn` still running (usually root, via the helper).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveTunnel {
    pub pid: u32,
    pub iface: Option<String>,
}

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

pub fn helper_pid_path(uid: u32) -> PathBuf {
    PathBuf::from(format!("/run/tofv/{uid}/vpn.pid"))
}

pub fn config_marker(uid: u32) -> String {
    format!("/run/user/{uid}/tofv/")
}

pub fn comm_is_openfortivpn(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|s| s.trim() == "openfortivpn")
        .unwrap_or(false)
}

pub fn cmdline_belongs_to_tofv(cmdline: &[u8], uid: u32) -> bool {
    let hay = cmdline
        .split(|b| *b == 0)
        .collect::<Vec<_>>()
        .join(&b" "[..]);
    let hay = String::from_utf8_lossy(&hay);
    hay.contains(&config_marker(uid)) || hay.contains(&format!("/run/tofv/{uid}/"))
}

pub fn detect_ppp_iface() -> Option<String> {
    for name in ["ppp0", "ppp1", "ppp2"] {
        if Path::new(&format!("/sys/class/net/{name}")).exists() {
            return Some(name.into());
        }
    }
    None
}

fn read_pid_file(path: &Path) -> Option<u32> {
    fs::read_to_string(path)
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn scan_proc_for_tofv(uid: u32) -> Option<u32> {
    let dir = fs::read_dir("/proc").ok()?;
    for ent in dir.flatten() {
        let pid: u32 = match ent.file_name().to_str().and_then(|s| s.parse().ok()) {
            Some(p) => p,
            None => continue,
        };
        if !comm_is_openfortivpn(pid) {
            continue;
        }
        let cmd = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        if cmdline_belongs_to_tofv(&cmd, uid) {
            return Some(pid);
        }
    }
    None
}

/// Look for a TOFV tunnel that outlived the UI (helper pid file or /proc).
pub fn probe_live_tunnel() -> Option<LiveTunnel> {
    let uid = current_uid();
    let pid = read_pid_file(&helper_pid_path(uid))
        .filter(|&pid| comm_is_openfortivpn(pid))
        .or_else(|| scan_proc_for_tofv(uid))?;
    Some(LiveTunnel {
        pid,
        iface: detect_ppp_iface(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_helper_argv() {
        let cmd = b"openfortivpn\0-c\0/run/user/1000/tofv/default.conf\0-v\0--no-ftm-push\0";
        assert!(cmdline_belongs_to_tofv(cmd, 1000));
        assert!(!cmdline_belongs_to_tofv(cmd, 1001));
        assert!(!cmdline_belongs_to_tofv(b"openfortivpn\0vpn.example.com\0", 1000));
        let root = b"openfortivpn\0-c\0/run/tofv/1000/session.conf\0";
        assert!(cmdline_belongs_to_tofv(root, 1000));
        assert!(!cmdline_belongs_to_tofv(root, 1001));
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
