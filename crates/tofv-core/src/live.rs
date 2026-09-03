//! Detect an `openfortivpn` started by TOFV that outlived the UI.

use std::path::Path;

use tofv_scan::{current_uid, find_session};

/// A TOFV-owned `openfortivpn` still running (usually root, via the helper).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveTunnel {
    pub pid: u32,
    pub iface: Option<String>,
}

pub fn detect_ppp_iface() -> Option<String> {
    for name in ["ppp0", "ppp1", "ppp2"] {
        if Path::new(&format!("/sys/class/net/{name}")).exists() {
            return Some(name.into());
        }
    }
    None
}

/// Look for a TOFV tunnel that outlived the UI (helper pid file, else /proc).
pub fn probe_live_tunnel() -> Option<LiveTunnel> {
    let pid = find_session(current_uid())?;
    Some(LiveTunnel {
        pid,
        iface: detect_ppp_iface(),
    })
}
