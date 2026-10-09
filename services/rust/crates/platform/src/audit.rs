//! Append-only audit trail (`core.audit_log`; UPDATE/DELETE are blocked by a trigger).
//!
//! Call [`record`] **inside the same transaction** as the change it describes, so the audit row
//! and the change commit or roll back together. That is the main reason this is a function
//! taking an executor and not a fire-and-forget HTTP call.

use serde::Serialize;
use serde_json::Value;
use sqlx::{Executor, Postgres};

use crate::auth::Caller;
use crate::error::AppResult;
use crate::ids::{ProjectId, TenantId};

#[derive(Debug, Clone, Default, Serialize)]
pub struct AuditEntry {
    pub tenant_id: Option<TenantId>,
    pub project_id: Option<ProjectId>,
    /// `user` | `service` | `system`
    pub actor_type: &'static str,
    pub actor_id: Option<String>,
    /// Dotted verb, e.g. `rule.create`, `rule.approve`, `project.settings.update`.
    pub action: String,
    pub subject_type: Option<String>,
    pub subject_id: Option<String>,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub metadata: Value,
    pub request_id: Option<String>,
}

impl AuditEntry {
    /// Starts an entry for `action` performed by `caller`.
    pub fn by(caller: &Caller, action: impl Into<String>) -> Self {
        let (actor_type, actor_id) = caller.audit_actor();
        Self {
            actor_type,
            actor_id,
            action: action.into(),
            metadata: Value::Object(Default::default()),
            ..Default::default()
        }
    }

    pub fn system(action: impl Into<String>) -> Self {
        Self {
            actor_type: "system",
            action: action.into(),
            metadata: Value::Object(Default::default()),
            ..Default::default()
        }
    }

    pub fn scope(mut self, tenant: TenantId, project: Option<ProjectId>) -> Self {
        self.tenant_id = Some(tenant);
        self.project_id = project;
        self
    }

    pub fn subject(mut self, subject_type: impl Into<String>, subject_id: impl ToString) -> Self {
        self.subject_type = Some(subject_type.into());
        self.subject_id = Some(subject_id.to_string());
        self
    }

    pub fn before(mut self, v: impl Serialize) -> Self {
        self.before = serde_json::to_value(v).ok();
        self
    }

    pub fn after(mut self, v: impl Serialize) -> Self {
        self.after = serde_json::to_value(v).ok();
        self
    }

    pub fn metadata(mut self, v: Value) -> Self {
        self.metadata = v;
        self
    }

    pub fn request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }
}

/// Inserts the entry using any executor (pool, connection or transaction).
pub async fn record<'e, E>(executor: E, entry: &AuditEntry) -> AppResult<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        "INSERT INTO core.audit_log (tenant_id, project_id, actor_type, actor_id, action, subject_type, subject_id, \
         before, after, metadata, request_id) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(entry.tenant_id.map(|t| t.0))
    .bind(entry.project_id.map(|p| p.0))
    .bind(entry.actor_type)
    .bind(&entry.actor_id)
    .bind(&entry.action)
    .bind(&entry.subject_type)
    .bind(&entry.subject_id)
    .bind(&entry.before)
    .bind(&entry.after)
    .bind(&entry.metadata)
    .bind(&entry.request_id)
    .execute(executor)
    .await?;
    Ok(())
}
