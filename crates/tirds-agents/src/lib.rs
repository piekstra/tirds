pub mod claude_cli;
pub mod error;
pub mod orchestrator;
pub mod parser;
pub mod prompts;
pub mod specialist;

pub mod test_support;

pub use error::AgentError;
pub use orchestrator::{build_trade_decision, Orchestrator};
pub use specialist::{ClaudeSpecialist, SpecialistAgent};
