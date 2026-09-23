//! # Domain layer — pure graph logic, no IO
//!
//! This is the centre of the hexagon ("ports and adapters"). Everything here is plain Rust:
//! no SQL, no HTTP, no clock. That makes the graph algorithms (BFS distances, fraud proximity,
//! components, similarity rules) unit-testable with an in-memory graph in microseconds.
//!
//! ## For a Java developer
//!
//! In a Spring project this would be the `service` package calling a `GraphRepository` interface,
//! with a JPA implementation injected at runtime. Here:
//!
//! | Java / Spring                          | This crate                                       |
//! |----------------------------------------|--------------------------------------------------|
//! | `interface GraphRepository`            | [`ports::GraphStore`] (an `async` trait)         |
//! | `@Repository class JpaGraphRepository` | `adapters::postgres::PgGraphStore`               |
//! | `@Service class GraphService`          | `app::*` use cases + [`explore`] / [`metrics`]   |
//! | DI container wiring                    | generics: `fn compute<S: GraphStore>(store: &mut S)` |
//!
//! The algorithms are **generic over the port** (`S: GraphStore`), so the compiler generates one
//! specialised copy per implementation (static dispatch, no virtual calls). Tests pass an
//! in-memory store; production passes the Postgres adapter. No framework or reflection is needed.
//!
//! ## Graph model
//!
//! The stored graph is **bipartite**: customers ↔ entities (email, phone, device, card, …).
//! Two customers are *one hop* apart when they share an entity of an allowed kind, or, with
//! `include_similar`, when their entities are linked by a similarity edge (phone differing by one
//! digit, near-identical address, …). "Supernode" entities shared by more than
//! `supernode_degree_cap` customers (a public Wi-Fi IP, a marketplace's warehouse address) are never
//! traversed, because they would connect everyone to everyone.

pub mod components;
pub mod explore;
pub mod extract;
pub mod metrics;
pub mod model;
pub mod ports;
pub mod similarity;

#[cfg(test)]
pub mod memory;
