//! Tool abstraction, registry, built-ins, and permission gating.

pub mod builtin;
pub mod permissions;
pub mod registry;

pub use registry::{Tool, ToolOutcome, ToolRegistry};
