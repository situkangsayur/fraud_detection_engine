//! # Domain layer (pure)
//!
//! Business rules that do not need IO: the mapping engine, event normalisation, feature
//! arithmetic, the decision combiner, settings validation and drift statistics.
//!
//! In a Java code base these would be "domain services" or value objects. The important property is
//! the same: nothing here knows about HTTP, SQL or other services, so it is tested with plain
//! unit tests and can be reused by every entry point (webhook, batch, preview, simulate).

pub mod combine;
pub mod features;
pub mod mapping;
pub mod normalize;
pub mod psi;
pub mod settings;
