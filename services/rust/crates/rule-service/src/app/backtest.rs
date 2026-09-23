//! Use case: replay historical events through a rule or a ruleset and measure it against labels.
//!
//! Point-in-time correctness: every replayed event is evaluated with **its own** `occurred_at` as the anchor, so
//! velocity windows only see history that existed at that moment (plus later-arriving late events, a known
//! limitation). Nothing is persisted; the engine runs with the same provider chain as live evaluation.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use platform::db::TenantTx;
use platform::http::CallCtx;
use platform::{AppError, AppResult, ProjectId, TenantId};
use rule_engine::eval::Outcome;
use rule_engine::model::RuleEnvelope;
use rule_engine::{evaluate_rule, evaluate_rulesets, EvalContext, EvalOptions, RulesetUnit};
use serde::Deserialize;

use crate::adapters::data_provider::PgDataProvider;
use crate::adapters::event_context::{engine_data, load_window, StoredEvent};
use crate::adapters::provider_cache::{new_ref_cache, CachedProvider, RefCache};
use crate::app::evaluation::TokioTimer;
use crate::domain::backtest::{decide, summarize, BacktestReport, Observation, Thresholds};
use crate::state::AppState;

pub const DEFAULT_LIMIT: i64 = 5_000;
pub const MAX_LIMIT: i64 = 50_000;
pub const DEFAULT_SINCE_DAYS: i64 = 30;
/// Backtests allow slower rules than live scoring.
const BACKTEST_RULE_TIMEOUT: Duration = Duration::from_secs(2);

/// Window & size of a backtest (`from`/`to` or `since_days`).
#[derive(Debug, Clone, Default, Deserialize, utoipa::ToSchema)]
pub struct BacktestWindow {
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub since_days: Option<i64>,
    pub limit: Option<i64>,
}

impl BacktestWindow {
    pub fn resolve(&self, now: DateTime<Utc>) -> AppResult<(DateTime<Utc>, DateTime<Utc>, i64)> {
        let to = self.to.unwrap_or(now);
        let from = match (self.from, self.since_days) {
            (Some(f), _) => f,
            (None, Some(d)) if (1..=3650).contains(&d) => to - chrono::Duration::days(d),
            (None, Some(_)) => return Err(AppError::field("since_days", "must be between 1 and 3650")),
            (None, None) => to - chrono::Duration::days(DEFAULT_SINCE_DAYS),
        };
        if from >= to {
            return Err(AppError::field("from", "must be before `to`"));
        }
        let limit = self.limit.unwrap_or(DEFAULT_LIMIT);
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(AppError::field(
                "limit",
                format!("must be between 1 and {MAX_LIMIT}"),
            ));
        }
        Ok((from, to, limit))
    }
}

async fn load(
    state: &AppState,
    tenant: TenantId,
    project: ProjectId,
    window: &BacktestWindow,
    event_types: &[String],
) -> AppResult<(Vec<StoredEvent>, bool)> {
    let (from, to, limit) = window.resolve(Utc::now())?;
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let mut events = load_window(&mut tx, project, from, to, event_types, limit).await?;
    tx.commit().await?;
    let truncated = events.len() as i64 > limit;
    events.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    Ok((events, truncated))
}

fn provider(
    state: &AppState,
    tenant: TenantId,
    project: ProjectId,
    catalog: Arc<crate::adapters::catalog::ProjectCatalog>,
    refs: &RefCache,
    ctx: &CallCtx,
) -> CachedProvider<PgDataProvider> {
    CachedProvider::new(
        PgDataProvider::new(
            state.pool.clone(),
            tenant,
            project,
            catalog,
            state.graph.clone(),
            ctx.clone(),
        ),
        project,
        Some(refs.clone()),
    )
}

fn engine_ctx(ev: &StoredEvent) -> EvalContext {
    EvalContext::new(
        engine_data(&ev.context(), ev.customer_id),
        &ev.event_type,
        ev.occurred_at,
    )
    .with_event_id(ev.id.to_string())
    .with_customer_id(ev.customer_id.to_string())
}

/// Backtests one rule envelope (stored version or inline draft).
pub async fn backtest_rule(
    state: &AppState,
    tenant: TenantId,
    project: ProjectId,
    rule: Arc<RuleEnvelope>,
    window: &BacktestWindow,
    call_ctx: &CallCtx,
) -> AppResult<BacktestReport> {
    let (events, truncated) = load(state, tenant, project, window, &rule.event_types).await?;
    let catalog = state.catalogs.get(tenant, project).await?;
    let refs = new_ref_cache(Duration::from_secs(300));
    let observations: Vec<Observation> = stream::iter(events)
        .map(|ev| {
            let rule = rule.clone();
            let provider = provider(state, tenant, project, catalog.clone(), &refs, call_ctx);
            async move {
                let ctx = engine_ctx(&ev);
                let evaluation =
                    tokio::time::timeout(BACKTEST_RULE_TIMEOUT, evaluate_rule(&rule, &ctx, &provider))
                        .await
                        .map(|e| e.outcome)
                        .unwrap_or_else(|_| Outcome::Trapped("timeout".into()));
                Observation {
                    event_id: ev.id,
                    occurred_at: ev.occurred_at,
                    matched: evaluation == Outcome::Match,
                    trapped: matches!(evaluation, Outcome::Trapped(_)),
                    label: ev.label(),
                    score: None,
                    decision: None,
                }
            }
        })
        .buffer_unordered(state.settings.backtest_concurrency)
        .collect()
        .await;
    Ok(summarize(&observations, truncated))
}

/// Backtests a ruleset as currently configured (the unit is treated as live).
pub async fn backtest_ruleset(
    state: &AppState,
    tenant: TenantId,
    project: ProjectId,
    unit: RulesetUnit,
    window: &BacktestWindow,
    thresholds: Thresholds,
    call_ctx: &CallCtx,
) -> AppResult<BacktestReport> {
    let (events, truncated) = load(state, tenant, project, window, &unit.event_types).await?;
    let catalog = state.catalogs.get(tenant, project).await?;
    let refs = new_ref_cache(Duration::from_secs(300));
    let units = Arc::new(vec![unit]);
    let observations: Vec<Observation> = stream::iter(events)
        .map(|ev| {
            let units = units.clone();
            let provider = provider(state, tenant, project, catalog.clone(), &refs, call_ctx);
            async move {
                let ctx = engine_ctx(&ev);
                let timer = TokioTimer;
                let options = EvalOptions {
                    rule_timeout: Some(BACKTEST_RULE_TIMEOUT),
                    timer: Some(&timer),
                };
                let result = evaluate_rulesets(&units, &ctx, &provider, &options).await;
                let action = result.actions.effective();
                let trapped = result.rule_results.iter().any(|t| t.outcome == "trapped");
                Observation {
                    event_id: ev.id,
                    occurred_at: ev.occurred_at,
                    matched: result.rules_score > 0.0 || action.is_some(),
                    trapped,
                    label: ev.label(),
                    score: Some(result.rules_score),
                    decision: Some(decide(result.rules_score, action, thresholds)),
                }
            }
        })
        .buffer_unordered(state.settings.backtest_concurrency)
        .collect()
        .await;
    Ok(summarize(&observations, truncated))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn window_resolution() {
        let now = Utc.with_ymd_and_hms(2026, 9, 30, 0, 0, 0).unwrap();
        let (from, to, limit) = BacktestWindow::default().resolve(now).unwrap();
        assert_eq!(to, now);
        assert_eq!(from, now - chrono::Duration::days(30));
        assert_eq!(limit, DEFAULT_LIMIT);
        let w = BacktestWindow {
            since_days: Some(7),
            limit: Some(10),
            ..Default::default()
        };
        let (from, _, limit) = w.resolve(now).unwrap();
        assert_eq!(from, now - chrono::Duration::days(7));
        assert_eq!(limit, 10);
        assert!(BacktestWindow {
            limit: Some(MAX_LIMIT + 1),
            ..Default::default()
        }
        .resolve(now)
        .is_err());
        assert!(BacktestWindow {
            since_days: Some(0),
            ..Default::default()
        }
        .resolve(now)
        .is_err());
        assert!(BacktestWindow {
            from: Some(now),
            to: Some(now),
            ..Default::default()
        }
        .resolve(now)
        .is_err());
    }
}
