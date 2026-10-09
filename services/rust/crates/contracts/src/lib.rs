//! # contracts — wire DTOs shared by the Rust services
//!
//! These types are the **published language** between services (docs/technical/api-contract.md).
//! When core-api calls rule-service's `/v1/projects/{pid}/evaluate`, both sides use
//! [`scoring::EvaluateRequest`] from this crate. That removes a whole class of "the JSON field was
//! renamed on one side only" bugs at compile time.
//!
//! Rules for this crate:
//! * data only: plain `serde` structs/enums, no IO, no business logic;
//! * **additive evolution**: new optional fields use `#[serde(default)]`, so an older peer keeps
//!   working during a rolling deploy;
//! * `snake_case` on the wire; enums serialise as lower-case strings.
//!
//! Python services (ml, llm, ingest) implement the same JSON shapes with Pydantic models.

pub mod catalog;
pub mod common;
pub mod events;
pub mod graph;
pub mod ml;
pub mod scoring;

pub use common::{Decision, EventType, RuleAction, RuleKind, RuleOutcome, Typology};
