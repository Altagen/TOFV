//! Runtime checks shared by `tofv doctor` and the GUI first-run gate.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::paths::{
    helper_version, openfortivpn_version, resolve_helper, resolve_openfortivpn,
    resolve_pinentry_bin, AppConfig,
};
use crate::secret::{which, SecretToolStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Distro {
    Arch,
    Debian,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorItem {
    pub id: String,
    pub ok: bool,
    pub blocking: bool,
    pub label: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    /// The running application's own version. The doctor answers "is this
    /// setup sane?", and it could not previously say which TOFV it was.
    pub version: String,
    pub blocking: bool,
    pub helper_ok: bool,
    pub tray_ok: bool,
    pub install_cmd: String,
    pub helper_cmd: String,
    pub items: Vec<DoctorItem>,
}

pub fn detect_distro() -> Distro {
    let text = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let id = text
        .lines()
        .find(|l| l.starts_with("ID="))
        .map(|l| l.trim_start_matches("ID=").trim_matches('"'))
        .unwrap_or("");
    match id {
        "arch" | "cachyos" | "manjaro" | "endeavouros" | "garuda" => Distro::Arch,
        "debian" | "ubuntu" | "linuxmint" | "pop" | "elementary" => Distro::Debian,
        _ => {
            if text.contains("arch") || Path::new("/usr/bin/pacman").is_file() {
                Distro::Arch
            } else if Path::new("/usr/bin/apt-get").is_file() {
                Distro::Debian
            } else {
                Distro::Unknown
            }
        }
    }
}

pub fn find_bin(name: &str, extras: &[&str]) -> Option<PathBuf> {
    if let Some(p) = which(name) {
        return Some(p);
    }
    extras.iter().map(PathBuf::from).find(|p| p.is_file())
}

pub fn appindicator_available() -> bool {
    const CANDIDATES: &[&str] = &[
        "/usr/lib/libayatana-appindicator3.so.1",
        "/usr/lib64/libayatana-appindicator3.so.1",
        "/usr/lib/libappindicator3.so.1",
        "/usr/lib64/libappindicator3.so.1",
        "/usr/lib/libayatana-appindicator3.so",
        "/usr/lib/x86_64-linux-gnu/libayatana-appindicator3.so.1",
    ];
    CANDIDATES.iter().any(|p| Path::new(p).exists())
}

/// Compiled-in version of whichever binary is asking.
const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn report() -> DoctorReport {
    let distro = detect_distro();
    let cfg = AppConfig::default();
    let mut items = Vec::new();

    match resolve_openfortivpn(&cfg) {
        Ok(p) => {
            let ver = openfortivpn_version(&p).unwrap_or_else(|| "?".into());
            items.push(item(
                "openfortivpn",
                true,
                true,
                format!("{} ({ver})", p.display()),
            ));
        }
        Err(_) => items.push(item(
            "openfortivpn",
            false,
            true,
            "missing — the tunnel cannot come up".into(),
        )),
    }

    match find_bin("pppd", &["/usr/sbin/pppd", "/usr/bin/pppd", "/sbin/pppd"]) {
        Some(p) => items.push(item("pppd", true, true, p.display().to_string())),
        None => items.push(item(
            "pppd",
            false,
            true,
            "missing (ppp package) — openfortivpn needs it".into(),
        )),
    }

    if SecretToolStore::is_available() {
        items.push(item(
            "secret-tool",
            true,
            true,
            "Secret Service keyring available".into(),
        ));
    } else {
        items.push(item(
            "secret-tool",
            false,
            true,
            "missing (libsecret) — the VPN password cannot be stored".into(),
        ));
    }

    match find_bin("pkexec", &["/usr/bin/pkexec"]) {
        Some(p) => items.push(item("pkexec", true, true, p.display().to_string())),
        None => items.push(item(
            "pkexec",
            false,
            true,
            "missing (polkit) — no elevation path for openfortivpn".into(),
        )),
    }

    match resolve_pinentry_bin(&cfg) {
        Ok(p) => items.push(item("pinentry", true, true, p.display().to_string())),
        Err(_) => items.push(item(
            "pinentry",
            false,
            true,
            "pinentry-tofv missing — install the helper (./scripts/install.sh)".into(),
        )),
    }

    let helper_ok = if let Some(p) = resolve_helper() {
        // The helper is installed separately and as root, so it can lag the
        // app by weeks without anything saying so. Report what is actually
        // on disk, and flag a mismatch: an older helper is missing whatever
        // has been fixed since.
        let installed = helper_version(&p);
        let (ok, detail) = match installed.as_deref() {
            Some(v) if v == VERSION => (true, format!("{} ({v})", p.display())),
            Some(v) => (
                false,
                format!(
                    "{} is {v} but this app is {VERSION} — re-run install-bin.sh / install-helper.sh",
                    p.display()
                ),
            ),
            // Helpers built before 0.1.1 checked for root before parsing argv,
            // so they cannot answer --version at all.
            None => (
                false,
                format!(
                    "{} predates 0.1.1 (cannot report its version) — re-run install-bin.sh",
                    p.display()
                ),
            ),
        };
        items.push(DoctorItem {
            id: "helper".into(),
            ok,
            // A stale helper still works; it is a warning, not a wall.
            blocking: false,
            label: "helper".into(),
            detail,
        });
        true
    } else {
        items.push(DoctorItem {
            id: "helper".into(),
            ok: false,
            blocking: true,
            label: "helper".into(),
            detail:
                "not installed — Connect refuses to run openfortivpn as root. ./scripts/install.sh"
                    .into(),
        });
        false
    };

    let tray_ok = appindicator_available();
    items.push(DoctorItem {
        id: "tray".into(),
        ok: tray_ok,
        blocking: false,
        label: "systray".into(),
        detail: if tray_ok {
            "libayatana-appindicator present".into()
        } else {
            "missing — the panel opens instead (no tray icon). On GNOME: the AppIndicator extension."
                .into()
        },
    });

    let blocking = items.iter().any(|i| i.blocking && !i.ok);
    let (install_cmd, helper_cmd) = match distro {
        Distro::Arch => (
            "sudo pacman -S openfortivpn ppp libsecret polkit libayatana-appindicator".into(),
            "./scripts/install.sh".into(),
        ),
        Distro::Debian => (
            "sudo apt install openfortivpn ppp libsecret-tools policykit-1 libayatana-appindicator3-1".into(),
            "./scripts/install.sh".into(),
        ),
        Distro::Unknown => (
            "installe openfortivpn, ppp, libsecret (secret-tool) et polkit (pkexec)".into(),
            "./scripts/install.sh".into(),
        ),
    };

    DoctorReport {
        version: VERSION.to_string(),
        blocking,
        helper_ok,
        tray_ok,
        install_cmd,
        helper_cmd,
        items,
    }
}

fn item(id: &str, is_ok: bool, blocking: bool, detail: String) -> DoctorItem {
    DoctorItem {
        id: id.into(),
        ok: is_ok,
        blocking,
        label: id.into(),
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_lists_core_checks() {
        let r = report();
        for id in [
            "openfortivpn",
            "pppd",
            "secret-tool",
            "pkexec",
            "helper",
            "tray",
        ] {
            assert!(r.items.iter().any(|i| i.id == id), "missing {id}");
        }
        assert!(!r.install_cmd.is_empty());
    }
}
