//! Domain layer: pure business rules of the rule service (no IO).
//!
//! The *evaluation* semantics (operators, Kleene logic, statistics, scoring) live in the separate pure
//! `rule-engine` crate. This module holds what is specific to operating rules inside a platform:
//! governance ([`lifecycle`]), proposals ([`proposal`]), backtest statistics ([`backtest`]), stage templates
//! ([`templates`]) and CSV parsing ([`csv_import`]).
//!
//! Dependency direction (the "hexagon"): `api` → `adapters` → `domain` / `rule-engine`. Nothing in `domain`
//! imports `sqlx`, `axum` or `reqwest`, so every function here is unit-tested without containers.

pub mod backtest;
pub mod csv_import;
pub mod lifecycle;
pub mod proposal;
pub mod templates;
