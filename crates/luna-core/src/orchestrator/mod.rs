//! Orchestrator: multi-agent coordination (delegation, fan-out, join).

pub mod agent_loop;
pub mod messaging;
pub mod registry;
pub mod runner;

pub use agent_loop::{AgentContext, AgentRunner, AgentSpec, ApprovalSink, AutoApprove, DenyAll};
pub use messaging::{AgentMessage, MessageHub, MessageKind, aggregate_results};
pub use registry::AgentRegistry;
pub use runner::Orchestrator;
