use thiserror::Error;

/// Operational failures. Diagnostics never include remote-controlled reason strings.
#[derive(Debug, Error)]
pub enum Error {
    #[error("local I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid or unsafe identity: {0}")]
    Identity(&'static str),
    #[error("private identity storage requires Unix permission support")]
    UnsupportedPlatform,
    #[error("peer authentication failed; verify both public certificates and the clock")]
    Authentication,
    #[error("network operation failed; check the address, UDP route and firewall")]
    Network,
    #[error("operation deadline exceeded; delivery may be uncertain")]
    Timeout,
    #[error("invalid message or receipt")]
    Protocol,
    #[error("message exceeds the {}-byte limit", crate::config::MAX_MESSAGE_BYTES)]
    MessageTooLarge,
    #[error("operation cancelled; delivery may be uncertain")]
    Cancelled,
    #[error("invalid configuration: {0}")]
    Configuration(&'static str),
    #[error("local discovery failed: {0}")]
    Discovery(&'static str),
    #[error("file integrity check failed")]
    Integrity,
    #[error("file destination refused: {0}")]
    Destination(&'static str),
    #[error("file exceeds the permitted byte count")]
    FileTooLarge,
    #[error("encryption or decryption failed")]
    Encryption,
    #[error("pairing failed or invitation is no longer available")]
    Pairing,
    #[error("operation is not authorized by the contact policy")]
    Denied,
    #[error("peer identity is revoked")]
    Revoked,
}

impl Error {
    /// Stable machine-readable classification, independent of diagnostic wording.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "IO",
            Self::Identity(_) => "IDENTITY",
            Self::UnsupportedPlatform => "UNSUPPORTED_PLATFORM",
            Self::Authentication => "AUTHENTICATION",
            Self::Network => "NETWORK",
            Self::Timeout => "TIMEOUT",
            Self::Protocol => "PROTOCOL",
            Self::MessageTooLarge => "MESSAGE_TOO_LARGE",
            Self::Cancelled => "CANCELLED",
            Self::Configuration(_) => "CONFIGURATION",
            Self::Discovery(_) => "DISCOVERY",
            Self::Integrity => "INTEGRITY",
            Self::Destination(_) => "DESTINATION",
            Self::FileTooLarge => "FILE_TOO_LARGE",
            Self::Encryption => "ENCRYPTION",
            Self::Pairing => "PAIRING",
            Self::Denied => "DENIED",
            Self::Revoked => "REVOKED",
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(unix)]
impl From<rustix::io::Errno> for Error {
    fn from(error: rustix::io::Errno) -> Self {
        Self::Io(error.into())
    }
}

impl From<quinn::ReadError> for Error {
    fn from(error: quinn::ReadError) -> Self {
        match error {
            quinn::ReadError::ConnectionLost(error) => connection_error(error),
            _ => Self::Protocol,
        }
    }
}

impl From<quinn::ReadExactError> for Error {
    fn from(error: quinn::ReadExactError) -> Self {
        match error {
            quinn::ReadExactError::ReadError(error) => error.into(),
            quinn::ReadExactError::FinishedEarly(_) => Self::Protocol,
        }
    }
}

impl From<quinn::ReadToEndError> for Error {
    fn from(error: quinn::ReadToEndError) -> Self {
        match error {
            quinn::ReadToEndError::Read(error) => error.into(),
            quinn::ReadToEndError::TooLong => Self::Protocol,
        }
    }
}

impl From<quinn::WriteError> for Error {
    fn from(error: quinn::WriteError) -> Self {
        match error {
            quinn::WriteError::ConnectionLost(error) => connection_error(error),
            _ => Self::Network,
        }
    }
}

pub(crate) fn connection_error(error: quinn::ConnectionError) -> Error {
    match error {
        quinn::ConnectionError::TimedOut => Error::Timeout,
        quinn::ConnectionError::TransportError(error)
            if (0x100..=0x1ff).contains(&u64::from(error.code)) =>
        {
            Error::Authentication
        }
        quinn::ConnectionError::ConnectionClosed(close)
            if (0x100..=0x1ff).contains(&u64::from(close.error_code)) =>
        {
            Error::Authentication
        }
        _ => Error::Network,
    }
}
