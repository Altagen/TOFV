use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::command::{plan_connect, PlanRequest, PlannedInvocation};
use crate::error::{Error, Result};
use crate::paths::{resolve_openfortivpn, AppConfig, AppPaths, Elevate};
use crate::profile::{validate_totp, Profile};

#[derive(Clone, Copy)]
pub struct ConnectRequest<'a> {
    pub profile: &'a Profile,
    pub otp: &'a str,
    pub paths: &'a AppPaths,
    pub app_config: &'a AppConfig,
    pub elevate: Elevate,
}

pub fn plan(req: ConnectRequest<'_>) -> Result<PlannedInvocation> {
    req.profile.validate_ready()?;
    validate_totp(req.otp)?;
    req.paths.ensure()?;
    let openfortivpn = resolve_openfortivpn(req.app_config)?;
    let config_path = req.paths.session_config_path(&req.profile.id);
    let mut cfg = req.app_config.clone();
    cfg.pinentry = Some(req.paths.pinentry_wrapper_path());
    plan_connect(PlanRequest {
        profile: req.profile,
        otp: req.otp,
        config_path: &config_path,
        openfortivpn: &openfortivpn,
        app_config: &cfg,
        elevate: req.elevate,
    })
}

/// Ephemeral openfortivpn config; unlinked on drop.
pub struct SessionFiles {
    pub config_path: PathBuf,
}

impl SessionFiles {
    pub fn create(path: &Path, body: &str) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| Error::IoPath {
                path: parent.to_path_buf(),
                source,
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
            }
        }
        let _ = fs::remove_file(path);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|source| Error::IoPath {
                path: path.to_path_buf(),
                source,
            })?;
        file.write_all(body.as_bytes())
            .map_err(|source| Error::IoPath {
                path: path.to_path_buf(),
                source,
            })?;
        file.sync_all().ok();
        Ok(Self {
            config_path: path.to_path_buf(),
        })
    }
}

impl Drop for SessionFiles {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.config_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn session_file_is_private_and_cleaned() {
        let root = std::env::temp_dir().join(format!("tofv-sess-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let paths = AppPaths::isolated(&root);
        paths.ensure().unwrap();
        let path = paths.session_config_path("default");
        {
            let files = SessionFiles::create(&path, "otp = 123456\n").unwrap();
            let mode = fs::metadata(&files.config_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
            assert!(path.exists());
        }
        assert!(!path.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_uses_runtime_dir() {
        let root = std::env::temp_dir().join(format!("tofv-plan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let paths = AppPaths::isolated(&root);
        let profile = Profile {
            host: "vpn.example.com".into(),
            username: "alice".into(),
            ..Profile::default()
        };
        let mut cfg = AppConfig::default();
        cfg.openfortivpn = Some(PathBuf::from("/usr/bin/openfortivpn"));

        // resolve_openfortivpn requires the file to exist; skip if missing.
        if !Path::new("/usr/bin/openfortivpn").is_file()
            && crate::secret::which("openfortivpn").is_none()
        {
            let _ = fs::remove_dir_all(&root);
            return;
        }

        // Force the path through config only if the binary exists.
        if let Some(found) = crate::secret::which("openfortivpn") {
            cfg.openfortivpn = Some(found);
        }

        let planned = plan(ConnectRequest {
            profile: &profile,
            otp: "123456",
            paths: &paths,
            app_config: &cfg,
            elevate: Elevate::None,
        });
        let _ = fs::remove_dir_all(&root);
        let planned = planned.unwrap();
        assert!(planned.config_path.ends_with("run/default.conf"));
        assert!(!planned.display.contains("123456"));
    }
}
