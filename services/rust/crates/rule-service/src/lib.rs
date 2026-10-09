//! # rule-service
//!
//! Serves the rule engine for every project: rule/ruleset/reference-list management with maker–checker
//! governance, the internal `evaluate` endpoint used by core-api's scoring pipeline, backtests, proposals and
//! stage templates (api-contract.md §B).
//!
//! ## Layout (hexagonal / "ports and adapters")
//!
//! ```text
//!   api/      HTTP: axum handlers + DTOs + OpenAPI          ─┐ depends on
//!   app/      use cases: evaluate, backtest, workflow        ─┤
//!   adapters/ Postgres repos, SQL compiler, graph client,     │
//!             caches — implement the engine's ports           │
//!   domain/   pure rules: lifecycle, proposals, backtest      ◀┘
//!             math, templates, CSV parsing
//!   rule-engine (separate crate): the pure DSL + evaluator + ports (traits)
//! ```
//!
//! Dependencies point inwards: `domain` and `rule-engine` know nothing about HTTP or SQL. For a Java developer,
//! this is the same shape as `controller → service → repository`, plus an explicit *port* (a trait, i.e. an
//! interface) between the evaluation core and its data sources. That port is what keeps the engine pure and
//! testable with in-memory fakes.

pub mod adapters;
pub mod api;
pub mod app;
pub mod config;
pub mod domain;
pub mod state;
