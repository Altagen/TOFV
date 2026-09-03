use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::profile::{Profile, DEFAULT_PROFILE_ID};
use crate::secret::which;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Elevate {
    #[default]
    Pkexec,
    Sudo,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openfortivpn: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinentry: Option<PathBuf>,
    #[serde(default)]
    pub elevate: Elevate,
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub runtime_dir: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let config_dir = xdg_dir("XDG_CONFIG_HOME", ".config")?.join("tofv");
        let runtime_dir = runtime_root()?.join("tofv");
        Ok(Self {
            config_dir,
            runtime_dir,
        })
    }

    #[cfg(test)]
    pub fn isolated(root: &Path) -> Self {
        Self {
            config_dir: root.join("config"),
            runtime_dir: root.join("run"),
        }
    }

    pub fn ensure(&self) -> Result<()> {
        mkdir_private(&self.config_dir)?;
        mkdir_private(&self.config_dir.join("profiles"))?;
        mkdir_private(&self.runtime_dir)?;
        Ok(())
    }

    pub fn app_config_path(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn profiles_dir(&self) -> PathBuf {
        self.config_dir.join("profiles")
    }

    pub fn profile_path(&self, id: &str) -> PathBuf {
        self.profiles_dir().join(format!("{id}.toml"))
    }

    pub fn load_app_config(&self) -> Result<AppConfig> {
        let path = self.app_config_path();
        if !path.exists() {
            return Ok(AppConfig::default());
        }
        let text = fs::read_to_string(&path).map_err(|source| Error::IoPath {
            path: path.clone(),
            source,
        })?;
        Ok(toml::from_str(&text)?)
    }

    pub fn save_app_config(&self, cfg: &AppConfig) -> Result<()> {
        self.ensure()?;
        let path = self.app_config_path();
        fs::write(&path, toml::to_string_pretty(cfg)?)
            .map_err(|source| Error::IoPath { path, source })
    }

    pub fn load_profile(&self, id: &str) -> Result<Profile> {
        Profile::load(&self.profile_path(id))
    }

    pub fn load_default_profile(&self) -> Result<Profile> {
        match self.load_profile(DEFAULT_PROFILE_ID) {
            Ok(p) => Ok(p),
            Err(Error::ProfileNotFound(_)) => Ok(Profile::default()),
            Err(e) => Err(e),
        }
    }

    pub fn save_profile(&self, profile: &Profile) -> Result<()> {
        self.ensure()?;
        profile.save(&self.profile_path(&profile.id))
    }

    pub fn session_config_path(&self, profile_id: &str) -> PathBuf {
        self.runtime_dir.join(format!("{profile_id}.conf"))
    }

    pub fn pinentry_socket_path(&self) -> PathBuf {
        self.runtime_dir.join("pinentry.sock")
    }

    /// Tiny shell wrapper invoked by root `openfortivpn --pinentry=`.
    /// Hard-codes `TOFV_PINENTRY_SOCKET` so pkexec's env wipe does not matter.
    pub fn pinentry_wrapper_path(&self) -> PathBuf {
        self.runtime_dir.join("pinentry")
    }

    pub fn session_pid_path(&self) -> PathBuf {
        self.runtime_dir.join("session.pid")
    }

    /// Unix socket: second `tofv-app` asks the first to show the panel.
    pub fn app_socket_path(&self) -> PathBuf {
        self.runtime_dir.join("app.sock")
    }
}

/// Allowlisted root helper installed by `scripts/install-helper.sh`.
pub fn resolve_helper() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TOFV_HELPER") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    for p in [
        "/usr/local/libexec/tofv-helper",
        "/usr/libexec/tofv-helper",
        "/usr/lib/tofv/tofv-helper",
    ] {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

pub fn resolve_openfortivpn(cfg: &AppConfig) -> Result<PathBuf> {
    if let Ok(p) = std::env::var("TOFV_OPENFORTIVPN") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
    }
    if let Some(p) = &cfg.openfortivpn {
        if p.is_file() {
            return Ok(p.clone());
        }
        if p.is_absolute() {
            return Err(Error::OpenfortivpnNotFound);
        }
        if let Some(found) = which(&p.to_string_lossy()) {
            return Ok(found);
        }
    }
    which("openfortivpn").ok_or(Error::OpenfortivpnNotFound)
}

pub fn resolve_pinentry(cfg: &AppConfig) -> PathBuf {
    if let Ok(p) = std::env::var("TOFV_PINENTRY") {
        return PathBuf::from(p);
    }
    if let Some(p) = &cfg.pinentry {
        return p.clone();
    }
    resolve_pinentry_bin(cfg).unwrap_or_else(|_| PathBuf::from("pinentry-tofv"))
}

/// Absolute `pinentry-tofv` binary. Never returns the session wrapper.
pub fn resolve_pinentry_bin(cfg: &AppConfig) -> Result<PathBuf> {
    if let Ok(p) = std::env::var("TOFV_PINENTRY") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path.canonicalize().unwrap_or(path));
        }
    }
    if let Some(p) = &cfg.pinentry {
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name != "pinentry" && p.is_file() {
            return Ok(p.canonicalize().unwrap_or_else(|_| p.clone()));
        }
    }
    if let Some(p) = which("pinentry-tofv") {
        return Ok(p.canonicalize().unwrap_or(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("pinentry-tofv");
            if candidate.is_file() {
                return Ok(candidate.canonicalize().unwrap_or(candidate));
            }
        }
    }
    for p in [
        "/usr/local/libexec/pinentry-tofv",
        "/usr/libexec/pinentry-tofv",
        "/usr/lib/tofv/pinentry-tofv",
    ] {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(Error::PinentryNotFound)
}

pub fn sh_single_quote(path: &Path) -> String {
    let s = path.to_string_lossy();
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

/// Write a 0700 wrapper that re-injects the socket path, then execs pinentry-tofv.
pub fn install_pinentry_wrapper(wrapper: &Path, pinentry_bin: &Path, socket: &Path) -> Result<()> {
    if let Some(parent) = wrapper.parent() {
        mkdir_private(parent)?;
    }
    let body = format!(
        "#!/bin/sh\nexport TOFV_PINENTRY_SOCKET={}\nexec {} \"$@\"\n",
        sh_single_quote(socket),
        sh_single_quote(pinentry_bin)
    );
    let _ = fs::remove_file(wrapper);
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(wrapper)
            .map_err(|source| Error::IoPath {
                path: wrapper.to_path_buf(),
                source,
            })?;
        file.write_all(body.as_bytes())
            .map_err(|source| Error::IoPath {
                path: wrapper.to_path_buf(),
                source,
            })?;
        file.sync_all().ok();
    }
    Ok(())
}

/// Version of an installed `tofv-helper`, or `None` when it cannot say.
///
/// The helper is installed separately from the app — the tarball's
/// `install-bin.sh` puts it in `/usr/local/libexec` as root — so the two can
/// drift. They did: a helper from an earlier build kept running for weeks
/// after the app was updated, and nothing surfaced it.
///
/// A helper built before 0.1.1 checked for root before parsing argv, so it
/// answers `--version` with a privilege error rather than a version. That
/// silence is itself the signal: it means the binary predates 0.1.1.
pub fn helper_version(bin: &Path) -> Option<String> {
    let output = std::process::Command::new(bin)
        .arg("--version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().next()?.trim();
    line.strip_prefix("tofv-helper ")
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

pub fn openfortivpn_version(bin: &Path) -> Option<String> {
    let output = std::process::Command::new(bin)
        .arg("--version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().next().unwrap_or(text.trim());
    let trimmed = line.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn runtime_root() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    let uid = uid();
    let fallback = PathBuf::from(format!("/run/user/{uid}"));
    if fallback.is_dir() {
        return Ok(fallback);
    }
    Err(Error::Path(
        "XDG_RUNTIME_DIR is unset and /run/user/<uid> is missing".into(),
    ))
}

fn xdg_dir(var: &str, home_suffix: &str) -> Result<PathBuf> {
    if let Ok(dir) = std::env::var(var) {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    let home = std::env::var("HOME").map_err(|_| Error::Path("$HOME is unset".into()))?;
    Ok(PathBuf::from(home).join(home_suffix))
}

fn mkdir_private(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|source| Error::IoPath {
        path: path.to_path_buf(),
        source,
    })?;
    let mut perms = fs::metadata(path)
        .map_err(|source| Error::IoPath {
            path: path.to_path_buf(),
            source,
        })?
        .permissions();
    perms.set_mode(0o700);
    fs::set_permissions(path, perms).ok();
    Ok(())
}

fn uid() -> u32 {
    libc_uid()
}

#[cfg(unix)]
fn libc_uid() -> u32 {
    // libc is not a dependency; use the syscall via std where possible.
    // nix/libc avoided on purpose to keep the container image tiny.
    use std::fs;
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_layout() {
        let root = std::env::temp_dir().join(format!("tofv-paths-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let paths = AppPaths::isolated(&root);
        paths.ensure().unwrap();
        assert!(paths.profiles_dir().is_dir());
        let mode = fs::metadata(&paths.runtime_dir)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
        let _ = fs::remove_dir_all(&root);
    }
}
