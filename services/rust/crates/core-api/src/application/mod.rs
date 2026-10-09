//! # Application layer (use cases)
//!
//! Orchestrates domain logic, persistence and the other engines for one use case at a time
//! ("ingest a record", "log in", "resolve a case"). It depends on **ports** (traits) for the other
//! services so the orchestration can be tested with fakes; see [`ports`].
//!
//! Every tenant-scoped query runs inside a `TenantTx` (RLS) *and* filters by `project_id`.

pub mod analytics;
pub mod auth;
pub mod cases;
pub mod context;
pub mod data_sources;
pub mod ingest;
pub mod pipeline;
pub mod ports;
pub mod projects;
pub mod queries;
pub mod tenants;
pub mod util;
