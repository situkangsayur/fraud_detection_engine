//! Adapters: everything that talks to the outside world (Postgres, graph-service, caches).
//!
//! Each adapter implements a port defined elsewhere (the rule engine's `DataProvider` / `FieldCatalog` traits)
//! or is a plain repository used by the application layer. Swapping Postgres for another store would touch
//! only this module.

pub mod catalog;
pub mod data_provider;
pub mod event_context;
pub mod provider_cache;
pub mod repo;
pub mod serving;
pub mod velocity_sql;
