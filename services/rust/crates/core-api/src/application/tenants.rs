//! Tenants and tenant users (platform admin / tenant admin use cases).

use platform::audit::{self, AuditEntry};
use platform::auth::Caller;
use platform::error::{AppError, AppResult, FieldError};
use platform::pagination::{Page, PageParams};
use platform::TenantId;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::adapters::crypto::hash_secret;
use crate::state::AppState;

use super::util::{collect_page, valid_slug};

pub const MIN_PASSWORD_LEN: usize = 10;

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct NewUserIn {
    pub email: String,
    pub full_name: String,
    pub password: String,
    #[serde(default)]
    pub tenant_role: Option<String>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct NewTenantIn {
    pub slug: String,
    pub name: String,
    pub admin: NewUserIn,
}

#[derive(Debug, Clone, Default, Deserialize, utoipa::ToSchema)]
pub struct PatchTenantIn {
    pub name: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, utoipa::ToSchema)]
pub struct PatchUserIn {
    pub tenant_role: Option<String>,
    pub is_active: Option<bool>,
    pub password: Option<String>,
    pub full_name: Option<String>,
}

fn validate_user(u: &NewUserIn, errors: &mut Vec<FieldError>, prefix: &str) {
    let email = u.email.trim();
    if email.len() > 320 || !email.contains('@') || email.starts_with('@') || email.ends_with('@') {
        errors.push(FieldError::new(format!("{prefix}email"), "invalid email"));
    }
    if u.full_name.trim().is_empty() || u.full_name.len() > 200 {
        errors.push(FieldError::new(
            format!("{prefix}full_name"),
            "required, max 200 characters",
        ));
    }
    if u.password.chars().count() < MIN_PASSWORD_LEN || u.password.len() > 1024 {
        errors.push(FieldError::new(
            format!("{prefix}password"),
            format!("at least {MIN_PASSWORD_LEN} characters"),
        ));
    }
    if let Some(r) = &u.tenant_role {
        if r != "tenant_admin" && r != "member" {
            errors.push(FieldError::new(
                format!("{prefix}tenant_role"),
                "tenant_admin | member",
            ));
        }
    }
}

/// Only the platform admin or the tenant's own admin may manage it.
fn require_pa_or_ta(caller: &Caller, tid: TenantId) -> AppResult<()> {
    if caller.require_platform_admin().is_ok() {
        return Ok(());
    }
    caller.require_tenant_admin(tid)
}

pub async fn list(st: &AppState, caller: &Caller, page: &PageParams) -> AppResult<Page<Value>> {
    caller.require_platform_admin()?;
    let rows: Vec<(Value, i64)> = sqlx::query_as(
        "SELECT to_jsonb(t) AS item, count(*) OVER () FROM \
           (SELECT id, slug, name, status, created_at, \
                   (SELECT count(*) FROM core.app_users u WHERE u.tenant_id = tn.id) AS users \
            FROM core.tenants tn ORDER BY created_at) t LIMIT $1 OFFSET $2",
    )
    .bind(page.limit())
    .bind(page.offset())
    .fetch_all(&st.pool)
    .await?;
    Ok(collect_page(rows, page))
}

pub async fn create(st: &AppState, caller: &Caller, input: &NewTenantIn) -> AppResult<Value> {
    caller.require_platform_admin()?;
    let mut errors = Vec::new();
    if !valid_slug(&input.slug) {
        errors.push(FieldError::new(
            "slug",
            "lower-case letters, digits and '-', 2-63 characters",
        ));
    }
    if input.name.trim().is_empty() || input.name.len() > 200 {
        errors.push(FieldError::new("name", "required, max 200 characters"));
    }
    validate_user(&input.admin, &mut errors, "admin.");
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    let hash = hash_secret(&input.admin.password)?;
    let mut tx = st.pool.begin().await?;
    let (tid,): (Uuid,) =
        sqlx::query_as("INSERT INTO core.tenants (slug, name) VALUES ($1, $2) RETURNING id")
            .bind(&input.slug)
            .bind(input.name.trim())
            .fetch_one(&mut *tx)
            .await?;
    let (uid,): (Uuid,) = sqlx::query_as(
        "INSERT INTO core.app_users (tenant_id, email, full_name, password_hash, tenant_role) \
         VALUES ($1, $2, $3, $4, 'tenant_admin') RETURNING id",
    )
    .bind(tid)
    .bind(input.admin.email.trim())
    .bind(input.admin.full_name.trim())
    .bind(hash)
    .fetch_one(&mut *tx)
    .await?;
    let out = json!({ "id": tid, "slug": input.slug, "name": input.name.trim(), "status": "active",
                      "admin_user_id": uid });
    audit::record(
        &mut *tx,
        &AuditEntry::by(caller, "tenant.create")
            .scope(TenantId(tid), None)
            .subject("tenant", tid)
            .after(&out),
    )
    .await?;
    tx.commit().await?;
    Ok(out)
}

pub async fn get(st: &AppState, caller: &Caller, tid: TenantId) -> AppResult<Value> {
    require_pa_or_ta(caller, tid)?;
    let row: Option<(Value,)> = sqlx::query_as(
        "SELECT to_jsonb(t) FROM (SELECT id, slug, name, status, settings, created_at, updated_at \
         FROM core.tenants WHERE id = $1) t",
    )
    .bind(tid.as_uuid())
    .fetch_optional(&st.pool)
    .await?;
    row.map(|r| r.0)
        .ok_or_else(|| AppError::not_found("tenant not found"))
}

pub async fn patch(st: &AppState, caller: &Caller, tid: TenantId, input: &PatchTenantIn) -> AppResult<Value> {
    require_pa_or_ta(caller, tid)?;
    if input.status.is_some() {
        caller.require_platform_admin()?;
    }
    if let Some(s) = &input.status {
        if s != "active" && s != "suspended" {
            return Err(AppError::field("status", "active | suspended"));
        }
    }
    if let Some(n) = &input.name {
        if n.trim().is_empty() || n.len() > 200 {
            return Err(AppError::field("name", "required, max 200 characters"));
        }
    }
    let before = get(st, caller, tid).await?;
    let mut tx = st.pool.begin().await?;
    sqlx::query(
        "UPDATE core.tenants SET name = COALESCE($2, name), status = COALESCE($3, status) WHERE id = $1",
    )
    .bind(tid.as_uuid())
    .bind(input.name.as_deref().map(str::trim))
    .bind(&input.status)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let after = get(st, caller, tid).await?;
    audit::record(
        &st.pool,
        &AuditEntry::by(caller, "tenant.update")
            .scope(tid, None)
            .subject("tenant", tid)
            .before(&before)
            .after(&after),
    )
    .await?;
    Ok(after)
}

pub async fn list_users(
    st: &AppState,
    caller: &Caller,
    tid: TenantId,
    page: &PageParams,
) -> AppResult<Page<Value>> {
    require_pa_or_ta(caller, tid)?;
    let rows: Vec<(Value, i64)> = sqlx::query_as(
        "SELECT to_jsonb(t) AS item, count(*) OVER () FROM \
           (SELECT id, email::text AS email, full_name, tenant_role, is_active, last_login_at, created_at \
            FROM core.app_users WHERE tenant_id = $1 ORDER BY email) t LIMIT $2 OFFSET $3",
    )
    .bind(tid.as_uuid())
    .bind(page.limit())
    .bind(page.offset())
    .fetch_all(&st.pool)
    .await?;
    Ok(collect_page(rows, page))
}

pub async fn create_user(
    st: &AppState,
    caller: &Caller,
    tid: TenantId,
    input: &NewUserIn,
) -> AppResult<Value> {
    require_pa_or_ta(caller, tid)?;
    let mut errors = Vec::new();
    validate_user(input, &mut errors, "");
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    let hash = hash_secret(&input.password)?;
    let role = input.tenant_role.clone().unwrap_or_else(|| "member".into());
    let (uid,): (Uuid,) = sqlx::query_as(
        "INSERT INTO core.app_users (tenant_id, email, full_name, password_hash, tenant_role) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(tid.as_uuid())
    .bind(input.email.trim())
    .bind(input.full_name.trim())
    .bind(hash)
    .bind(&role)
    .fetch_one(&st.pool)
    .await?;
    let out = json!({ "id": uid, "email": input.email.trim(), "full_name": input.full_name.trim(),
                      "tenant_role": role, "is_active": true });
    audit::record(
        &st.pool,
        &AuditEntry::by(caller, "user.create")
            .scope(tid, None)
            .subject("user", uid)
            .after(&out),
    )
    .await?;
    Ok(out)
}

pub async fn patch_user(
    st: &AppState,
    caller: &Caller,
    tid: TenantId,
    uid: Uuid,
    input: &PatchUserIn,
) -> AppResult<Value> {
    require_pa_or_ta(caller, tid)?;
    if let Some(r) = &input.tenant_role {
        if r != "tenant_admin" && r != "member" {
            return Err(AppError::field("tenant_role", "tenant_admin | member"));
        }
    }
    let hash = match &input.password {
        Some(p) if p.chars().count() < MIN_PASSWORD_LEN => {
            return Err(AppError::field(
                "password",
                format!("at least {MIN_PASSWORD_LEN} characters"),
            ))
        }
        Some(p) => Some(hash_secret(p)?),
        None => None,
    };
    let row: Option<(Value,)> = sqlx::query_as(
        "UPDATE core.app_users SET tenant_role = COALESCE($3, tenant_role), is_active = COALESCE($4, is_active), \
            password_hash = COALESCE($5, password_hash), full_name = COALESCE($6, full_name) \
         WHERE id = $1 AND tenant_id = $2 \
         RETURNING jsonb_build_object('id', id, 'email', email::text, 'full_name', full_name, \
                                      'tenant_role', tenant_role, 'is_active', is_active)",
    )
    .bind(uid)
    .bind(tid.as_uuid())
    .bind(&input.tenant_role)
    .bind(input.is_active)
    .bind(hash)
    .bind(input.full_name.as_deref().map(str::trim))
    .fetch_optional(&st.pool)
    .await?;
    let out = row
        .map(|r| r.0)
        .ok_or_else(|| AppError::not_found("user not found"))?;
    if input.is_active == Some(false) || input.password.is_some() {
        // disabling a user or changing the password ends existing sessions
        sqlx::query(
            "UPDATE core.refresh_tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(uid)
        .execute(&st.pool)
        .await?;
    }
    audit::record(
        &st.pool,
        &AuditEntry::by(caller, "user.update")
            .scope(tid, None)
            .subject("user", uid)
            .after(
                json!({ "tenant_role": input.tenant_role, "is_active": input.is_active,
                           "password_changed": input.password.is_some() }),
            ),
    )
    .await?;
    Ok(out)
}
