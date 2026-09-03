//! Strip secrets from helper output and copied commands.

const REDACTED: &str = "******";

const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "passwd",
    "otp",
    "cookie",
    "svpncookie",
    "pem-passphrase",
    "pin",
];

pub fn redact_line(line: &str) -> String {
    let mut out = redact_flag_values(line);
    out = redact_config_assignments(&out);
    out = redact_cookie_token(&out);
    out
}

pub fn redact_text(text: &str) -> String {
    text.lines().map(redact_line).collect::<Vec<_>>().join("\n")
}

fn redact_flag_values(line: &str) -> String {
    let mut result = line.to_string();
    for key in SENSITIVE_KEYS {
        for flag in [format!("--{key}="), format!("--{key} "), format!("-{key}=")] {
            result = replace_after_marker(&result, &flag);
        }
    }
    // openfortivpn short flags: -p password, -o otp
    result = replace_short_flag(&result, "-p");
    result = replace_short_flag(&result, "-o");
    result
}

fn replace_after_marker(line: &str, marker: &str) -> String {
    let lower = line.to_ascii_lowercase();
    let marker_l = marker.to_ascii_lowercase();
    let Some(idx) = lower.find(&marker_l) else {
        return line.to_string();
    };
    let start = idx + marker.len();
    let rest = &line[start..];
    let (value, tail) = split_value(rest);
    if value.is_empty() {
        return line.to_string();
    }
    format!("{}{REDACTED}{tail}", &line[..start])
}

fn replace_short_flag(line: &str, flag: &str) -> String {
    // Match `-p value` but not `-pppd-log`.
    let bytes = line.as_bytes();
    let flag_b = flag.as_bytes();
    let mut i = 0;
    let mut out = String::new();
    while i < bytes.len() {
        if i + flag_b.len() <= bytes.len()
            && bytes[i..i + flag_b.len()] == *flag_b
            && (i == 0 || bytes[i - 1].is_ascii_whitespace())
            && i + flag_b.len() < bytes.len()
            && bytes[i + flag_b.len()].is_ascii_whitespace()
        {
            out.push_str(flag);
            let mut j = i + flag_b.len();
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                out.push(bytes[j] as char);
                j += 1;
            }
            let rest = &line[j..];
            let (value, tail) = split_value(rest);
            if !value.is_empty() {
                out.push_str(REDACTED);
                out.push_str(tail);
                return out;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn split_value(rest: &str) -> (&str, &str) {
    if rest.is_empty() {
        return ("", "");
    }
    if let Some(inner) = rest.strip_prefix('"') {
        if let Some(end) = inner.find('"') {
            return (&rest[..=end + 1], &rest[end + 2..]);
        }
        return (rest, "");
    }
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    (&rest[..end], &rest[end..])
}

fn redact_config_assignments(line: &str) -> String {
    let trimmed = line.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    for key in SENSITIVE_KEYS {
        let prefix = format!("{key} ");
        let prefix_eq = format!("{key}=");
        if lower.starts_with(&prefix) || lower.starts_with(&prefix_eq) {
            if let Some(eq) = trimmed.find('=') {
                let head = &line[..line.len() - trimmed.len() + eq + 1];
                let rest = trimmed[eq + 1..].trim_start();
                let (value, tail) = split_value(rest);
                if !value.is_empty() {
                    return format!("{head} {REDACTED}{tail}");
                }
            }
        }
    }
    line.to_string()
}

fn redact_cookie_token(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    if let Some(idx) = lower.find("svpncookie=") {
        let start = idx + "svpncookie=".len();
        let rest = &line[start..];
        let end = rest
            .find(|c: char| c == ';' || c.is_whitespace())
            .unwrap_or(rest.len());
        return format!("{}{REDACTED}{}", &line[..start], &rest[end..]);
    }
    line.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_cli_password_and_otp() {
        let line = "openfortivpn -u alice -p hunter2 --otp=123456 host";
        let out = redact_line(line);
        assert!(!out.contains("hunter2"), "{out}");
        assert!(!out.contains("123456"), "{out}");
        assert!(out.contains(REDACTED));
        assert!(out.contains("-u alice"));
    }

    #[test]
    fn redacts_config_otp_line() {
        let out = redact_line("otp = 123456");
        assert_eq!(out, "otp = ******");
        let out = redact_line("password=super-secret");
        assert!(out.contains(REDACTED));
        assert!(!out.contains("super-secret"));
    }

    #[test]
    fn redacts_cookie() {
        let out = redact_line("Set-Cookie: SVPNCOOKIE=abcDEF123; Path=/");
        assert!(!out.to_ascii_lowercase().contains("abcdef123"));
        assert!(out.contains(REDACTED));
    }

    #[test]
    fn does_not_eat_pppd_flags() {
        let line = "openfortivpn --pppd-log=/tmp/x --no-ftm-push";
        assert_eq!(redact_line(line), line);
    }
}
