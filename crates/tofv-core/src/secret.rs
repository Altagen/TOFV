//! Password storage. The VPN password never goes on disk in TOFV itself.
//!
//! Default Linux backend is FreeDesktop Secret Service via `secret-tool`
//! (libsecret). The wallet implementation is the desktop's: KWallet on
//! Plasma, gnome-keyring on GNOME, KeePassXC, etc. Tests use [`MemoryStore`].
//! macOS will be a Keychain store, same `PasswordStore` trait.

use std::io::Write;
use std::process::{Command, Stdio};

use zeroize::ZeroizeOnDrop;

use crate::error::{Error, Result};

const SERVICE: &str = "dev.tofv";

#[derive(Clone, ZeroizeOnDrop)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretString(***)")
    }
}

pub trait PasswordStore {
    fn get(&self, profile_id: &str) -> Result<Option<SecretString>>;
    fn set(&self, profile_id: &str, password: &str) -> Result<()>;
    fn delete(&self, profile_id: &str) -> Result<()>;
}

/// In-memory store, for tests and dry-runs that inject a password.
#[derive(Default)]
pub struct MemoryStore {
    inner: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PasswordStore for MemoryStore {
    fn get(&self, profile_id: &str) -> Result<Option<SecretString>> {
        let map = self
            .inner
            .lock()
            .map_err(|e| Error::Secret(e.to_string()))?;
        Ok(map.get(profile_id).cloned().map(SecretString::new))
    }

    fn set(&self, profile_id: &str, password: &str) -> Result<()> {
        let mut map = self
            .inner
            .lock()
            .map_err(|e| Error::Secret(e.to_string()))?;
        map.insert(profile_id.to_string(), password.to_string());
        Ok(())
    }

    fn delete(&self, profile_id: &str) -> Result<()> {
        let mut map = self
            .inner
            .lock()
            .map_err(|e| Error::Secret(e.to_string()))?;
        map.remove(profile_id);
        Ok(())
    }
}

/// `secret-tool` CLI (libsecret). Available on KDE once `libsecret` is installed.
#[derive(Debug, Default, Clone, Copy)]
pub struct SecretToolStore;

impl SecretToolStore {
    pub fn new() -> Self {
        Self
    }

    pub fn is_available() -> bool {
        which("secret-tool").is_some()
    }
}

impl PasswordStore for SecretToolStore {
    fn get(&self, profile_id: &str) -> Result<Option<SecretString>> {
        let tool = require_secret_tool()?;
        let output = Command::new(&tool)
            .args(["lookup", "service", SERVICE, "username", profile_id])
            .output()
            .map_err(|e| Error::Secret(format!("failed to run secret-tool: {e}")))?;

        if !output.status.success() {
            // secret-tool prints to stderr when the item is missing.
            return Ok(None);
        }
        let mut value = String::from_utf8(output.stdout)
            .map_err(|_| Error::Secret("secret-tool returned non-utf8 password".into()))?;
        if value.ends_with('\n') {
            value.pop();
            if value.ends_with('\r') {
                value.pop();
            }
        }
        if value.is_empty() {
            Ok(None)
        } else {
            Ok(Some(SecretString::new(value)))
        }
    }

    fn set(&self, profile_id: &str, password: &str) -> Result<()> {
        let tool = require_secret_tool()?;
        let mut child = Command::new(&tool)
            .args([
                "store",
                "--label",
                &format!("TOFV password ({profile_id})"),
                "service",
                SERVICE,
                "username",
                profile_id,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Secret(format!("failed to run secret-tool: {e}")))?;

        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| Error::Secret("secret-tool stdin is closed".into()))?;
            stdin
                .write_all(password.as_bytes())
                .map_err(|e| Error::Secret(format!("failed to write password: {e}")))?;
        }

        let output = child
            .wait_with_output()
            .map_err(|e| Error::Secret(format!("secret-tool failed: {e}")))?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Secret(format!(
                "secret-tool store failed: {}",
                err.trim()
            )));
        }
        Ok(())
    }

    fn delete(&self, profile_id: &str) -> Result<()> {
        let tool = require_secret_tool()?;
        let output = Command::new(&tool)
            .args(["clear", "service", SERVICE, "username", profile_id])
            .output()
            .map_err(|e| Error::Secret(format!("failed to run secret-tool: {e}")))?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Secret(format!(
                "secret-tool clear failed: {}",
                err.trim()
            )));
        }
        Ok(())
    }
}

fn require_secret_tool() -> Result<std::path::PathBuf> {
    which("secret-tool").ok_or_else(|| {
        Error::Secret(
            "secret-tool not found (install libsecret: KWallet, gnome-keyring, or another Secret Service daemon)".into(),
        )
    })
}

pub fn which(bin: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_roundtrip() {
        let store = MemoryStore::new();
        assert!(store.get("default").unwrap().is_none());
        store.set("default", "s3cret").unwrap();
        assert_eq!(store.get("default").unwrap().unwrap().expose(), "s3cret");
        store.delete("default").unwrap();
        assert!(store.get("default").unwrap().is_none());
    }

    #[test]
    fn secret_string_hides_debug() {
        let s = SecretString::new("hunter2");
        assert_eq!(format!("{s:?}"), "SecretString(***)");
        assert!(!format!("{s:?}").contains("hunter2"));
    }
}
