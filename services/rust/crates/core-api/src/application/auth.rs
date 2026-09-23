//! Authentication use cases: login, refresh-token rotation, logout, `/me`.
//!
//! * Access tokens are short-lived JWTs (default 60 min) carrying the project-role map (`prj`),
//!   so every service authorises locally without calling core-api.
//! * Refresh tokens are opaque random strings stored as SHA-256. Every refresh **rotates** the token
//!   (old one revoked, new one in the same `family_id`). Presenting an already-revoked token means
//!   it was stolen and replayed, so the whole family is revoked (OAuth 2.0 BCP "refresh token
//!   rotation with reuse detection").

use std::collections::HashMap;

use chrono::{Duration as ChronoDuration, Utc};
use platform::auth::{Claims, ProjectRole, TenantRole};
use platform::db::TenantTx;
use platform::error::{AppError, AppResult};
use platform::TenantId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::adapters::crypto::{dummy_verify, random_token, sha256_hex, verify_secret};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct UserOut {
    pub id: Uuid,
    pub email: String,
    pub full_name: String,
    pub tenant_id: Option<Uuid>,
    pub tenant_role: String,
    pub is_platform_admin: bool,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct TokenPair {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_in: u64,
    pub refresh_token: String,
    pub user: UserOut,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct LoginIn {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone)]
struct UserRow {
    id: Uuid,
    tenant_id: Option<Uuid>,
    email: String,
    full_name: String,
    password_hash: String,
    tenant_role: String,
    is_platform_admin: bool,
    is_active: bool,
    tenant_active: bool,
}

const USER_SQL: &str =
    "SELECT u.id, u.tenant_id, u.email::text AS email, u.full_name, u.password_hash, u.tenant_role, \
       u.is_platform_admin, u.is_active, COALESCE(t.status = 'active', true) AS tenant_active \
     FROM core.app_users u LEFT JOIN core.tenants t ON t.id = u.tenant_id";

fn user_from_row(r: &sqlx::postgres::PgRow) -> UserRow {
    UserRow {
        id: r.get("id"),
        tenant_id: r.get("tenant_id"),
        email: r.get("email"),
        full_name: r.get("full_name"),
        password_hash: r.get("password_hash"),
        tenant_role: r.get("tenant_role"),
        is_platform_admin: r.get("is_platform_admin"),
        is_active: r.get("is_active"),
        tenant_active: r.get("tenant_active"),
    }
}

fn user_out(u: &UserRow) -> UserOut {
    UserOut {
        id: u.id,
        email: u.email.clone(),
        full_name: u.full_name.clone(),
        tenant_id: u.tenant_id,
        tenant_role: u.tenant_role.clone(),
        is_platform_admin: u.is_platform_admin,
    }
}

fn unauthorized() -> AppError {
    AppError::Unauthorized("invalid email or password".into())
}

pub async fn login(st: &AppState, input: &LoginIn, user_agent: Option<String>) -> AppResult<TokenPair> {
    if input.email.len() > 320 || input.password.len() > 1024 {
        return Err(unauthorized());
    }
    let row = sqlx::query(&format!("{USER_SQL} WHERE u.email = $1::citext"))
        .bind(input.email.trim())
        .fetch_optional(&st.pool)
        .await?;
    let Some(row) = row else {
        dummy_verify(&input.password);
        return Err(unauthorized());
    };
    let user = user_from_row(&row);
    if !verify_secret(&input.password, &user.password_hash) || !user.is_active || !user.tenant_active {
        return Err(unauthorized());
    }
    sqlx::query("UPDATE core.app_users SET last_login_at = now() WHERE id = $1")
        .bind(user.id)
        .execute(&st.pool)
        .await?;
    let pair = issue_pair(st, &user, Uuid::new_v4(), user_agent).await?;
    platform::audit::record(
        &st.pool,
        &platform::audit::AuditEntry {
            tenant_id: user.tenant_id.map(TenantId),
            actor_type: "user",
            actor_id: Some(user.id.to_string()),
            action: "auth.login".into(),
            subject_type: Some("user".into()),
            subject_id: Some(user.id.to_string()),
            metadata: json!({}),
            ..Default::default()
        },
    )
    .await?;
    Ok(pair)
}

/// Project roles for the JWT `prj` claim.
async fn project_roles(st: &AppState, user: &UserRow) -> AppResult<HashMap<Uuid, ProjectRole>> {
    let Some(tid) = user.tenant_id else {
        return Ok(HashMap::new());
    };
    let mut tx = TenantTx::begin(&st.pool, TenantId(tid)).await?;
    let rows: Vec<(Uuid, String)> = if user.tenant_role == "tenant_admin" {
        sqlx::query_as("SELECT id, 'project_admin'::text FROM core.projects WHERE status = 'active'")
            .fetch_all(&mut **tx)
            .await?
    } else {
        sqlx::query_as(
            "SELECT m.project_id, m.role FROM core.project_members m \
             JOIN core.projects p ON p.id = m.project_id AND p.status = 'active' WHERE m.user_id = $1",
        )
        .bind(user.id)
        .fetch_all(&mut **tx)
        .await?
    };
    tx.commit().await?;
    Ok(rows
        .into_iter()
        .filter_map(|(p, r)| ProjectRole::parse(&r).map(|r| (p, r)))
        .collect())
}

async fn issue_pair(
    st: &AppState,
    user: &UserRow,
    family: Uuid,
    user_agent: Option<String>,
) -> AppResult<TokenPair> {
    let prj = project_roles(st, user).await?;
    let trole = if user.tenant_role == "tenant_admin" {
        TenantRole::TenantAdmin
    } else {
        TenantRole::Member
    };
    let claims = Claims::new(
        user.id,
        user.tenant_id,
        trole,
        user.is_platform_admin,
        prj,
        st.cfg.jwt_ttl,
    );
    let access = st.auth.jwt.issue(&claims)?;
    let refresh = random_token(32);
    sqlx::query(
        "INSERT INTO core.refresh_tokens (user_id, token_hash, family_id, expires_at, user_agent) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(user.id)
    .bind(sha256_hex(&refresh))
    .bind(family)
    .bind(Utc::now() + ChronoDuration::days(st.cfg.refresh_ttl_days))
    .bind(user_agent.map(|u| u.chars().take(300).collect::<String>()))
    .execute(&st.pool)
    .await?;
    Ok(TokenPair {
        access_token: access,
        token_type: "Bearer",
        expires_in: st.cfg.jwt_ttl.as_secs(),
        refresh_token: refresh,
        user: user_out(user),
    })
}

/// Rotates a refresh token. Reuse of a revoked token revokes the whole family.
pub async fn refresh(st: &AppState, token: &str, user_agent: Option<String>) -> AppResult<TokenPair> {
    let hash = sha256_hex(token);
    let mut tx = st.pool.begin().await?;
    let row = sqlx::query(
        "SELECT id, user_id, family_id, expires_at, revoked_at FROM core.refresh_tokens \
         WHERE token_hash = $1 FOR UPDATE",
    )
    .bind(&hash)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(AppError::Unauthorized("invalid refresh token".into()));
    };
    let id: Uuid = row.get("id");
    let user_id: Uuid = row.get("user_id");
    let family: Uuid = row.get("family_id");
    let expires: chrono::DateTime<Utc> = row.get("expires_at");
    let revoked: Option<chrono::DateTime<Utc>> = row.get("revoked_at");
    if revoked.is_some() {
        sqlx::query(
            "UPDATE core.refresh_tokens SET revoked_at = now() WHERE family_id = $1 AND revoked_at IS NULL",
        )
        .bind(family)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        tracing::warn!(user_id = %user_id, "refresh token reuse detected; family revoked");
        platform::audit::record(
            &st.pool,
            &platform::audit::AuditEntry {
                actor_type: "system",
                action: "auth.refresh_reuse_detected".into(),
                subject_type: Some("user".into()),
                subject_id: Some(user_id.to_string()),
                metadata: json!({ "family_id": family }),
                ..Default::default()
            },
        )
        .await?;
        return Err(AppError::Unauthorized("refresh token reuse detected".into()));
    }
    if expires < Utc::now() {
        return Err(AppError::Unauthorized("refresh token expired".into()));
    }
    sqlx::query("UPDATE core.refresh_tokens SET revoked_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let user_row = sqlx::query(&format!("{USER_SQL} WHERE u.id = $1"))
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".into()))?;
    tx.commit().await?;
    let user = user_from_row(&user_row);
    if !user.is_active || !user.tenant_active {
        return Err(AppError::Unauthorized("user is disabled".into()));
    }
    issue_pair(st, &user, family, user_agent).await
}

/// Revokes the family of `token` (if given and owned by the user) or all of the user's tokens.
pub async fn logout(st: &AppState, user_id: Uuid, token: Option<&str>) -> AppResult<()> {
    match token {
        Some(t) => {
            sqlx::query(
                "UPDATE core.refresh_tokens SET revoked_at = now() WHERE revoked_at IS NULL AND user_id = $1 \
                 AND family_id = (SELECT family_id FROM core.refresh_tokens WHERE token_hash = $2 AND user_id = $1)",
            )
            .bind(user_id)
            .bind(sha256_hex(t))
            .execute(&st.pool)
            .await?;
        }
        None => {
            sqlx::query(
                "UPDATE core.refresh_tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
            )
            .bind(user_id)
            .execute(&st.pool)
            .await?;
        }
    }
    Ok(())
}

/// `/me`: user, tenant and visible projects with roles (fresh from the DB, not from the token).
pub async fn me(st: &AppState, user_id: Uuid) -> AppResult<Value> {
    let row = sqlx::query(&format!("{USER_SQL} WHERE u.id = $1"))
        .bind(user_id)
        .fetch_optional(&st.pool)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".into()))?;
    let user = user_from_row(&row);
    let (tenant, projects) = match user.tenant_id {
        None => (Value::Null, Vec::new()),
        Some(tid) => {
            let t: Option<(Uuid, String, String, String)> =
                sqlx::query_as("SELECT id, slug, name, status FROM core.tenants WHERE id = $1")
                    .bind(tid)
                    .fetch_optional(&st.pool)
                    .await?;
            let roles = project_roles(st, &user).await?;
            let mut tx = TenantTx::begin(&st.pool, TenantId(tid)).await?;
            let rows: Vec<(Uuid, String, String, String, String)> = sqlx::query_as(
                "SELECT id, slug, name, stage, status FROM core.projects WHERE id = ANY($1) ORDER BY name",
            )
            .bind(roles.keys().copied().collect::<Vec<_>>())
            .fetch_all(&mut **tx)
            .await?;
            tx.commit().await?;
            let projects = rows
                .into_iter()
                .map(|(id, slug, name, stage, status)| {
                    json!({ "id": id, "slug": slug, "name": name, "stage": stage, "status": status,
                            "role": roles.get(&id).map(ProjectRole::as_str) })
                })
                .collect();
            (
                t.map(|(id, slug, name, status)| json!({ "id": id, "slug": slug, "name": name, "status": status }))
                    .unwrap_or(Value::Null),
                projects,
            )
        }
    };
    Ok(json!({ "user": user_out(&user), "tenant": tenant, "projects": projects }))
}
