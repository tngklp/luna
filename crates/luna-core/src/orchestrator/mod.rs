//! Orchestrator: multi-agent coordination (delegation, fan-out, join).

pub mod agent_loop;
pub mod messaging;
pub mod registry;
pub mod runner;

pub use agent_loop::{AgentContext, AgentRunner, AgentSpec, ApprovalSink, AutoApprove, DenyAll};
pub use messaging::{aggregate_results, AgentMessage, MessageHub, MessageKind};
pub use registry::AgentRegistry;
pub use runner::Orchestrator;
