use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const DEFAULT_PROFILE_ID: &str = "default";
pub const DEFAULT_PORT: u16 = 443;

/// FortiToken hardware / Mobile : saisie des 6 chiffres. Pas de seed OATH.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMethod {
    #[default]
    TotpManual,
    /// Anciennes valeurs `totp-show` / `totp-auto` dans un profil : traitées comme manuel.
    #[serde(other)]
    LegacyIgnored,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    #[serde(default = "default_id")]
    pub id: String,
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub realm: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trusted_cert: Option<String>,
    #[serde(default)]
    pub auth_method: AuthMethod,
}

fn default_id() -> String {
    DEFAULT_PROFILE_ID.to_string()
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            id: default_id(),
            host: String::new(),
            port: DEFAULT_PORT,
            username: String::new(),
            realm: String::new(),
            trusted_cert: None,
            auth_method: AuthMethod::TotpManual,
        }
    }
}

impl Profile {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(Error::ProfileNotFound(
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(DEFAULT_PROFILE_ID)
                    .to_string(),
            ));
        }
        let text = fs::read_to_string(path).map_err(|source| Error::IoPath {
            path: path.to_path_buf(),
            source,
        })?;
        let mut profile: Profile = toml::from_str(&text)?;
        if profile.id.is_empty() {
            profile.id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(DEFAULT_PROFILE_ID)
                .to_string();
        }
        profile.validate_fields()?;
        Ok(profile)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate_fields()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| Error::IoPath {
                path: parent.to_path_buf(),
                source,
            })?;
            let mut perms = fs::metadata(parent)
                .map_err(|source| Error::IoPath {
                    path: parent.to_path_buf(),
                    source,
                })?
                .permissions();
            perms.set_mode(0o700);
            fs::set_permissions(parent, perms).ok();
        }

        let mut stored = self.clone();
        stored.auth_method = AuthMethod::TotpManual;
        let body = toml::to_string_pretty(&stored)?;
        let tmp = path.with_extension("toml.tmp");
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)
                .map_err(|source| Error::IoPath {
                    path: tmp.clone(),
                    source,
                })?;
            file.write_all(body.as_bytes())
                .map_err(|source| Error::IoPath {
                    path: tmp.clone(),
                    source,
                })?;
            file.sync_all().ok();
        }
        fs::rename(&tmp, path).map_err(|source| Error::IoPath {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(())
    }

    /// Fields that can be stored; does not require host/username yet
    /// (the user may save a partial profile from the panel).
    pub fn validate_fields(&self) -> Result<()> {
        if self.id.is_empty() {
            return Err(Error::InvalidProfile("id is empty".into()));
        }
        if self.port == 0 {
            return Err(Error::InvalidProfile("port must be 1-65535".into()));
        }
        if let Some(cert) = &self.trusted_cert {
            validate_trusted_cert(cert)?;
        }
        if self.host.contains('\n') || self.username.contains('\n') || self.realm.contains('\n') {
            return Err(Error::InvalidProfile("fields must be single-line".into()));
        }
        Ok(())
    }

    pub fn validate_ready(&self) -> Result<()> {
        self.validate_fields()?;
        if self.host.trim().is_empty() {
            return Err(Error::InvalidProfile("host is required".into()));
        }
        if self.username.trim().is_empty() {
            return Err(Error::InvalidProfile("username is required".into()));
        }
        Ok(())
    }

    pub fn set_trusted_cert(&mut self, cert: &str) -> Result<()> {
        validate_trusted_cert(cert)?;
        self.trusted_cert = Some(cert.to_ascii_lowercase());
        Ok(())
    }
}

pub fn validate_trusted_cert(cert: &str) -> Result<()> {
    let cert = cert.trim();
    if cert.len() != 64 || !cert.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::InvalidTrustedCert);
    }
    Ok(())
}

pub fn validate_totp(otp: &str) -> Result<()> {
    if otp.len() == 6 && otp.bytes().all(|b| b.is_ascii_digit()) {
        Ok(())
    } else {
        Err(Error::InvalidTotp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn roundtrip_toml() {
        let dir = std::env::temp_dir().join(format!("tofv-profile-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("default.toml");

        let mut p = Profile {
            host: "vpn.example.com".into(),
            port: 8443,
            username: "alice".into(),
            realm: "corp".into(),
            ..Profile::default()
        };
        p.set_trusted_cert("e46d4aff08ba6914e64daa85bc6112a422fa7ce16631bff0b592a28556f993db")
            .unwrap();
        p.save(&path).unwrap();

        let loaded = Profile::load(&path).unwrap();
        assert_eq!(loaded.host, "vpn.example.com");
        assert_eq!(loaded.port, 8443);
        assert_eq!(loaded.username, "alice");
        assert_eq!(loaded.realm, "corp");
        assert_eq!(
            loaded.trusted_cert.as_deref(),
            Some("e46d4aff08ba6914e64daa85bc6112a422fa7ce16631bff0b592a28556f993db")
        );

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("password"));
        assert!(!text.contains("otp ="));
        assert!(!text.contains("totp-seed"));
        assert_eq!(loaded.auth_method, AuthMethod::TotpManual);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_bad_cert() {
        assert!(validate_trusted_cert("deadbeef").is_err());
        assert!(validate_trusted_cert(&"g".repeat(64)).is_err());
        assert!(validate_trusted_cert(&"ab".repeat(32)).is_ok());
    }

    #[test]
    fn totp_must_be_six_digits() {
        assert!(validate_totp("123456").is_ok());
        assert!(validate_totp("12345").is_err());
        assert!(validate_totp("1234567").is_err());
        assert!(validate_totp("12345a").is_err());
    }

    #[test]
    fn ready_requires_host_and_user() {
        let mut p = Profile::default();
        assert!(p.validate_ready().is_err());
        p.host = "vpn.example.com".into();
        assert!(p.validate_ready().is_err());
        p.username = "alice".into();
        assert!(p.validate_ready().is_ok());
    }
}
