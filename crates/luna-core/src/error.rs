use std::path::PathBuf;

/// Everything Luna can fail with, in one place.
#[derive(Debug, thiserror::Error)]
pub enum LunaError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("{provider} returned {status}: {message}")]
    Provider { provider: String, status: u16, message: String, retryable: bool },

    #[error("network error: {0}")]
    Network(String),

    #[error("invalid response from provider: {0}")]
    Decode(String),

    #[error("i/o error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("i/o error: {0}")]
    IoPlain(#[from] std::io::Error),

    #[error("session store error: {0}")]
    Store(String),

    #[error("{0}")]
    Tool(String),

    #[error("cancelled")]
    Cancelled,

    #[error("budget exceeded: {0}")]
    Budget(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, LunaError>;

impl LunaError {
    /// Whether retrying the exact same request could plausibly succeed.
    pub fn retryable(&self) -> bool {
        match self {
            LunaError::Provider { retryable, .. } => *retryable,
            LunaError::Network(_) => true,
            LunaError::Decode(_) => false,
            LunaError::Io { .. } | LunaError::IoPlain(_) => true,
            _ => false,
        }
    }

    pub fn tool(msg: impl Into<String>) -> Self {
        LunaError::Tool(msg.into())
    }

    pub fn config(msg: impl Into<String>) -> Self {
        LunaError::Config(msg.into())
    }

    pub fn other(msg: impl Into<String>) -> Self {
        LunaError::Other(msg.into())
    }
}

impl From<reqwest::Error> for LunaError {
    fn from(value: reqwest::Error) -> Self {
        LunaError::Network(value.to_string())
    }
}

impl From<serde_json::Error> for LunaError {
    fn from(value: serde_json::Error) -> Self {
        LunaError::Decode(value.to_string())
    }
}

impl From<rusqlite::Error> for LunaError {
    fn from(value: rusqlite::Error) -> Self {
        LunaError::Store(value.to_string())
    }
}

impl From<LunaError> for std::io::Error {
    fn from(value: LunaError) -> Self {
        match value {
            LunaError::IoPlain(err) => err,
            other => std::io::Error::other(other.to_string()),
        }
    }
}
