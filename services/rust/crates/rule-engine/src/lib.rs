//! # rule-engine — the pure domain core of the fraud rule engine
//!
//! This crate implements the rule DSL specified in `docs/technical/rule-dsl.md`: the JSON model of rules and
//! rulesets, the formula language (`F(x,y,z) = 2x + 2^y / z^2`), the three-valued evaluator
//! (`match` / `no_match` / `trapped`), the statistical velocity functions and save-time validation.
//!
//! ## Why the crate is *pure* (hexagonal architecture)
//!
//! The crate performs **no IO**: no database, no HTTP, no async runtime. Everything that needs data from the
//! outside world (velocity aggregates over history, reference-list lookups, graph metrics) goes through the
//! [`ports::DataProvider`] trait. `rule-service` implements that trait with SQL (Postgres) and HTTP
//! (graph-service); the tests in this crate implement it in memory.
//!
//! This is the "ports and adapters" (hexagonal) pattern: the domain core defines the *port* (a trait) and the
//! infrastructure provides *adapters*. It gives us:
//! * fast, deterministic unit tests of every rule semantic without Postgres;
//! * the same engine for real-time scoring, backtests, the "test rule" button and LLM proposal validation;
//! * a clear dependency direction: infrastructure depends on the domain, never the other way around.
//!
//! ## Coming from Java
//!
//! In a Java rule engine you would typically write `abstract class Rule { abstract Outcome evaluate(Context c); }`
//! with subclasses `SimpleRule`, `VelocityRule`, … and a `RuleDao` interface for data access. In Rust:
//!
//! * The set of rule kinds is **closed and known** (it is a DSL we own), so it is modelled as an `enum`
//!   ([`model::RuleDefinition`]) — an algebraic *sum type*. `match` over it is checked by the compiler for
//!   exhaustiveness: adding a new kind is a compile error everywhere it is not handled, which an abstract class
//!   hierarchy cannot guarantee. Serde maps the enum directly to/from the tagged JSON (`"kind": "velocity"`).
//! * The set of *data sources* is **open** (Postgres today, maybe a cache or a feature store tomorrow), so it
//!   is a `trait` ([`ports::DataProvider`]) — the Rust equivalent of a Java interface. Traits give dynamic
//!   dispatch (`&dyn DataProvider`) when needed without inheritance.
//! * Errors are values ([`Outcome::Trapped`], `Result`), not exceptions: the three-valued outcome is part of
//!   the domain, so it is part of the type.
//!
//! ## Entry points
//!
//! * [`model::RuleEnvelope`] / [`model::RulesetSpec`] — the JSON model.
//! * [`eval::evaluate_rule`] — evaluate one rule against an [`context::EvalContext`].
//! * [`ruleset::evaluate_rulesets`] — evaluate rulesets, aggregate scores, derive actions and traces.
//! * [`validate::validate_rule_json`] — save-time validation with precise error paths (used by the API and
//!   before any LLM proposal is stored).
//! * [`formula::Formula`] — parse/evaluate formulas (also backs the formula playground endpoint).

pub mod cache;
pub mod context;
pub mod duration;
pub mod eval;
pub mod formula;
pub mod model;
pub mod ports;
pub mod ruleset;
pub mod validate;
pub mod value;

pub use context::EvalContext;
pub use eval::{evaluate_rule, Outcome, RuleEvaluation};
pub use model::{RuleDefinition, RuleEnvelope, RulesetSpec};
pub use ports::DataProvider;
pub use ruleset::{evaluate_rulesets, EngineResult, EvalOptions, RuleUnit, RulesetUnit};
pub use validate::{validate_rule_json, FieldCatalog, ValidationError, ValidationReport};
