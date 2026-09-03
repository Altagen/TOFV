use crate::profile::Profile;

/// Build the ephemeral openfortivpn config body.
/// Password is never written here; pinentry supplies it.
pub fn render(profile: &Profile, otp: Option<&str>) -> String {
    let mut lines = Vec::new();
    lines.push(format!("host = {}", profile.host.trim()));
    lines.push(format!("port = {}", profile.port));
    lines.push(format!("username = {}", profile.username.trim()));
    let realm = profile.realm.trim();
    if !realm.is_empty() {
        lines.push(format!("realm = {realm}"));
    }
    if let Some(cert) = &profile.trusted_cert {
        lines.push(format!("trusted-cert = {cert}"));
    }
    lines.push("no-ftm-push = 1".into());
    if let Some(otp) = otp {
        lines.push(format!("otp = {otp}"));
    }
    lines.push(String::new());
    lines.join("\n")
}

pub fn redact_config(body: &str) -> String {
    crate::redact::redact_text(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;

    #[test]
    fn omits_empty_realm_and_password() {
        let p = Profile {
            host: "vpn.example.com".into(),
            username: "alice".into(),
            ..Profile::default()
        };
        let body = render(&p, Some("123456"));
        assert!(body.contains("host = vpn.example.com"));
        assert!(body.contains("otp = 123456"));
        assert!(!body.contains("realm"));
        assert!(!body.contains("password"));
        assert!(body.contains("no-ftm-push = 1"));

        let redacted = redact_config(&body);
        assert!(!redacted.contains("123456"));
        assert!(redacted.contains("otp = ******"));
    }
}
