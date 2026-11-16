use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Standardized RUVI exit codes (see RUVIC_ARCHITECTURE.md §8 and RUVI_MASTER_ARCHITECTURE.md §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ExitCode {
    Success = 0,
    General = 1,
    ConfigOrArgs = 2,
    Unsupported = 4,
    Filesystem = 5,
    Backend = 6,
    Cancelled = 7,
    Integrity = 8,
}

impl ExitCode {
    pub fn code(self) -> i32 {
        self as i32
    }

    pub fn as_process_exit_code(self) -> std::process::ExitCode {
        std::process::ExitCode::from(self as u8)
    }

    pub fn category(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::General => "general",
            Self::ConfigOrArgs => "config_or_args",
            Self::Unsupported => "unsupported",
            Self::Filesystem => "filesystem",
            Self::Backend => "backend",
            Self::Cancelled => "cancelled",
            Self::Integrity => "integrity",
        }
    }
}

/// JSON error envelope emitted on failure when `--json` is active.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorEnvelope {
    pub ok: bool,
    pub exit_code: i32,
    pub category: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Domain and infrastructure errors for RUVIC.
#[derive(Debug, Error)]
pub enum RuviError {
    #[error("{0}")]
    General(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("unsupported format or codec: {0}")]
    Unsupported(String),

    #[error("filesystem error: {0}")]
    Filesystem(String),

    #[error("media backend error: {0}")]
    Backend(String),

    #[error("operation cancelled: {0}")]
    Cancelled(String),

    #[error("output validation error: {0}")]
    Integrity(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl RuviError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::General(_) => ExitCode::General,
            Self::InvalidArgument(_) | Self::Config(_) => ExitCode::ConfigOrArgs,
            Self::Unsupported(_) => ExitCode::Unsupported,
            Self::Filesystem(_) | Self::Io(_) => ExitCode::Filesystem,
            Self::Backend(_) => ExitCode::Backend,
            Self::Cancelled(_) => ExitCode::Cancelled,
            Self::Integrity(_) => ExitCode::Integrity,
        }
    }

    pub fn to_envelope(&self) -> ErrorEnvelope {
        let code = self.exit_code();
        ErrorEnvelope {
            ok: false,
            exit_code: code.code(),
            category: code.category().to_string(),
            message: self.to_string(),
            detail: None,
        }
    }
}

pub type RuviResult<T> = Result<T, RuviError>;
