//! # platform — shared infrastructure for every Rust service
//!
//! `core-api`, `rule-service` and `graph-service` are separate binaries, but they share the same
//! cross-cutting concerns: configuration, logging/metrics, error format, authentication,
//! tenant isolation, outgoing HTTP, pagination, audit, and the HTTP server itself.
//!
//! ## Why a library crate instead of a base class
//!
//! In Java you would typically write an `AbstractService` / `BaseController` and let every
//! service inherit from it. Rust has no inheritance. We use **composition** instead:
//!
//! * small, focused modules with free functions and plain structs ([`server::serve`],
//!   [`db::TenantTx`], [`http::ServiceClient`]);
//! * **axum extractors** ([`auth::AuthUser`], [`auth::Caller`]) for what a Java framework would do
//!   with filters/interceptors and `@AuthenticationPrincipal`. A handler that declares a
//!   `Caller` parameter *cannot* run without an authenticated caller: the type system enforces it;
//! * **traits** ([`auth::ProjectDirectory`], [`server::ReadinessCheck`]) where a service must plug in
//!   its own behaviour — the Rust equivalent of an interface.
//!
//! Each service depends on this crate and composes the parts it needs. Nothing here knows about
//! rules, graphs or events: domain logic lives in the services (and in the pure `rule-engine` crate).

pub mod audit;
pub mod auth;
pub mod cli;
pub mod config;
pub mod db;
pub mod error;
pub mod http;
pub mod pagination;
pub mod pii;
pub mod query_time;
pub mod server;
pub mod telemetry;

pub use error::{AppError, AppResult};

/// Strongly-typed ids. A `TenantId` can never be passed where a `ProjectId` is expected.
/// This is the "newtype" pattern: zero runtime cost, compile-time safety.
pub mod ids {
    use serde::{Deserialize, Serialize};
    use std::fmt;
    use uuid::Uuid;

    macro_rules! id_type {
        ($name:ident) => {
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
            #[serde(transparent)]
            pub struct $name(pub Uuid);

            impl $name {
                pub fn as_uuid(&self) -> Uuid {
                    self.0
                }
            }

            impl From<Uuid> for $name {
                fn from(u: Uuid) -> Self {
                    Self(u)
                }
            }

            impl fmt::Display for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    self.0.fmt(f)
                }
            }
        };
    }

    id_type!(TenantId);
    id_type!(ProjectId);
    id_type!(UserId);
}

pub use ids::{ProjectId, TenantId, UserId};
