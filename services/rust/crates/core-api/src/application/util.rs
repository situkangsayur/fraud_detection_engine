//! Small helpers shared by use cases.

use once_cell::sync::Lazy;
use platform::auth::{Caller, CallerKind, ProjectRole};
use platform::pagination::{Page, PageParams};
use platform::ProjectId;
use regex::Regex;
use serde_json::Value;

static SLUG: Lazy<Option<Regex>> = Lazy::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]{1,62}$").ok());
static SOURCE_SLUG: Lazy<Option<Regex>> = Lazy::new(|| Regex::new(r"^[a-z0-9][a-z0-9_-]{1,62}$").ok());

pub fn valid_slug(s: &str) -> bool {
    SLUG.as_ref().is_some_and(|r| r.is_match(s))
}

pub fn valid_source_slug(s: &str) -> bool {
    SOURCE_SLUG.as_ref().is_some_and(|r| r.is_match(s))
}

/// Builds a page from `(item, total)` rows produced by `SELECT to_jsonb(t), count(*) OVER ()`.
pub fn collect_page(rows: Vec<(Value, i64)>, params: &PageParams) -> Page<Value> {
    let total = rows.first().map(|r| r.1).unwrap_or(0);
    Page::new(rows.into_iter().map(|r| r.0).collect(), total, params)
}

/// Effective role of the caller in a project (after `require_project_role` succeeded).
/// Services and tenant admins count as `ProjectAdmin`.
pub fn effective_role(caller: &Caller, project: ProjectId) -> ProjectRole {
    match &caller.kind {
        CallerKind::Service { .. } => ProjectRole::ProjectAdmin,
        CallerKind::User(u) => {
            u.project_role(project)
                .unwrap_or(if u.claims.trole == platform::auth::TenantRole::TenantAdmin {
                    ProjectRole::ProjectAdmin
                } else {
                    ProjectRole::Viewer
                })
        }
    }
}

/// Masks e-mail / phone / address fields of a JSON object in place (viewer role).
pub fn mask_pii(v: &mut Value) {
    use platform::pii::{mask_email, mask_phone, mask_text};
    if let Some(m) = v.as_object_mut() {
        for (k, val) in m.iter_mut() {
            if let Value::String(s) = val {
                let masked = match k.as_str() {
                    "email" | "email_normalized" => Some(mask_email(s)),
                    "phone" | "phone_normalized" => Some(mask_phone(s)),
                    "shipping_address" | "billing_address" | "full_name" => Some(mask_text(s, 3)),
                    _ => None,
                };
                if let Some(x) = masked {
                    *val = Value::String(x);
                }
            } else if val.is_object() {
                mask_pii(val);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn slugs() {
        assert!(valid_slug("checkout"));
        assert!(valid_slug("post-payment"));
        assert!(!valid_slug("Post"));
        assert!(!valid_slug("a"));
        assert!(valid_source_slug("erp_orders"));
    }

    #[test]
    fn masking() {
        let mut v = json!({ "email": "budi@example.com", "customer": { "phone": "+6281234567890" }, "x": 1 });
        mask_pii(&mut v);
        assert_ne!(v["email"], json!("budi@example.com"));
        assert_ne!(v["customer"]["phone"], json!("+6281234567890"));
        assert_eq!(v["x"], json!(1));
    }
}
