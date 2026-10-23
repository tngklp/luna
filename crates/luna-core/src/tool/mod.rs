//! Tool abstraction, registry, built-ins, and permission gating.

pub mod builtin;
pub mod mcp;
pub mod permissions;
pub mod registry;
pub mod web;

pub use registry::{Tool, ToolOutcome, ToolRegistry};
