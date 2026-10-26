//! Agent engine. Ownership split:
//!  - runner/session/manager/tools/policy/checkpoints/context/modes/chat commands → "agent-core"
//!  - prompt/rules/memories/hooks/skills/commands/subagents/plans (+ their commands) → "agent-knowledge"
pub mod modes;
pub mod prompt;
pub mod rules;
pub mod memories;
pub mod hooks;
pub mod skills;
pub mod commands;
pub mod subagents;
pub mod plans;
pub mod frontmatter;
pub mod knowledge_commands;
pub mod session;
pub mod manager;
pub mod runner;
pub mod policy;
pub mod checkpoints;
pub mod context;
pub mod summarize;
pub mod tools;
pub mod chat_commands;
pub mod review_commands;
#[cfg(test)] pub mod testutil;
#[cfg(test)] mod tests;
// REGISTER:agent-core-modules (done)
