//! Luna's engine: providers, tools, the agentic loop, sessions and the
//! multi-agent orchestrator.
//!
//! Both front-ends — the ratatui TUI and the Tauri GUI — drive this crate and
//! consume the same [`event::AgentEvent`] stream, so behaviour cannot drift
//! between them.

pub mod agent;
pub mod catalog;
pub mod config;
pub mod error;
pub mod event;
pub mod orchestrator;
pub mod presets;
pub mod provider;
pub mod session;
pub mod tokens;
pub mod tool;
pub mod types;

pub use config::{Config, Paths};
pub use error::{LunaError, Result};
pub use event::{AgentEvent, AgentRef, Emitted, EventSink, NullSink, SinkRef};
pub use types::{
    ContentBlock, Cost, Message, ModelInfo, Role, StopReason, ThinkingLevel, ToolCall, ToolResult,
    ToolSchema, Usage,
};

/// The version reported to front-ends.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
