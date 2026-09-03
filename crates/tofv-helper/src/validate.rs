use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use tofv_scan::runtime_dir;

/// Keys the GUI actually emits. Anything else (pppd-*, resolvconf, …) is refused.
const ALLOWED_KEYS: &[&str] = &[
    "host",
    "port",
    "username",
    "realm",
    "trusted-cert",
    "otp",
    "no-ftm-push",
];

const BANNED_KEYS: &[&str] = &[
    "password",
    "passwd",
    "pinentry",
    "pppd-plugin",
    "pppd-log",
    "pppd-call",
    "pppd-ipparam",
    "insecure-ssl",
    "cookie",
    "user-key",
    "pem-passphrase",
];

pub fn caller_uid() -> Result<u32, String> {
    for var in ["PKEXEC_UID", "SUDO_UID"] {
        if let Ok(v) = std::env::var(var) {
            if let Ok(uid) = v.parse::<u32>() {
                if uid != 0 {
                    return Ok(uid);
                }
            }
        }
    }
    Err("PKEXEC_UID/SUDO_UID missing — refuse to run without a calling user".into())
}

/// Open the config and validate the **file descriptor**, then read from that
/// same descriptor.
///
/// Checking a path and then re-opening it is a TOCTOU: the caller owns
/// `/run/user/<uid>/tofv`, so between the check and the open it can swap the
/// file (or a parent directory) for a symlink and make this root process read
/// something else. Validating the fd we actually read from closes that window:
/// whatever we end up holding must still be a regular file owned by the
/// calling user and mode 0600, so it can never be a file they do not own.
pub fn read_config_checked(path: &Path, uid: u32) -> Result<(PathBuf, String), String> {
    read_config_checked_under(path, uid, &runtime_dir(uid))
}

pub fn read_config_checked_under(
    path: &Path,
    uid: u32,
    expected: &Path,
) -> Result<(PathBuf, String), String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;

    // Cheap early rejection with a clear message; the fd checks below are
    // what actually enforce the guarantee.
    let canon = validate_config_path_under(path, uid, expected)?;

    // O_NOFOLLOW: if the final component became a symlink after the check,
    // this fails instead of following it.
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NOCTTY)
        .open(&canon)
        .map_err(|e| format!("config {}: {e}", canon.display()))?;

    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("config is not a regular file".into());
    }
    if meta.uid() != uid {
        return Err("config is not owned by the calling user".into());
    }
    if meta.mode() & 0o077 != 0 {
        return Err("config must be mode 0600 (no group/other access)".into());
    }
    if meta.nlink() != 1 {
        return Err("config must not be hard-linked".into());
    }
    if meta.len() > MAX_CONFIG_BYTES {
        return Err(format!("config larger than {MAX_CONFIG_BYTES} bytes"));
    }

    let mut body = String::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut body)
        .map_err(|_| "config is not valid UTF-8".to_string())?;
    if body.len() as u64 > MAX_CONFIG_BYTES {
        return Err(format!("config larger than {MAX_CONFIG_BYTES} bytes"));
    }
    Ok((canon, body))
}

/// A rendered profile is a few hundred bytes; anything larger is not ours.
const MAX_CONFIG_BYTES: u64 = 16 * 1024;

/// `expected` is `/run/user/<uid>/tofv` in production. Tests pass a temp dir.
pub fn validate_config_path_under(
    path: &Path,
    uid: u32,
    expected: &Path,
) -> Result<PathBuf, String> {
    let expected = expected
        .canonicalize()
        .map_err(|e| format!("runtime dir {}: {e}", expected.display()))?;
    let canon = path
        .canonicalize()
        .map_err(|e| format!("config {}: {e}", path.display()))?;
    if !canon.starts_with(&expected) {
        return Err(format!(
            "config must live under {} (got {})",
            expected.display(),
            canon.display()
        ));
    }
    if canon.extension().and_then(|s| s.to_str()) != Some("conf") {
        return Err("config must end in .conf".into());
    }
    let meta = fs::metadata(&canon).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("config is not a file".into());
    }
    if meta.uid() != uid {
        return Err("config is not owned by the calling user".into());
    }
    if meta.mode() & 0o077 != 0 {
        return Err("config must be mode 0600 (no group/other access)".into());
    }
    Ok(canon)
}

/// Errors here are printed on stderr and end up in the GUI log, so they name
/// the line *number* and never echo file content — otherwise a rejected file
/// would be an exfiltration channel for whatever we were pointed at.
pub fn validate_config_body(body: &str) -> Result<(), String> {
    for (n, raw) in body.lines().enumerate() {
        let n = n + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("line {n}: not a `key = value` line"))?;
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        // Naming a *known* key is safe: it is one of our own constants.
        if BANNED_KEYS.contains(&key.as_str()) {
            return Err(format!("line {n}: forbidden config key `{key}`"));
        }
        if !ALLOWED_KEYS.contains(&key.as_str()) {
            return Err(format!("line {n}: unknown config key"));
        }
        if value.is_empty() && key != "realm" {
            return Err(format!("line {n}: empty value for `{key}`"));
        }
        if value.contains('\0') {
            return Err(format!("line {n}: NUL byte"));
        }
        validate_value(&key, value).map_err(|e| format!("line {n}: {e}"))?;
    }
    Ok(())
}

fn validate_value(key: &str, value: &str) -> Result<(), String> {
    match key {
        "host" => {
            if value
                .chars()
                .any(|c| c.is_whitespace() || c == '/' || c == '=')
            {
                return Err("invalid host".into());
            }
            if !value.chars().all(|c| {
                c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']')
            }) {
                return Err("invalid host".into());
            }
        }
        "port" => {
            let p: u16 = value.parse().map_err(|_| "invalid port".to_string())?;
            if p == 0 {
                return Err("invalid port".into());
            }
        }
        "username" | "realm" => {
            if value
                .chars()
                .any(|c| c.is_control() || c == '=' || c.is_whitespace())
            {
                return Err(format!("invalid {key}"));
            }
        }
        "trusted-cert" => {
            if value.len() != 64 || !value.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err("trusted-cert must be 64 hex chars".into());
            }
        }
        "otp" => {
            if value.len() != 6 || !value.chars().all(|c| c.is_ascii_digit()) {
                return Err("otp must be 6 digits".into());
            }
        }
        "no-ftm-push" if value != "1" => {
            return Err("no-ftm-push must be 1".into());
        }
        _ => {}
    }
    Ok(())
}

pub fn allowed_openfortivpn(path: &Path) -> Result<PathBuf, String> {
    let canon = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let ok = matches!(
        canon.to_str(),
        Some("/usr/bin/openfortivpn") | Some("/usr/local/bin/openfortivpn")
    );
    if !ok {
        return Err(format!("refusing openfortivpn path {}", canon.display()));
    }
    let meta = fs::metadata(&canon).map_err(|e| e.to_string())?;
    if meta.uid() != 0 {
        return Err("openfortivpn is not root-owned".into());
    }
    if meta.mode() & 0o022 != 0 {
        return Err("openfortivpn is writable by group/other".into());
    }
    Ok(canon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    fn uid() -> u32 {
        fs::metadata("/proc/self").unwrap().uid()
    }

    fn sandbox(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "tofv-helper-{}-{}-{tag}",
            std::process::id(),
            uid()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_mode(path: &Path, body: &str, mode: u32) {
        fs::write(path, body).unwrap();
        let mut p = fs::metadata(path).unwrap().permissions();
        p.set_mode(mode);
        fs::set_permissions(path, p).unwrap();
    }

    #[test]
    fn rejects_password_and_plugin() {
        assert!(validate_config_body("host = vpn\npassword = x\n").is_err());
        assert!(validate_config_body("host = vpn\npppd-plugin = /tmp/x.so\n").is_err());
        assert!(validate_config_body("host = vpn\ninsecure-ssl = 1\n").is_err());
        assert!(validate_config_body(
            "host = vpn.example.com\nport = 443\nusername = a\notp = 123456\nno-ftm-push = 1\n"
        )
        .is_ok());
    }

    #[test]
    fn rejects_unknown_key() {
        assert!(validate_config_body("host = vpn\nevil = 1\n").is_err());
        assert!(validate_config_body("host = vpn\nuse-resolvconf = /tmp/x\n").is_err());
    }

    #[test]
    fn rejects_bad_values() {
        assert!(validate_config_body(
            "host = vpn/../etc\nport = 443\nusername = a\notp = 123456\nno-ftm-push = 1\n"
        )
        .is_err());
        assert!(validate_config_body(
            "host = vpn.example.com\nport = 0\nusername = a\notp = 123456\nno-ftm-push = 1\n"
        )
        .is_err());
        assert!(validate_config_body(
            "host = vpn.example.com\nport = 443\nusername = a\notp = 12\nno-ftm-push = 1\n"
        )
        .is_err());
        assert!(validate_config_body(
            "host = vpn.example.com\nport = 443\nusername = a\notp = 123456\nno-ftm-push = 0\n"
        )
        .is_err());
        assert!(validate_config_body("host = vpn.example.com\nport = 443\nusername = a\ntrusted-cert = deadbeef\notp = 123456\nno-ftm-push = 1\n").is_err());
    }

    #[test]
    fn path_must_stay_under_runtime() {
        let root = sandbox("path");
        let expected = root.join("tofv");
        fs::create_dir_all(&expected).unwrap();
        let ok = expected.join("default.conf");
        write_mode(&ok, "host = vpn.example.com\n", 0o600);
        assert!(validate_config_path_under(&ok, uid(), &expected).is_ok());

        let outside = root.join("shadow.conf");
        write_mode(&outside, "host = x\n", 0o600);
        let link = expected.join("evil.conf");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let err = validate_config_path_under(&link, uid(), &expected).unwrap_err();
        assert!(err.contains("must live under"), "{err}");

        let world = expected.join("open.conf");
        write_mode(&world, "host = vpn.example.com\n", 0o644);
        assert!(validate_config_path_under(&world, uid(), &expected).is_err());

        let txt = expected.join("notes.txt");
        write_mode(&txt, "host = vpn.example.com\n", 0o600);
        assert!(validate_config_path_under(&txt, uid(), &expected).is_err());

        let _ = fs::remove_dir_all(&root);
    }

    /// The bug this guards: `validate_config_path` + a separate
    /// `fs::read_to_string` let the (unprivileged) caller swap the file for a
    /// symlink after the check, so this root process would read the target and
    /// then echo a line of it back in the rejection message.
    #[test]
    fn swapping_the_file_after_the_check_cannot_read_another_users_file() {
        let root = sandbox("toctou");
        let expected = root.join("tofv");
        fs::create_dir_all(&expected).unwrap();
        let conf = expected.join("default.conf");
        write_mode(&conf, "host = vpn.example.com\n", 0o600);

        // Path check passes right now.
        assert!(validate_config_path_under(&conf, uid(), &expected).is_ok());

        // Attacker swaps it for a symlink to a file they do not own.
        fs::remove_file(&conf).unwrap();
        std::os::unix::fs::symlink("/etc/shadow", &conf).unwrap();

        let err = read_config_checked_under(&conf, uid(), &expected)
            .expect_err("must refuse to read through the swapped symlink");
        // Whatever the reason, no byte of the target may appear in the error.
        assert!(!err.contains("root:"), "leaked target content: {err}");

        // A file owned by someone else, swapped in without a symlink, is
        // refused by the fd check too (simulated: root-owned /etc/hostname).
        fs::remove_file(&conf).unwrap();
        if fs::hard_link("/etc/hostname", &conf).is_ok() {
            let err = read_config_checked_under(&conf, uid(), &expected)
                .expect_err("must refuse a file owned by another user");
            assert!(!err.contains("root:"), "{err}");
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejection_messages_never_echo_file_content() {
        let secret = "root:$6$verysecrethash:19000:0:99999:7:::";
        let err = validate_config_body(secret).unwrap_err();
        assert!(!err.contains("verysecrethash"), "{err}");
        assert!(err.contains("line 1"), "{err}");

        let err = validate_config_body("AWS_SECRET_ACCESS_KEY = abc123\n").unwrap_err();
        assert!(!err.contains("abc123"), "{err}");
        assert!(!err.contains("aws_secret"), "{err}");
    }

    #[test]
    fn refuses_non_allowlisted_openfortivpn() {
        let root = sandbox("vpn");
        let fake = root.join("openfortivpn");
        write_mode(&fake, "#!/bin/sh\n", 0o755);
        assert!(allowed_openfortivpn(&fake).is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
