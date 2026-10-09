//! Startup bootstrap (idempotent): platform admin, optional demo tenant, pending template jobs.

use platform::config::Secret;
use platform::db::TenantTx;
use platform::{ProjectId, TenantId};
use uuid::Uuid;

use crate::adapters::crypto::hash_secret;
use crate::application::projects::{init_project, resume_pending_bootstraps};
use crate::application::tenants::MIN_PASSWORD_LEN;
use crate::config::{non_empty, ServeConfig};
use crate::state::AppState;

/// Demo projects created when `SEED_DEMO=true` (multi-tenancy.md §5).
pub const DEMO_PROJECTS: &[(&str, &str, &str)] = &[
    ("checkout", "Checkout (pre-payment)", "pre_payment"),
    ("post-payment", "Post-payment", "post_payment"),
    ("returns", "Returns & refunds", "returns"),
    ("promo", "Promo & cashback", "promo"),
];

pub async fn run(st: &AppState, cfg: &ServeConfig) -> anyhow::Result<()> {
    if let Some(pw) = non_empty(&cfg.admin_password) {
        ensure_platform_admin(st, &cfg.admin_email, pw).await?;
    } else {
        tracing::warn!("ADMIN_PASSWORD not set: no platform admin is bootstrapped");
    }
    if cfg.seed_demo {
        if let Err(e) = seed_demo(st, non_empty(&cfg.demo_user_password)).await {
            tracing::error!(error = %e, "demo seed failed");
        }
    }
    if let Err(e) = resume_pending_bootstraps(st).await {
        tracing::warn!(error = %e, "could not resume pending template bootstraps");
    }
    Ok(())
}

pub async fn ensure_platform_admin(st: &AppState, email: &str, password: &Secret) -> anyhow::Result<()> {
    let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM core.app_users WHERE email = $1::citext")
        .bind(email)
        .fetch_optional(&st.pool)
        .await?;
    if exists.is_some() {
        return Ok(());
    }
    if password.expose().chars().count() < MIN_PASSWORD_LEN {
        anyhow::bail!("ADMIN_PASSWORD must have at least {MIN_PASSWORD_LEN} characters");
    }
    let hash = hash_secret(password.expose()).map_err(|e| anyhow::anyhow!("{e}"))?;
    sqlx::query(
        "INSERT INTO core.app_users (tenant_id, email, full_name, password_hash, tenant_role, is_platform_admin) \
         VALUES (NULL, $1, 'Platform Admin', $2, 'member', true) ON CONFLICT (email) DO NOTHING",
    )
    .bind(email)
    .bind(hash)
    .execute(&st.pool)
    .await?;
    tracing::info!(%email, "platform admin created");
    Ok(())
}

async fn upsert_user(
    st: &AppState,
    tenant: Uuid,
    email: &str,
    name: &str,
    pw: &Secret,
) -> anyhow::Result<Uuid> {
    let hash = hash_secret(pw.expose()).map_err(|e| anyhow::anyhow!("{e}"))?;
    sqlx::query(
        "INSERT INTO core.app_users (tenant_id, email, full_name, password_hash, tenant_role) \
         VALUES ($1, $2, $3, $4, 'member') ON CONFLICT (email) DO NOTHING",
    )
    .bind(tenant)
    .bind(email)
    .bind(name)
    .bind(hash)
    .execute(&st.pool)
    .await?;
    let (id,): (Uuid,) = sqlx::query_as("SELECT id FROM core.app_users WHERE email = $1::citext")
        .bind(email)
        .fetch_one(&st.pool)
        .await?;
    Ok(id)
}

pub async fn seed_demo(st: &AppState, demo_password: Option<&Secret>) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO core.tenants (slug, name) VALUES ('demo', 'Demo Marketplace') ON CONFLICT (slug) DO NOTHING")
        .execute(&st.pool)
        .await?;
    let (tid,): (Uuid,) = sqlx::query_as("SELECT id FROM core.tenants WHERE slug = 'demo'")
        .fetch_one(&st.pool)
        .await?;
    let tenant = TenantId(tid);

    let mut members: Vec<(Uuid, &str)> = Vec::new();
    match demo_password {
        Some(pw) if pw.expose().chars().count() >= MIN_PASSWORD_LEN => {
            members.push((
                upsert_user(st, tid, "analyst@demo.local", "Demo Analyst", pw).await?,
                "analyst",
            ));
            members.push((
                upsert_user(st, tid, "approver@demo.local", "Demo Approver", pw).await?,
                "approver",
            ));
        }
        Some(_) => {
            tracing::warn!("DEMO_USER_PASSWORD shorter than {MIN_PASSWORD_LEN} chars: demo users skipped")
        }
        None => tracing::info!("DEMO_USER_PASSWORD empty: demo users skipped"),
    }

    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    for (slug, name, stage) in DEMO_PROJECTS {
        let created: Option<(Uuid,)> = sqlx::query_as(
            "INSERT INTO core.projects (tenant_id, slug, name, stage, description) VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (tenant_id, slug) DO NOTHING RETURNING id",
        )
        .bind(tid)
        .bind(slug)
        .bind(name)
        .bind(stage)
        .bind(format!("Demo project ({stage})"))
        .fetch_optional(&mut **tx)
        .await?;
        let pid = match created {
            Some((id,)) => {
                init_project(&mut tx, tenant, ProjectId(id), None, Some(stage)).await?;
                id
            }
            None => {
                let (id,): (Uuid,) =
                    sqlx::query_as("SELECT id FROM core.projects WHERE tenant_id = $1 AND slug = $2")
                        .bind(tid)
                        .bind(slug)
                        .fetch_one(&mut **tx)
                        .await?;
                id
            }
        };
        for (uid, role) in &members {
            sqlx::query(
                "INSERT INTO core.project_members (tenant_id, project_id, user_id, role) VALUES ($1,$2,$3,$4) \
                 ON CONFLICT (project_id, user_id) DO NOTHING",
            )
            .bind(tid)
            .bind(pid)
            .bind(uid)
            .bind(role)
            .execute(&mut **tx)
            .await?;
        }
    }
    tx.commit().await?;
    tracing::info!("demo tenant seeded");
    Ok(())
}
