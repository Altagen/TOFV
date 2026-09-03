//! Parse openfortivpn stdout/stderr.

use crate::profile::validate_trusted_cert;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CertFinding {
    /// Gateway cert is unknown; user must pin the SHA-256.
    Unknown { sha256: String },
    /// Digest was offered and already in the whitelist (informational).
    AlreadyTrusted { sha256: String },
}

/// Extract a `trusted-cert` SHA-256 from openfortivpn output, if any.
///
/// Typical failure:
/// ```text
/// ERROR:  Gateway certificate validation failed, and the certificate digest is not in the local whitelist.
/// If you trust it, rerun with:
///     --trusted-cert 1f9b63379d75e9f3f4f133167be7a3a7ee2c81bdc8ed06f8b8b068986868a8c6
/// ```
pub fn parse_openfortivpn_output(text: &str) -> Option<CertFinding> {
    let mut last_hex: Option<String> = None;
    let mut unknown = false;
    let mut already = false;

    for raw in text.lines() {
        let line = raw.trim();
        let lower = line.to_ascii_lowercase();

        if lower.contains("certificate digest is not in the local whitelist")
            || lower.contains("certificate digest in not in the local whitelist")
            || lower.contains("gateway certificate validation failed")
        {
            unknown = true;
        }
        if lower.contains("certificate digest found in white list")
            || lower.contains("certificate digest found in whitelist")
        {
            already = true;
        }

        if let Some(hex) = extract_trusted_cert_flag(line) {
            last_hex = Some(hex);
        } else if let Some(hex) = extract_sha256_labeled(line) {
            last_hex = Some(hex);
        }
    }

    let sha256 = last_hex?;
    if unknown {
        Some(CertFinding::Unknown { sha256 })
    } else if already {
        Some(CertFinding::AlreadyTrusted { sha256 })
    } else {
        Some(CertFinding::Unknown { sha256 })
    }
}

fn extract_trusted_cert_flag(line: &str) -> Option<String> {
    let marker = "--trusted-cert";
    let idx = line.find(marker)?;
    let rest = line[idx + marker.len()..].trim();
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim();
    let token = rest.split_whitespace().next()?.trim();
    normalize_hex(token)
}

fn extract_sha256_labeled(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    for prefix in ["sha256 digest:", "sha256:", "digest:"] {
        if let Some(rest) = lower
            .strip_prefix(prefix)
            .or_else(|| lower.find(prefix).map(|i| &lower[i + prefix.len()..]))
        {
            if let Some(token) = rest.split_whitespace().next() {
                if let Some(hex) = normalize_hex(token) {
                    return Some(hex);
                }
            }
        }
    }
    None
}

fn normalize_hex(token: &str) -> Option<String> {
    let hex: String = token
        .bytes()
        .filter(|b| *b != b':')
        .map(|b| b.to_ascii_lowercase() as char)
        .collect();
    if validate_trusted_cert(&hex).is_ok() {
        Some(hex)
    } else {
        None
    }
}

/// TLS to the gateway is not the tunnel. Cert failures also print
/// "Connected to gateway" first.
pub fn looks_tunnel_up(text: &str) -> bool {
    text.lines().any(line_looks_tunnel_up)
}

pub fn line_looks_tunnel_up(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("tunnel is up")
        || l.contains("authenticated.")
        || (l.contains("interface ") && l.contains(" is up"))
        || l.contains("got addresses")
}

pub fn looks_auth_failed(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.to_ascii_lowercase();
        l.contains("could not authenticate")
            || l.contains("authentication failed")
            || l.contains("invalid password")
            || l.contains("permission denied")
    })
}

/// TLS pin rejected (rotation or first connect). May or may not include `--trusted-cert`.
pub fn looks_cert_failed(text: &str) -> bool {
    let l = text.to_ascii_lowercase();
    l.contains("gateway certificate validation failed")
        || l.contains("certificate digest is not in the local whitelist")
        || l.contains("certificate digest in not in the local whitelist")
        || l.contains("not in the local whitelist")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
INFO:   Connected to gateway.
ERROR:  Gateway certificate validation failed, and the certificate digest is not in the local whitelist.
If you trust it, rerun with:
    --trusted-cert 1f9b63379d75e9f3f4f133167be7a3a7ee2c81bdc8ed06f8b8b068986868a8c6
"#;

    #[test]
    fn parses_unknown_cert() {
        match parse_openfortivpn_output(SAMPLE) {
            Some(CertFinding::Unknown { sha256 }) => {
                assert_eq!(
                    sha256,
                    "1f9b63379d75e9f3f4f133167be7a3a7ee2c81bdc8ed06f8b8b068986868a8c6"
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parses_equals_form() {
        let text =
            "--trusted-cert=e46d4aff08ba6914e64daa85bc6112a422fa7ce16631bff0b592a28556f993db";
        match parse_openfortivpn_output(text) {
            Some(CertFinding::Unknown { sha256 }) => {
                assert_eq!(
                    sha256,
                    "e46d4aff08ba6914e64daa85bc6112a422fa7ce16631bff0b592a28556f993db"
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn ignores_unrelated_output() {
        assert!(parse_openfortivpn_output("INFO: Connected to gateway.\n").is_none());
    }

    #[test]
    fn gateway_tcp_is_not_tunnel_up() {
        assert!(!looks_tunnel_up("INFO: Connected to gateway.\n"));
        assert!(looks_tunnel_up("INFO: Authenticated.\n"));
        assert!(looks_tunnel_up("INFO: Interface ppp0 is UP.\n"));
    }

    #[test]
    fn cert_failed_even_without_hex() {
        assert!(looks_cert_failed(
            "ERROR: Gateway certificate validation failed, and the certificate digest is not in the local whitelist."
        ));
        assert!(!looks_cert_failed("INFO: Authenticated.\n"));
    }

    #[test]
    fn colon_separated_fingerprint() {
        let text = "sha256 digest: 1f:9b:63:37:9d:75:e9:f3:f4:f1:33:16:7b:e7:a3:a7:ee:2c:81:bd:c8:ed:06:f8:b8:b0:68:98:68:68:a8:c6";
        match parse_openfortivpn_output(text) {
            Some(CertFinding::Unknown { sha256 }) => {
                assert_eq!(
                    sha256,
                    "1f9b63379d75e9f3f4f133167be7a3a7ee2c81bdc8ed06f8b8b068986868a8c6"
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
