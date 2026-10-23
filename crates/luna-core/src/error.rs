//! Unified error type for Luna core.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("provider `{provider}` returned HTTP {status}: {body}")]
    ProviderHttp {
        provider: String,
        status: u16,
        body: String,
    },

    #[error("tool error: {0}")]
    Tool(String),

    #[error("tool `{tool}` was denied by permission policy: {reason}")]
    ToolDenied { tool: String, reason: String },

    #[error("orchestrator error: {0}")]
    Orchestrator(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("run cancelled")]
    Cancelled,

    #[error("agent exceeded maximum iterations ({0})")]
    MaxIterations(u32),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

impl Error {
    /// True when the error is a user-initiated cancellation.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Error::Cancelled)
    }
}
