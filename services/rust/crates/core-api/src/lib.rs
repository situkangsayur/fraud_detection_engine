//! # core-api
//!
//! The platform's front door and orchestrator: identity, tenants/projects, data sources and the
//! mapping engine, the scoring pipeline (which calls graph-, ml- and rule-service), decisions,
//! cases, labels, analytics and audit.
//!
//! ## Layout (hexagonal / "ports & adapters")
//!
//! ```text
//! api/          HTTP adapters (axum routers)          ── depends on ──▶ application/
//! application/  use cases + ports (traits)            ── depends on ──▶ domain/, ports
//! domain/       pure business logic (no IO)
//! adapters/     implementations of ports + SQL        ── implements ──▶ application::ports
//! state.rs      dependency wiring (AppState)
//! bootstrap.rs  idempotent startup seeding
//! ```
//!
//! For a Java developer: `domain` ≈ domain model + domain services, `application` ≈ @Service
//! classes, `ports` ≈ interfaces, `adapters` ≈ @Repository / Feign clients, `api` ≈ @RestController.
//! The difference is that wiring is explicit (`state.rs`) and resolved at compile time.

pub mod adapters;
pub mod api;
pub mod application;
pub mod bootstrap;
pub mod config;
pub mod domain;
pub mod state;

pub use api::router;
