//! Local socket used by `pinentry-tofv` (possibly running as root) to fetch
//! the password from the user-session daemon. The password never appears in
//! argv or in the openfortivpn config file.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::secret::SecretString;

const GET_CMD: &str = "GET";

/// Percent-encode every byte that is not printable ASCII.
///
/// This is a *byte* codec: pushing a raw byte >= 0x80 as a `char` would
/// reinterpret it as a Latin-1 code point and re-encode it as two UTF-8
/// bytes, so any password with an accent came back corrupted and silently
/// failed authentication.
pub fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'%' => out.push_str("%25"),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn percent_decode(input: &str) -> Result<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
                .map_err(|_| Error::PinentrySocket("invalid percent-encoding".into()))?;
            out.push(h);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| Error::PinentrySocket("non-utf8 password".into()))
}

/// openfortivpn asks once per connect attempt; a couple of retries is the
/// most a legitimate run needs.
const MAX_SERVES: usize = 8;

fn own_uid() -> u32 {
    // SAFETY: getuid() is always successful and has no preconditions.
    unsafe { libc::getuid() }
}

/// Peer uid from `SO_PEERCRED`. `std`'s `UCred` fields are still unstable.
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    use std::os::unix::io::AsRawFd;
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred`/`len` are correctly sized for SO_PEERCRED on Linux and
    // the fd is owned by `stream` for the duration of the call.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        )
    };
    if rc == 0 {
        Some(cred.uid)
    } else {
        None
    }
}

pub struct PinentryServer {
    listener: UnixListener,
    path: PathBuf,
}

impl PinentryServer {
    pub fn bind(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path)
            .map_err(|e| Error::PinentrySocket(format!("bind {}: {e}", path.display())))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        listener
            .set_nonblocking(false)
            .map_err(|e| Error::PinentrySocket(e.to_string()))?;
        Ok(Self {
            listener,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Serve the password to `pinentry-tofv` until `shutdown` is dropped.
    ///
    /// The socket is 0600 inside a 0700 runtime dir, so only this user and
    /// root can reach it, and it only exists for the duration of one connect
    /// attempt. Beyond that we check the peer's uid and cap how many times the
    /// password may be handed out, so a stray same-user process cannot sit on
    /// the socket and drain it.
    pub fn spawn(self, password: SecretString) -> PinentryShutdown {
        let path = self.path.clone();
        let watch = path.clone();
        let handle = thread::spawn(move || {
            self.listener.set_nonblocking(true).ok();
            let us = own_uid();
            let mut served = 0usize;
            loop {
                match self.listener.accept() {
                    Ok((stream, _)) => {
                        match peer_uid(&stream) {
                            Some(peer) if peer == us || peer == 0 => {}
                            other => {
                                eprintln!("tofv pinentry socket: refusing peer uid {other:?}");
                                let _ = writeln!(&stream, "ERR forbidden");
                                continue;
                            }
                        }
                        if served >= MAX_SERVES {
                            eprintln!("tofv pinentry socket: request cap reached");
                            let _ = writeln!(&stream, "ERR exhausted");
                            continue;
                        }
                        served += 1;
                        if let Err(e) = handle_client(stream, password.expose()) {
                            eprintln!("tofv pinentry socket: {e}");
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => break,
                }
                if !watch.exists() {
                    break;
                }
            }
        });
        PinentryShutdown {
            path,
            join: Some(handle),
        }
    }
}

pub struct PinentryShutdown {
    path: PathBuf,
    join: Option<thread::JoinHandle<()>>,
}

impl Drop for PinentryShutdown {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        if let Some(h) = self.join.take() {
            let _ = h.join();
        }
    }
}

fn handle_client(stream: UnixStream, password: &str) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|e| Error::PinentrySocket(e.to_string()))?,
    );
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| Error::PinentrySocket(e.to_string()))?;
    let mut stream = stream;
    if line.trim() == GET_CMD {
        writeln!(stream, "D {}", percent_encode(password))
            .map_err(|e| Error::PinentrySocket(e.to_string()))?;
    } else {
        writeln!(stream, "ERR unknown-command")
            .map_err(|e| Error::PinentrySocket(e.to_string()))?;
    }
    Ok(())
}

pub fn fetch_password(path: &Path) -> Result<SecretString> {
    let mut stream = UnixStream::connect(path)
        .map_err(|e| Error::PinentrySocket(format!("connect {}: {e}", path.display())))?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(b"GET\n")
        .map_err(|e| Error::PinentrySocket(e.to_string()))?;
    stream
        .flush()
        .map_err(|e| Error::PinentrySocket(e.to_string()))?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| Error::PinentrySocket(e.to_string()))?;
    let line = line.trim_end();
    if let Some(rest) = line.strip_prefix("D ") {
        Ok(SecretString::new(percent_decode(rest)?))
    } else {
        Err(Error::PinentrySocket(format!(
            "unexpected reply: {}",
            crate::redact::redact_line(line)
        )))
    }
}

/// Where pinentry-tofv should look. Works even when the process is root
/// (`pkexec` sets `PKEXEC_UID`, `sudo` sets `SUDO_UID`).
pub fn discover_socket_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TOFV_PINENTRY_SOCKET") {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    for var in ["PKEXEC_UID", "SUDO_UID"] {
        if let Ok(uid) = std::env::var(var) {
            if !uid.is_empty() {
                return Some(PathBuf::from(format!("/run/user/{uid}/tofv/pinentry.sock")));
            }
        }
    }
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("tofv/pinentry.sock"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_roundtrip() {
        let s = "p%ass\nword";
        assert_eq!(percent_decode(&percent_encode(s)).unwrap(), s);
    }

    /// Regression: the encoder pushed raw bytes as `char`, so any byte >= 0x80
    /// was reinterpreted as Latin-1 and re-encoded. Accented passwords came
    /// back mangled and authentication failed for no visible reason.
    #[test]
    fn non_ascii_password_survives_the_round_trip() {
        for pw in [
            "café-du-commerce",
            "Motdepasse-é@2024",
            "пароль",
            "🔐emoji",
            "with space and %25 literal",
            "tab\there",
        ] {
            let encoded = percent_encode(pw);
            assert!(
                encoded.is_ascii(),
                "wire format must stay ASCII: {encoded:?}"
            );
            assert!(!encoded.contains('\n') && !encoded.contains('\r'));
            assert_eq!(
                percent_decode(&encoded).unwrap(),
                pw,
                "round trip for {pw:?}"
            );
        }
    }

    #[test]
    fn socket_get_password() {
        let dir = std::env::temp_dir().join(format!("tofv-sock-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("pinentry.sock");
        let server = PinentryServer::bind(&path).unwrap();
        let _guard = server.spawn(SecretString::new("s3cret"));
        thread::sleep(Duration::from_millis(80));
        let got = fetch_password(&path).unwrap();
        assert_eq!(got.expose(), "s3cret");
        drop(_guard);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
