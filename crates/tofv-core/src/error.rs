use std::io;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("profile `{0}` not found")]
    ProfileNotFound(String),

    #[error("invalid profile: {0}")]
    InvalidProfile(String),

    #[error("invalid TOTP: expected 6 digits")]
    InvalidTotp,

    #[error("invalid trusted-cert: expected 64 hex characters (SHA-256)")]
    InvalidTrustedCert,

    #[error("openfortivpn not found (install it, or set TOFV_OPENFORTIVPN / config openfortivpn)")]
    OpenfortivpnNotFound,

    #[error("pinentry-tofv not found (build the workspace, or set TOFV_PINENTRY)")]
    PinentryNotFound,

    #[error(
        "tofv-helper not installed — run ./scripts/install.sh (refusing to pkexec openfortivpn)"
    )]
    HelperNotFound,

    #[error("{0}")]
    Connect(String),

    #[error("password is not stored for profile `{0}`")]
    PasswordMissing(String),

    #[error("secret store: {0}")]
    Secret(String),

    #[error("pinentry socket: {0}")]
    PinentrySocket(String),

    #[error("path error: {0}")]
    Path(String),

    #[error("I/O error on {path}: {source}")]
    IoPath {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("toml deserialize: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("toml serialize: {0}")]
    TomlSer(#[from] toml::ser::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
