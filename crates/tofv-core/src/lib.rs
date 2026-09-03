//! Core logic for TOFV: one profile, command planning, log redaction,
//! and the pinentry socket used when `openfortivpn` runs as root.

mod command;
mod doctor;
mod error;
mod live;
mod openfortivpn_conf;
mod parse;
mod paths;
mod pinentry_socket;
mod profile;
mod redact;
mod runner;
mod secret;
mod session;

pub use command::{plan_connect, PlanRequest, PlannedInvocation};
pub use doctor::{appindicator_available, report as doctor_report, DoctorItem, DoctorReport};
pub use error::{Error, Result};
pub use live::{probe_live_tunnel, LiveTunnel};
pub use openfortivpn_conf::render as render_openfortivpn_config;
pub use parse::{
    looks_auth_failed, looks_cert_failed, looks_tunnel_up, parse_openfortivpn_output, CertFinding,
};
pub use paths::{
    helper_version, install_pinentry_wrapper, openfortivpn_version, resolve_helper,
    resolve_openfortivpn, resolve_pinentry, resolve_pinentry_bin, AppConfig, AppPaths, Elevate,
};
pub use pinentry_socket::{
    discover_socket_path, fetch_password, percent_decode, percent_encode, PinentryServer,
    PinentryShutdown,
};
pub use profile::{
    validate_totp, validate_trusted_cert, AuthMethod, Profile, DEFAULT_PORT, DEFAULT_PROFILE_ID,
};
pub use redact::{redact_line, redact_text};
pub use runner::{disconnect, spawn_connect, ConnectOutcome, RunningConnect, SessionSnapshot};
pub use secret::{which, MemoryStore, PasswordStore, SecretString, SecretToolStore};
pub use session::{plan as plan_session, ConnectRequest, SessionFiles};
