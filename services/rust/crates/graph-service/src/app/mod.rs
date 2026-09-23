//! # Application layer — use cases
//!
//! Each function here is one use case (Java: a `@Service` method annotated `@Transactional`). It
//! opens a tenant-scoped transaction, loads the project's graph configuration, calls the domain
//! algorithms through the Postgres adapter, and commits. HTTP concerns (status codes, JSON
//! shapes, auth) stay in `api`, and the algorithms stay in `domain`.

pub mod links;
pub mod project_config;
pub mod queries;
pub mod state;
