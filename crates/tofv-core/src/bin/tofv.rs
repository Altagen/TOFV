use std::io::{self, IsTerminal, Read, Write};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};

use tofv_core::{
    disconnect, parse_openfortivpn_output, plan_session, spawn_connect, validate_totp,
    validate_trusted_cert, AppPaths, CertFinding, ConnectOutcome, ConnectRequest, Elevate,
    PasswordStore, SecretString, SecretToolStore, DEFAULT_PROFILE_ID,
};

#[derive(Parser)]
#[command(
    name = "tofv",
    about = "Tray OpenFortiVPN — plan and inspect an openfortivpn session"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show, create or update the single profile
    Profile {
        #[command(subcommand)]
        action: ProfileCmd,
    },
    /// Pin a gateway certificate SHA-256 on the profile
    Trust {
        /// SHA-256 hex (64 chars), or `-` to parse openfortivpn output on stdin
        cert: String,
    },
    /// Connect: writes a 0600 config, serves the password over pinentry, runs openfortivpn
    Connect {
        /// 6-digit FortiToken TOTP. If omitted, prompted on the tty.
        #[arg(long)]
        otp: Option<String>,
        /// Print the redacted command and config, do not execute
        #[arg(long)]
        dry_run: bool,
        #[arg(long, value_enum, default_value = "pkexec")]
        elevate: ElevateArg,
    },
    /// Send SIGTERM to a running session started by `tofv connect`
    Disconnect,
    /// Extract --trusted-cert from openfortivpn output on stdin
    ParseCert,
    /// Check openfortivpn, secret-tool, profile, runtime dirs
    Doctor,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum ElevateArg {
    Pkexec,
    Sudo,
    None,
}

impl From<ElevateArg> for Elevate {
    fn from(v: ElevateArg) -> Self {
        match v {
            ElevateArg::Pkexec => Elevate::Pkexec,
            ElevateArg::Sudo => Elevate::Sudo,
            ElevateArg::None => Elevate::None,
        }
    }
}

#[derive(Subcommand)]
enum ProfileCmd {
    /// Print the profile as TOML (never includes the password)
    Show,
    /// Create or update fields. Omitted flags keep their previous value.
    Set {
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        realm: Option<String>,
        #[arg(long)]
        trusted_cert: Option<String>,
    },
    /// Read the VPN password from stdin and store it in the desktop keyring
    Password,
    /// Remove the stored password
    ClearPassword,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tofv: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> tofv_core::Result<()> {
    let cli = Cli::parse();
    let paths = AppPaths::discover()?;

    match cli.command {
        Command::Profile { action } => match action {
            ProfileCmd::Show => {
                let profile = paths.load_default_profile()?;
                print!("{}", toml::to_string_pretty(&profile)?);
            }
            ProfileCmd::Set {
                host,
                port,
                username,
                realm,
                trusted_cert,
            } => {
                let mut profile = paths.load_default_profile()?;
                profile.id = DEFAULT_PROFILE_ID.to_string();
                if let Some(h) = host {
                    profile.host = h;
                }
                if let Some(p) = port {
                    profile.port = p;
                }
                if let Some(u) = username {
                    profile.username = u;
                }
                if let Some(r) = realm {
                    profile.realm = r;
                }
                if let Some(c) = trusted_cert {
                    profile.set_trusted_cert(&c)?;
                }
                paths.save_profile(&profile)?;
                println!("saved {}", paths.profile_path(&profile.id).display());
            }
            ProfileCmd::Password => {
                let password = read_secret_stdin("password")?;
                if password.is_empty() {
                    return Err(tofv_core::Error::Secret("empty password".into()));
                }
                SecretToolStore.set(DEFAULT_PROFILE_ID, &password)?;
                println!("password stored in the desktop keyring (service dev.tofv)");
            }
            ProfileCmd::ClearPassword => {
                SecretToolStore.delete(DEFAULT_PROFILE_ID)?;
                println!("password removed from the desktop keyring");
            }
        },
        Command::Trust { cert } => {
            let hex = if cert == "-" {
                let mut buf = String::new();
                io::stdin().read_to_string(&mut buf)?;
                match parse_openfortivpn_output(&buf) {
                    Some(CertFinding::Unknown { sha256 })
                    | Some(CertFinding::AlreadyTrusted { sha256 }) => sha256,
                    None => return Err(tofv_core::Error::InvalidTrustedCert),
                }
            } else {
                validate_trusted_cert(&cert)?;
                cert.trim().to_ascii_lowercase()
            };
            let mut profile = paths.load_default_profile()?;
            profile.set_trusted_cert(&hex)?;
            paths.save_profile(&profile)?;
            println!("trusted-cert = {hex}");
        }
        Command::Connect {
            otp,
            dry_run,
            elevate,
        } => {
            let otp = match otp {
                Some(v) => v,
                None => prompt_totp()?,
            };
            validate_totp(&otp)?;
            let profile = paths.load_profile(DEFAULT_PROFILE_ID)?;
            let app_config = paths.load_app_config()?;
            let req = ConnectRequest {
                profile: &profile,
                otp: &otp,
                paths: &paths,
                app_config: &app_config,
                elevate: elevate.into(),
            };
            let planned = plan_session(req)?;
            println!("# command");
            println!("{}", planned.display);
            println!();
            println!("# openfortivpn config (redacted)");
            print!("{}", planned.config_redacted);
            if !planned.config_redacted.ends_with('\n') {
                println!();
            }
            if dry_run {
                return Ok(());
            }
            let password = SecretToolStore
                .get(DEFAULT_PROFILE_ID)?
                .ok_or_else(|| tofv_core::Error::PasswordMissing(DEFAULT_PROFILE_ID.into()))?;
            println!("# connecting (Ctrl+C to disconnect)");
            run_live(req, password)?;
        }
        Command::Disconnect => {
            if disconnect(&paths)? {
                println!("sent stop to the session");
            } else {
                println!("no TOFV tunnel");
            }
        }
        Command::ParseCert => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            match parse_openfortivpn_output(&buf) {
                Some(CertFinding::Unknown { sha256 })
                | Some(CertFinding::AlreadyTrusted { sha256 }) => println!("{sha256}"),
                None => {
                    eprintln!("tofv: no trusted-cert found in input");
                    return Err(tofv_core::Error::InvalidTrustedCert);
                }
            }
        }
        Command::Doctor => doctor(&paths)?,
    }
    Ok(())
}

fn doctor(paths: &AppPaths) -> tofv_core::Result<()> {
    println!("config dir:  {}", paths.config_dir.display());
    println!("runtime dir: {}", paths.runtime_dir.display());

    let report = tofv_core::doctor_report();
    for item in &report.items {
        let mark = if item.ok { "ok" } else if item.blocking { "MISSING" } else { "warn" };
        println!("{:<12} [{mark}] {}", item.label, item.detail);
    }
    println!();
    println!("paquets : {}", report.install_cmd);
    println!("helper  : {}", report.helper_cmd);

    match paths.load_profile(DEFAULT_PROFILE_ID) {
        Ok(p) => match p.validate_ready() {
            Ok(()) => println!("profile:      {}@{}:{} ready", p.username, p.host, p.port),
            Err(e) => println!("profile:      present but incomplete ({e})"),
        },
        Err(e) => println!("profile:      {e}"),
    }

    match SecretToolStore.get(DEFAULT_PROFILE_ID) {
        Ok(Some(_)) => println!("password:     stored in keyring"),
        Ok(None) => println!("password:     not stored"),
        Err(e) => println!("password:     {e}"),
    }

    if report.blocking {
        Err(tofv_core::Error::Connect(
            "prérequis bloquants manquants (voir ci-dessus)".into(),
        ))
    } else {
        Ok(())
    }
}

fn read_secret_stdin(what: &str) -> tofv_core::Result<String> {
    if io::stdin().is_terminal() {
        eprint!("enter {what}: ");
        let _ = io::stderr().flush();
    }
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;
    Ok(buf.trim_end_matches(['\n', '\r']).to_string())
}

fn prompt_totp() -> tofv_core::Result<String> {
    if !io::stdin().is_terminal() {
        return Err(tofv_core::Error::InvalidTotp);
    }
    eprint!("TOTP (6 digits): ");
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn run_live(req: ConnectRequest<'_>, password: SecretString) -> tofv_core::Result<()> {
    let (mut running, logs) = spawn_connect(req, password)?;
    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = stop.clone();
        let _ = ctrlc::set_handler(move || {
            stop.store(true, Ordering::SeqCst);
        });
    }
    let printer = thread::spawn(move || {
        for line in logs {
            println!("{line}");
        }
    });

    let outcome = loop {
        if stop.load(Ordering::SeqCst) {
            let _ = running.terminate();
        }
        match running.try_wait()? {
            Some(outcome) => break outcome,
            None => thread::sleep(Duration::from_millis(50)),
        }
    };
    drop(running);
    let _ = printer.join();

    match outcome {
        ConnectOutcome::NeedCert { sha256 } => Err(tofv_core::Error::Connect(format!(
            "gateway certificate is not trusted\nSHA-256: {sha256}\nTrust it with: tofv trust {sha256}"
        ))),
        ConnectOutcome::CertRejected => Err(tofv_core::Error::Connect(
            "gateway certificate rejected (no digest in output)".into(),
        )),
        ConnectOutcome::AuthFailed => Err(tofv_core::Error::Connect(
            "authentication failed (password or TOTP)".into(),
        )),
        ConnectOutcome::ExitedAfterUp { code } => {
            println!("# session ended (exit {code:?})");
            Ok(())
        }
        ConnectOutcome::Interrupted => {
            println!("# interrupted");
            Ok(())
        }
        ConnectOutcome::Failed { code } => Err(tofv_core::Error::Connect(format!(
            "openfortivpn exited ({code:?})"
        ))),
    }
}
