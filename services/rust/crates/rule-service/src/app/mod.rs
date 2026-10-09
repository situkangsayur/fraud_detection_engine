//! Application layer (use cases). Orchestrates domain rules, the rule engine and adapters; owns transactions.
//! Handlers in [`crate::api`] call these functions; nothing here knows about HTTP.

pub mod backtest;
pub mod evaluation;
pub mod workflow;
