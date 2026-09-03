use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::openfortivpn_conf;
use crate::paths::{resolve_helper, resolve_pinentry, Elevate};
use crate::profile::{validate_totp, Profile};
use crate::AppConfig;

/// What TOFV would execute, with secrets already stripped from the display.
#[derive(Debug, Clone)]
pub struct PlannedInvocation {
    pub argv: Vec<String>,
    pub display: String,
    pub config_path: PathBuf,
    pub config_body: String,
    pub config_redacted: String,
    pub elevate: Elevate,
    pub openfortivpn: PathBuf,
    pub pinentry: PathBuf,
}

pub struct PlanRequest<'a> {
    pub profile: &'a Profile,
    pub otp: &'a str,
    pub config_path: &'a Path,
    pub openfortivpn: &'a Path,
    pub app_config: &'a AppConfig,
    pub elevate: Elevate,
}

pub fn plan_connect(req: PlanRequest<'_>) -> Result<PlannedInvocation> {
    req.profile.validate_ready()?;
    validate_totp(req.otp)?;

    let pinentry = resolve_pinentry(req.app_config);
    let config_body = openfortivpn_conf::render(req.profile, Some(req.otp));
    let config_redacted = openfortivpn_conf::redact_config(&config_body);

    let mut argv = Vec::new();
    match req.elevate {
        Elevate::None => {
            // Tests / dry unprivileged only. Never pkexec the VPN binary.
            argv.push(req.openfortivpn.display().to_string());
            argv.push("-c".into());
            argv.push(req.config_path.display().to_string());
            argv.push("--pinentry".into());
            argv.push(pinentry.display().to_string());
            argv.push("-v".into());
            argv.push("--no-ftm-push".into());
        }
        Elevate::Pkexec | Elevate::Sudo => {
            let helper = resolve_helper().ok_or(Error::HelperNotFound)?;
            match req.elevate {
                Elevate::Sudo => {
                    argv.push("sudo".into());
                    argv.push("--".into());
                }
                Elevate::Pkexec => argv.push("pkexec".into()),
                Elevate::None => unreachable!(),
            }
            argv.push(helper.display().to_string());
            argv.push("start".into());
            argv.push("--config".into());
            argv.push(req.config_path.display().to_string());
        }
    }

    forbid_dangerous_args(&argv);

    let display = argv.join(" ");
    debug_assert!(!display.contains(req.otp), "OTP leaked into argv display");

    Ok(PlannedInvocation {
        argv,
        display,
        config_path: req.config_path.to_path_buf(),
        config_body,
        config_redacted,
        elevate: req.elevate,
        openfortivpn: req.openfortivpn.to_path_buf(),
        pinentry,
    })
}

fn forbid_dangerous_args(argv: &[String]) {
    const BANNED: &[&str] = &[
        "--pppd-plugin",
        "--pppd-log",
        "--insecure-ssl",
        "--password",
        "-p",
        "--otp",
        "-o",
        "--cookie",
    ];
    for arg in argv {
        for banned in BANNED {
            if arg == banned || arg.starts_with(&format!("{banned}=")) {
                panic!("refusing to build argv containing {arg}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AppConfig;
    use crate::profile::Profile;
    use std::sync::Mutex;

    static HELPER_ENV: Mutex<()> = Mutex::new(());

    fn profile() -> Profile {
        Profile {
            host: "vpn.example.com".into(),
            port: 8443,
            username: "alice".into(),
            realm: "corp".into(),
            trusted_cert: Some(
                "e46d4aff08ba6914e64daa85bc6112a422fa7ce16631bff0b592a28556f993db".into(),
            ),
            ..Profile::default()
        }
    }

    #[test]
    fn dry_run_never_puts_secrets_in_argv() {
        let p = profile();
        let cfg = AppConfig::default();
        let plan = plan_connect(PlanRequest {
            profile: &p,
            otp: "123456",
            config_path: Path::new("/run/user/1000/tofv/default.conf"),
            openfortivpn: Path::new("/usr/bin/openfortivpn"),
            app_config: &cfg,
            elevate: Elevate::None,
        })
        .unwrap();

        assert_eq!(plan.argv[0], "/usr/bin/openfortivpn");
        assert!(plan.display.contains("-c /run/user/1000/tofv/default.conf"));
        assert!(plan.display.contains("--pinentry"));
        assert!(!plan.display.contains("123456"));
        assert!(!plan.argv.iter().any(|a| a.contains("123456")));
        assert!(!plan.argv.iter().any(|a| a.contains("password")));
        assert!(plan.config_body.contains("otp = 123456"));
        assert!(!plan.config_redacted.contains("123456"));
        assert!(plan.config_body.contains("trusted-cert ="));
        assert!(plan.config_body.contains("realm = corp"));
        assert!(!plan.argv.iter().any(|a| a.contains("pppd-plugin")));
        assert!(!plan.argv.iter().any(|a| a == "pkexec"));
    }

    #[test]
    fn elevate_without_helper_is_refused() {
        let _guard = HELPER_ENV.lock().unwrap();
        let prev = std::env::var("TOFV_HELPER").ok();
        std::env::remove_var("TOFV_HELPER");
        let installed = crate::paths::resolve_helper().is_some();
        let p = profile();
        let cfg = AppConfig::default();
        let result = plan_connect(PlanRequest {
            profile: &p,
            otp: "123456",
            config_path: Path::new("/run/user/1000/tofv/default.conf"),
            openfortivpn: Path::new("/usr/bin/openfortivpn"),
            app_config: &cfg,
            elevate: Elevate::Pkexec,
        });
        match prev {
            Some(v) => std::env::set_var("TOFV_HELPER", v),
            None => std::env::remove_var("TOFV_HELPER"),
        }
        if installed {
            return;
        }
        assert!(matches!(result.unwrap_err(), Error::HelperNotFound));
    }

    #[test]
    fn rejects_bad_totp() {
        let p = profile();
        let cfg = AppConfig::default();
        let err = plan_connect(PlanRequest {
            profile: &p,
            otp: "12",
            config_path: Path::new("/tmp/x.conf"),
            openfortivpn: Path::new("/usr/bin/openfortivpn"),
            app_config: &cfg,
            elevate: Elevate::None,
        })
        .unwrap_err();
        assert!(matches!(err, crate::error::Error::InvalidTotp));
    }

    #[test]
    fn elevate_uses_helper_not_raw_openfortivpn() {
        let _guard = HELPER_ENV.lock().unwrap();
        let fake = std::env::temp_dir().join(format!("tofv-fake-helper-{}", std::process::id()));
        std::fs::write(&fake, b"#!/bin/sh\n").unwrap();
        let prev = std::env::var("TOFV_HELPER").ok();
        std::env::set_var("TOFV_HELPER", &fake);
        let p = profile();
        let cfg = AppConfig::default();
        let plan = plan_connect(PlanRequest {
            profile: &p,
            otp: "654321",
            config_path: Path::new("/run/user/1000/tofv/default.conf"),
            openfortivpn: Path::new("/usr/bin/openfortivpn"),
            app_config: &cfg,
            elevate: Elevate::Pkexec,
        });
        match prev {
            Some(v) => std::env::set_var("TOFV_HELPER", v),
            None => std::env::remove_var("TOFV_HELPER"),
        }
        let _ = std::fs::remove_file(&fake);
        let plan = plan.unwrap();
        assert_eq!(plan.argv[0], "pkexec");
        assert_eq!(plan.argv[2], "start");
        assert!(plan.argv.iter().any(|a| a == "--config"));
        assert!(!plan.argv.iter().any(|a| a.ends_with("/openfortivpn")));
        assert!(!plan.display.contains("654321"));
    }
}
