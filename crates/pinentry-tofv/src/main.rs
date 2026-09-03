//! Assuan pinentry spoken by openfortivpn `--pinentry=`.
//!
//! The password is never accepted on our argv. We fetch it from:
//! 1. the user-session Unix socket (`/run/user/<uid>/tofv/pinentry.sock`)
//! 2. otherwise `secret-tool` (only works when we still run as the user)

use std::io::{self, BufRead, Write};

use tofv_core::{
    discover_socket_path, fetch_password, percent_encode, PasswordStore, SecretString,
    SecretToolStore, DEFAULT_PROFILE_ID,
};

fn main() {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if writeln!(out, "OK Pleased to meet you").is_err() {
        return;
    }
    let _ = out.flush();

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let cmd = line.trim();
        if cmd.is_empty() || cmd.starts_with('#') {
            continue;
        }
        let verb = cmd.split_whitespace().next().unwrap_or("");
        let ok = match verb.to_ascii_uppercase().as_str() {
            "GETPIN" => reply_pin(&mut out),
            "BYE" => {
                let _ = writeln!(out, "OK closing connection");
                let _ = out.flush();
                break;
            }
            _ => writeln!(out, "OK").is_ok(),
        };
        if !ok {
            break;
        }
        let _ = out.flush();
    }
}

fn reply_pin(out: &mut impl Write) -> bool {
    match get_pin() {
        Ok(pin) => {
            // Assuan data line, percent-encoded. Never log `pin`.
            writeln!(out, "D {}", percent_encode(pin.expose())).is_ok()
                && writeln!(out, "OK").is_ok()
        }
        Err(e) => {
            eprintln!("pinentry-tofv: {e}");
            writeln!(out, "ERR 83886179 Operation cancelled").is_ok()
        }
    }
}

fn get_pin() -> tofv_core::Result<SecretString> {
    if let Some(path) = discover_socket_path() {
        if path.exists() {
            return fetch_password(&path);
        }
    }
    let profile = std::env::var("TOFV_PROFILE").unwrap_or_else(|_| DEFAULT_PROFILE_ID.to_string());
    SecretToolStore
        .get(&profile)?
        .ok_or(tofv_core::Error::PasswordMissing(profile))
}
