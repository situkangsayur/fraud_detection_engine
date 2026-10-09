//! # Adapters (infrastructure)
//!
//! Implementations of the ports and the SQL used by the application layer:
//! * [`engines`]: HTTP clients for rule-, graph- and ml-service (implement `application::ports`);
//! * [`repo`] / [`features_sql`]: Postgres persistence for the scoring hot path;
//! * [`crypto`]: argon2id hashing and random tokens.

pub mod crypto;
pub mod engines;
pub mod features_sql;
pub mod repo;
