//! Spawn and supervise `openfortivpn`. The GUI/CLI stay unprivileged.

use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use zeroize::Zeroize;

use crate::command::PlannedInvocation;
use crate::error::{Error, Result};
use crate::parse::{
    looks_auth_failed, looks_cert_failed, looks_tunnel_up, parse_openfortivpn_output, CertFinding,
};
use crate::paths::{install_pinentry_wrapper, resolve_pinentry_bin, AppPaths, Elevate};
use crate::pinentry_socket::{PinentryServer, PinentryShutdown};
use crate::redact::redact_line;
use crate::secret::which;
use crate::secret::SecretString;
use crate::session::{plan, ConnectRequest, SessionFiles};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectOutcome {
    NeedCert {
        sha256: String,
    },
    AuthFailed,
    /// Tunnel came up; the process later exited (disconnect or crash).
    ExitedAfterUp {
        code: Option<i32>,
    },
    /// Cert pin refused and openfortivpn did not print a new SHA (retry without pin).
    CertRejected,
    Failed {
        code: Option<i32>,
    },
    Interrupted,
}

/// openfortivpn prints the SHA-256 a few lines *after* the validation error,
/// so cert detection needs a little history — but a bounded one.
const CERT_WINDOW: usize = 64;

struct ParseState {
    /// Recent lines, capped at `CERT_WINDOW`. Only the connect phase fills it.
    window: VecDeque<String>,
    need_cert: Option<String>,
    up: bool,
    auth_failed: bool,
    cert_failed: bool,
}

impl ParseState {
    fn ingest(&mut self, line: &str) {
        // This runs on the thread draining the child's stdout/stderr pipe.
        // Anything slow here fills the 64 KiB pipe, and openfortivpn then
        // blocks in do_log() -> fflush() while holding its global log mutex,
        // which freezes its four packet-forwarding threads. Cost per line
        // must stay constant, never a function of the session's log volume.
        if self.up {
            // `-v` logs one line per packet once the tunnel is up, and every
            // check below only ever fires during the connect phase.
            return;
        }
        if looks_tunnel_up(line) {
            self.up = true;
            self.window.clear();
            self.window.shrink_to_fit();
            return;
        }
        if looks_auth_failed(line) {
            self.auth_failed = true;
        }
        if looks_cert_failed(line) {
            self.cert_failed = true;
        }
        if self.window.len() == CERT_WINDOW {
            self.window.pop_front();
        }
        self.window.push_back(line.to_string());
        if self.need_cert.is_none() {
            let recent = self
                .window
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(CertFinding::Unknown { sha256 }) = parse_openfortivpn_output(&recent) {
                self.need_cert = Some(sha256);
            }
        }
    }
}

pub struct RunningConnect {
    child: Child,
    pgid: u32,
    files: Option<SessionFiles>,
    pinentry: Option<PinentryShutdown>,
    wrapper_path: PathBuf,
    pid_path: PathBuf,
    readers: Vec<JoinHandle<()>>,
    state: Arc<Mutex<ParseState>>,
    finished: bool,
}

pub fn spawn_connect(
    req: ConnectRequest<'_>,
    password: SecretString,
) -> Result<(RunningConnect, Receiver<String>)> {
    if password.is_empty() {
        return Err(Error::PasswordMissing(req.profile.id.clone()));
    }
    match req.elevate {
        Elevate::Pkexec if which("pkexec").is_none() => {
            return Err(Error::Path("pkexec not found".into()));
        }
        Elevate::Sudo if which("sudo").is_none() => {
            return Err(Error::Path("sudo not found".into()));
        }
        _ => {}
    }

    let pinentry_bin = resolve_pinentry_bin(req.app_config)?;
    let mut planned = plan(req)?;
    let socket = req.paths.pinentry_socket_path();
    let wrapper = req.paths.pinentry_wrapper_path();
    let pid_path = req.paths.session_pid_path();

    install_pinentry_wrapper(&wrapper, &pinentry_bin, &socket)?;
    let files = SessionFiles::create(&planned.config_path, &planned.config_body)?;
    planned.config_body.zeroize();

    let server = PinentryServer::bind(&socket)?;
    let pinentry = server.spawn(password);

    let (tx, rx) = mpsc::channel::<String>();
    let state = Arc::new(Mutex::new(ParseState {
        window: VecDeque::new(),
        need_cert: None,
        up: false,
        auth_failed: false,
        cert_failed: false,
    }));

    let mut child = spawn_child(&planned)?;
    let pgid = child.id();
    fs::write(&pid_path, pgid.to_string()).ok();

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Path("failed to capture stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::Path("failed to capture stderr".into()))?;

    let readers = vec![
        spawn_reader(stdout, tx.clone(), state.clone()),
        spawn_reader(stderr, tx, state.clone()),
    ];

    Ok((
        RunningConnect {
            child,
            pgid,
            files: Some(files),
            pinentry: Some(pinentry),
            wrapper_path: wrapper,
            pid_path,
            readers,
            state,
            finished: false,
        },
        rx,
    ))
}

fn spawn_child(planned: &PlannedInvocation) -> Result<Child> {
    let mut cmd = Command::new(&planned.argv[0]);
    cmd.args(&planned.argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.process_group(0);
    cmd.spawn().map_err(|source| Error::IoPath {
        path: PathBuf::from(&planned.argv[0]),
        source,
    })
}

fn spawn_reader<R: std::io::Read + Send + 'static>(
    reader: R,
    tx: mpsc::Sender<String>,
    state: Arc<Mutex<ParseState>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let buf = BufReader::new(reader);
        for line in buf.lines() {
            let Ok(line) = line else { break };
            if let Ok(mut st) = state.lock() {
                st.ingest(&line);
            }
            let _ = tx.send(redact_line(&line));
        }
    })
}

#[derive(Debug, Clone, Default)]
pub struct SessionSnapshot {
    pub up: bool,
    pub need_cert: Option<String>,
    pub auth_failed: bool,
}

impl RunningConnect {
    pub fn pid(&self) -> u32 {
        self.pgid
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        match self.state.lock() {
            Ok(st) => SessionSnapshot {
                up: st.up,
                need_cert: st.need_cert.clone(),
                auth_failed: st.auth_failed,
            },
            Err(_) => SessionSnapshot::default(),
        }
    }

    pub fn terminate(&mut self) -> Result<()> {
        // openfortivpn is usually root (pkexec). A user-level kill gets EPERM
        // and wait() would freeze the UI forever — that is the "Couper" crash.
        stop_vpn_process(self.pgid);
        for _ in 0..30 {
            if let Ok(Some(_)) = self.child.try_wait() {
                self.finished = true;
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        if let Ok(Some(_)) = self.child.try_wait() {
            self.finished = true;
        }
        Ok(())
    }

    pub fn try_wait(&mut self) -> Result<Option<ConnectOutcome>> {
        match self.child.try_wait()? {
            None => Ok(None),
            Some(status) => {
                self.finished = true;
                for h in self.readers.drain(..) {
                    let _ = h.join();
                }
                Ok(Some(self.classify(status)))
            }
        }
    }

    pub fn wait(mut self) -> Result<ConnectOutcome> {
        let status = self.child.wait()?;
        self.finished = true;
        for h in self.readers.drain(..) {
            let _ = h.join();
        }
        Ok(self.classify(status))
    }

    fn classify(&self, status: ExitStatus) -> ConnectOutcome {
        let state = self.state.lock().ok();
        if let Some(st) = state.as_ref() {
            if let Some(sha256) = st.need_cert.clone() {
                return ConnectOutcome::NeedCert { sha256 };
            }
            if st.auth_failed {
                return ConnectOutcome::AuthFailed;
            }
            if st.cert_failed {
                return ConnectOutcome::CertRejected;
            }
            if st.up {
                return ConnectOutcome::ExitedAfterUp {
                    code: status.code(),
                };
            }
        }
        if status.code() == Some(130) || status.code() == Some(143) {
            return ConnectOutcome::Interrupted;
        }
        ConnectOutcome::Failed {
            code: status.code(),
        }
    }
}

impl Drop for RunningConnect {
    fn drop(&mut self) {
        // Never wait() here: if the child is still root we cannot reap it
        // from this process, and a blocking wait freezes the GUI.
        if !self.finished {
            let _ = self.child.try_wait();
            self.finished = true;
        }
        let _ = fs::remove_file(&self.pid_path);
        let _ = fs::remove_file(&self.wrapper_path);
        self.pinentry.take();
        self.files.take();
    }
}

fn helper_stop() -> bool {
    let Some(helper) = crate::paths::resolve_helper() else {
        return false;
    };
    Command::new("pkexec")
        .arg(&helper)
        .arg("stop")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn stop_vpn_process(pid: u32) {
    if helper_stop() {
        return;
    }
    // User-level only. A root tunnel requires tofv-helper (never pkexec /bin/sh).
    let _ = Command::new("/bin/kill")
        .args(["-s", "INT", &pid.to_string()])
        .status();
}

pub fn disconnect(paths: &AppPaths) -> Result<bool> {
    let helper_ok = helper_stop();
    let pid_path = paths.session_pid_path();
    let leftover = fs::read_to_string(&pid_path)
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok());
    if helper_ok {
        let _ = fs::remove_file(&pid_path);
        return Ok(true);
    }
    if let Some(pid) = leftover {
        stop_vpn_process(pid);
        let _ = fs::remove_file(&pid_path);
        return Ok(true);
    }
    let _ = fs::remove_file(&pid_path);
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{AppConfig, AppPaths};
    use crate::profile::Profile;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn setup(tag: &str) -> (AppPaths, PathBuf) {
        let root = std::env::temp_dir().join(format!("tofv-run-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let paths = AppPaths::isolated(&root);
        paths.ensure().unwrap();
        (paths, root)
    }

    fn profile() -> Profile {
        Profile {
            host: "vpn.example.com".into(),
            username: "alice".into(),
            ..Profile::default()
        }
    }

    fn write_exec(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
        let mut p = fs::metadata(path).unwrap().permissions();
        p.set_mode(0o755);
        fs::set_permissions(path, p).unwrap();
    }

    fn stub_pinentry(dir: &Path) -> PathBuf {
        let path = dir.join("pinentry-tofv");
        write_exec(
            &path,
            r#"#!/bin/sh
echo "OK Pleased to meet you"
while IFS= read -r line; do
  case "$line" in
    GETPIN)
      echo "D stub"
      echo "OK"
      ;;
    BYE)
      echo "OK closing connection"
      exit 0
      ;;
    *)
      echo "OK"
      ;;
  esac
done
"#,
        );
        path
    }

    fn fake_vpn(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("openfortivpn");
        write_exec(&path, body);
        path
    }

    fn cfg(vpn: &Path, pin: &Path) -> AppConfig {
        AppConfig {
            openfortivpn: Some(vpn.to_path_buf()),
            pinentry: Some(pin.to_path_buf()),
            elevate: Elevate::None,
        }
    }

    #[test]
    fn detects_unknown_cert_and_does_not_leak_otp_in_argv() {
        let (paths, root) = setup("cert");
        let argv_out = root.join("argv.txt");
        let vpn = fake_vpn(
            &root,
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$@" > '{argv}'
echo "INFO: Connected to gateway."
echo "ERROR: Gateway certificate validation failed, and the certificate digest is not in the local whitelist."
echo "If you trust it, rerun with:"
echo "    --trusted-cert 1f9b63379d75e9f3f4f133167be7a3a7ee2c81bdc8ed06f8b8b068986868a8c6"
exit 1
"#,
                argv = argv_out.display()
            ),
        );
        let pin = stub_pinentry(&root);
        let profile = profile();
        let app_config = cfg(&vpn, &pin);
        let (running, _logs) = spawn_connect(
            ConnectRequest {
                profile: &profile,
                otp: "123456",
                paths: &paths,
                app_config: &app_config,
                elevate: Elevate::None,
            },
            SecretString::new("pw"),
        )
        .unwrap();
        let outcome = running.wait().unwrap();
        match outcome {
            ConnectOutcome::NeedCert { sha256 } => {
                assert_eq!(
                    sha256,
                    "1f9b63379d75e9f3f4f133167be7a3a7ee2c81bdc8ed06f8b8b068986868a8c6"
                );
            }
            other => panic!("unexpected {other:?}"),
        }
        let argv = fs::read_to_string(&argv_out).unwrap();
        assert!(!argv.contains("123456"), "{argv}");
        assert!(!argv.contains("pw"), "{argv}");
        assert!(!argv.contains("--otp"), "{argv}");
        assert!(!paths.session_config_path("default").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detects_tunnel_up() {
        let (paths, root) = setup("up");
        let vpn = fake_vpn(
            &root,
            r#"#!/bin/sh
echo "INFO: Authenticated."
echo "INFO: Interface ppp0 is UP."
exit 0
"#,
        );
        let pin = stub_pinentry(&root);
        let profile = profile();
        let app_config = cfg(&vpn, &pin);
        let (running, logs) = spawn_connect(
            ConnectRequest {
                profile: &profile,
                otp: "654321",
                paths: &paths,
                app_config: &app_config,
                elevate: Elevate::None,
            },
            SecretString::new("pw"),
        )
        .unwrap();
        let outcome = running.wait().unwrap();
        assert_eq!(outcome, ConnectOutcome::ExitedAfterUp { code: Some(0) });
        let collected: Vec<_> = logs.try_iter().collect();
        assert!(collected.iter().any(|l| l.contains("Authenticated")));
        assert!(!collected.iter().any(|l| l.contains("654321")));
        let _ = fs::remove_dir_all(&root);
    }

    /// Regression: `ingest` used to re-scan the whole accumulated log on every
    /// line, so the reader thread went quadratic. It stopped draining the pipe,
    /// openfortivpn blocked in do_log()->fflush() holding its global log mutex,
    /// and the tunnel stalled — the VPN froze a bit more every minute.
    /// With `-v` the gateway logs one line per packet, so this must stay flat.
    #[test]
    fn reader_keeps_up_with_per_packet_debug_flood() {
        const LINES: usize = 20_000;
        let (paths, root) = setup("flood");
        let vpn = fake_vpn(
            &root,
            &format!(
                r#"#!/bin/sh
echo "INFO:   Authenticated."
i=0
while [ $i -lt {LINES} ]; do
  echo "DEBUG:  gateway ---> pppd (1454 bytes)"
  i=$((i+1))
done
exit 0
"#
            ),
        );
        let pin = stub_pinentry(&root);
        let profile = profile();
        let app_config = cfg(&vpn, &pin);
        let started = std::time::Instant::now();
        let (running, logs) = spawn_connect(
            ConnectRequest {
                profile: &profile,
                otp: "123456",
                paths: &paths,
                app_config: &app_config,
                elevate: Elevate::None,
            },
            SecretString::new("pw"),
        )
        .unwrap();
        let outcome = running.wait().unwrap();
        let elapsed = started.elapsed();
        let _ = fs::remove_dir_all(&root);

        assert_eq!(outcome, ConnectOutcome::ExitedAfterUp { code: Some(0) });
        assert_eq!(
            logs.try_iter().count(),
            LINES + 1,
            "lines must still reach the UI"
        );
        // The quadratic version needed ~30s for this; O(1) needs well under one.
        assert!(
            elapsed < Duration::from_secs(10),
            "reader is not keeping up: {LINES} lines took {elapsed:?}"
        );
    }

    #[test]
    fn redacts_otp_if_child_echoes_config() {
        let (paths, root) = setup("echo");
        let vpn = fake_vpn(
            &root,
            r#"#!/bin/sh
# find -c <file> and cat it to stderr (simulates a leaky helper)
c=
while [ $# -gt 0 ]; do
  if [ "$1" = "-c" ]; then
    shift
    c=$1
  fi
  shift
done
echo "otp dump:" >&2
cat "$c" >&2
exit 1
"#,
        );
        let pin = stub_pinentry(&root);
        let profile = profile();
        let app_config = cfg(&vpn, &pin);
        let (running, logs) = spawn_connect(
            ConnectRequest {
                profile: &profile,
                otp: "111222",
                paths: &paths,
                app_config: &app_config,
                elevate: Elevate::None,
            },
            SecretString::new("pw"),
        )
        .unwrap();
        let _ = running.wait();
        let collected: Vec<_> = logs.try_iter().collect();
        let blob = collected.join("\n");
        assert!(!blob.contains("111222"), "{blob}");
        assert!(
            blob.contains("otp = ******") || blob.contains("******"),
            "{blob}"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
