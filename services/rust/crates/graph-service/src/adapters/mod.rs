//! # Adapters — the outside of the hexagon
//!
//! Adapters implement the domain ports with real infrastructure. Here that is Postgres (sqlx).
//! Everything runs inside a [`platform::db::TenantTx`], so Row-Level Security confines each query to
//! the caller's tenant even if a `project_id` filter were forgotten. Queries still filter by
//! `project_id` explicitly.

pub mod postgres;
