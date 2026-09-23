//! # graph-service
//!
//! The graph engine of the fraud platform (docs/technical/architecture.md §2, api-contract.md §C).
//! It resolves the entities of every event (email, phone, device, card, bank account, address,
//! reference transaction, API client), links customers through them, and answers graph questions
//! such as "how many hops is this customer from a known fraudster?".
//!
//! ## Hexagonal layout (ports & adapters)
//!
//! ```text
//!            ┌──────────── api (axum handlers, auth, OpenAPI) ────────────┐
//!            │                                                            │
//!            ▼                                                            │
//!        app (use cases: one tenant transaction each)                     │
//!            │                                                            │
//!            ▼                                                            │
//!        domain (pure: extraction, similarity, BFS, metrics, components)  │
//!            │ uses the port `GraphStore` (trait)                         │
//!            ▼                                                            │
//!        adapters::postgres (sqlx implementation of the port) ◀───────────┘
//! ```
//!
//! Dependencies point **inwards**: `domain` depends on nothing in this crate, `adapters`
//! implements a `domain` trait, and `api` knows only `app`. Java developers will recognise
//! Clean/Hexagonal Architecture. In Rust, traits play the role of interfaces, and generics
//! replace the DI container.

pub mod adapters;
pub mod api;
pub mod app;
pub mod domain;

pub use api::router;
pub use app::state::AppState;
