//! Privileged helper. The GUI never runs as root; this binary is the only
//! allowlisted program Polkit/sudo may invoke without a password.

use std::fs;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};

mod validate;
use validate::{
    allowed_openfortivpn, caller_uid, find_session, priv_dir, read_config_checked,
    validate_config_body,
};

#[derive(Parser)]
#[command(name = "tofv-helper", about = "Allowlisted openfortivpn start/stop")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Start {
        #[arg(long)]
        config: PathBuf,
    },
    Stop,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tofv-helper: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    if !is_root() {
        return Err("must run as root (pkexec/sudo)".into());
    }
    let cli = Cli::parse();
    let uid = caller_uid()?;
    match cli.cmd {
        Cmd::Start { config } => start(uid, &config),
        Cmd::Stop => stop(uid),
    }
}

fn start(uid: u32, config: &Path) -> Result<(), String> {
    // Validates the descriptor it reads from, so the caller cannot swap the
    // file for a symlink between the check and the read.
    let (_config, body) = read_config_checked(config, uid)?;
    validate_config_body(&body)?;

    if let Some(pid) = find_session(uid) {
        return Err(format!(
            "openfortivpn already running (pid {pid}) — stop it first"
        ));
    }

    let vpn = allowed_openfortivpn(Path::new("/usr/bin/openfortivpn"))
        .or_else(|_| allowed_openfortivpn(Path::new("/usr/local/bin/openfortivpn")))?;

    let pinentry_bin = pinentry_bin()?;
    let dir = lock_priv_dir(uid)?;
    let wrapper = dir.join("pinentry");
    write_root_pinentry(&wrapper, &pinentry_bin, uid)?;
    // Snapshot already validated in `body` — never re-read the user path (TOCTOU).
    let safe_conf = write_root_file(&dir.join("session.conf"), body.as_bytes(), 0o600)?;

    let pid_path = dir.join("vpn.pid");
    write_root_file(
        &pid_path,
        format!("{}\n", std::process::id()).as_bytes(),
        0o600,
    )?;

    let err = Command::new(&vpn)
        .arg("-c")
        .arg(&safe_conf)
        .arg("--pinentry")
        .arg(&wrapper)
        .arg("-v")
        .arg("--no-ftm-push")
        .env(
            "TOFV_PINENTRY_SOCKET",
            format!("/run/user/{uid}/tofv/pinentry.sock"),
        )
        .exec();
    Err(format!("exec {}: {err}", vpn.display()))
}

fn stop(uid: u32) -> Result<(), String> {
    let dir = priv_dir(uid);
    let pid = find_session(uid).ok_or_else(|| {
        let _ = fs::remove_file(dir.join("session.conf"));
        "no active TOFV session".to_string()
    })?;
    let _ = Command::new("/bin/kill")
        .args(["-s", "INT", &pid.to_string()])
        .status();
    for _ in 0..20 {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            wipe_session_files(&dir);
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _ = Command::new("/bin/kill")
        .args(["-s", "TERM", &pid.to_string()])
        .status();
    thread::sleep(Duration::from_millis(300));
    let _ = Command::new("/bin/kill")
        .args(["-s", "KILL", &pid.to_string()])
        .status();
    wipe_session_files(&dir);
    Ok(())
}

fn wipe_session_files(dir: &Path) {
    let _ = fs::remove_file(dir.join("vpn.pid"));
    let _ = fs::remove_file(dir.join("session.conf"));
}

fn lock_priv_dir(uid: u32) -> Result<PathBuf, String> {
    let dir = priv_dir(uid);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut perms = fs::metadata(&dir).map_err(|e| e.to_string())?.permissions();
    perms.set_mode(0o700);
    fs::set_permissions(&dir, perms).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn write_root_file(path: &Path, body: &[u8], mode: u32) -> Result<PathBuf, String> {
    let _ = fs::remove_file(path);
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(path)
            .map_err(|e| e.to_string())?;
        use std::io::Write;
        f.write_all(body).map_err(|e| e.to_string())?;
        f.sync_all().ok();
    }
    Ok(path.to_path_buf())
}

fn pinentry_bin() -> Result<PathBuf, String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cand = dir.join("pinentry-tofv");
            if cand.is_file() {
                if let Ok(p) = require_root_exec(&cand) {
                    return Ok(p);
                }
            }
        }
    }
    for p in [
        "/usr/local/libexec/pinentry-tofv",
        "/usr/libexec/pinentry-tofv",
        "/usr/lib/tofv/pinentry-tofv",
    ] {
        let p = PathBuf::from(p);
        if p.is_file() {
            return require_root_exec(&p);
        }
    }
    Err("pinentry-tofv not installed next to tofv-helper".into())
}

fn require_root_exec(path: &Path) -> Result<PathBuf, String> {
    let canon = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let meta = fs::metadata(&canon).map_err(|e| e.to_string())?;
    use std::os::unix::fs::MetadataExt;
    if meta.uid() != 0 {
        return Err(format!("{} is not root-owned", canon.display()));
    }
    if meta.mode() & 0o022 != 0 {
        return Err(format!("{} is writable by group/other", canon.display()));
    }
    Ok(canon)
}

fn write_root_pinentry(path: &Path, bin: &Path, uid: u32) -> Result<(), String> {
    let sock = format!("/run/user/{uid}/tofv/pinentry.sock");
    let body = format!(
        "#!/bin/sh\nexport TOFV_PINENTRY_SOCKET='{}'\nexec '{}' \"$@\"\n",
        sock.replace('\'', r"'\''"),
        bin.display().to_string().replace('\'', r"'\''"),
    );
    let _ = fs::remove_file(path);
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(path)
            .map_err(|e| e.to_string())?;
        use std::io::Write;
        f.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn is_root() -> bool {
    fs::read_to_string("/proc/self/status").ok().and_then(|s| {
        s.lines()
            .find(|l| l.starts_with("Uid:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u32>().ok())
    }) == Some(0)
}
