//! Use case: evaluate the live rulesets of a project against one event (`POST /v1/projects/{pid}/evaluate`).
//!
//! Orchestration only: load what is live (cached), wire the data provider chain
//! (`CachedProvider` → `PgDataProvider`), run the pure engine, translate its result into the wire contract, and
//! record hits/counters. The scoring rules themselves live in the `rule-engine` crate.

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use contracts::common::{RuleAction, RuleKind, RuleOutcome};
use contracts::scoring::{Actions, EvaluateRequest, EvaluateResponse, Reason, RuleResultTrace, RulesetScore};
use platform::db::TenantTx;
use platform::http::CallCtx;
use platform::{AppError, AppResult, ProjectId, TenantId};
use rule_engine::ports::Timer;
use rule_engine::ruleset::TraceItem;
use rule_engine::{evaluate_rulesets, EngineResult, EvalContext, EvalOptions};
use serde::de::DeserializeOwned;
use serde::Serialize;
use uuid::Uuid;

use crate::adapters::data_provider::PgDataProvider;
use crate::adapters::event_context::engine_data;
use crate::adapters::provider_cache::CachedProvider;
use crate::adapters::repo::{self, CounterDelta, HitRow};
use crate::state::AppState;

/// Timer port implemented with tokio (the engine itself has no runtime dependency).
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioTimer;

#[async_trait]
impl Timer for TokioTimer {
    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

/// Converts between two serde-compatible enums of different crates (engine ⇄ wire contract).
pub fn convert<T: DeserializeOwned>(value: impl Serialize) -> AppResult<T> {
    serde_json::to_value(value)
        .and_then(serde_json::from_value)
        .map_err(|e| AppError::internal(format!("contract conversion: {e}")))
}

fn parse_uuid(s: &str) -> AppResult<Uuid> {
    Uuid::parse_str(s).map_err(|e| AppError::internal(format!("bad uuid {s}: {e}")))
}

pub fn trace_to_contract(item: &TraceItem) -> AppResult<RuleResultTrace> {
    Ok(RuleResultTrace {
        rule_id: parse_uuid(&item.rule_id)?,
        rule_code: item.rule_code.clone(),
        version: item.version,
        ruleset_id: Uuid::parse_str(&item.ruleset_id).ok(),
        ruleset_code: Some(item.ruleset_code.clone()),
        kind: convert::<RuleKind>(&item.kind)?,
        outcome: convert::<RuleOutcome>(&item.outcome)?,
        contribution: item.contribution,
        shadow: item.shadow,
        action: convert::<RuleAction>(item.action)?,
        trapped_reason: item.trapped_reason.clone(),
        trace: item.trace.clone(),
        duration_us: item.duration_us,
    })
}

pub fn to_response(result: &EngineResult) -> AppResult<EvaluateResponse> {
    Ok(EvaluateResponse {
        rules_score: result.rules_score,
        rulesets: result
            .rulesets
            .iter()
            .map(|r| {
                Ok(RulesetScore {
                    ruleset_id: parse_uuid(&r.ruleset_id)?,
                    code: r.code.clone(),
                    score: r.score,
                    shadow: r.shadow,
                })
            })
            .collect::<AppResult<_>>()?,
        rule_results: result
            .rule_results
            .iter()
            .map(trace_to_contract)
            .collect::<AppResult<_>>()?,
        actions: Actions {
            force_decline: result.actions.force_decline,
            force_approve: result.actions.force_approve,
            force_review: result.actions.force_review,
        },
        reasons: result
            .reasons
            .iter()
            .map(|r| Reason {
                code: r.code.clone(),
                engine: r.engine.to_string(),
                contribution: r.contribution,
                message: r.message.clone(),
            })
            .collect(),
        duration_ms: result.duration_us as f64 / 1000.0,
    })
}

/// Hits (match/trapped) and one counter delta per distinct rule.
pub fn hits_and_counters(result: &EngineResult) -> (Vec<HitRow>, Vec<CounterDelta>) {
    let mut hits = Vec::new();
    let mut counters: HashMap<Uuid, CounterDelta> = HashMap::new();
    for item in &result.rule_results {
        let Ok(rule_id) = Uuid::parse_str(&item.rule_id) else {
            continue;
        };
        let matched = item.outcome == "match";
        let trapped = item.outcome == "trapped";
        let c = counters.entry(rule_id).or_insert(CounterDelta {
            rule_id,
            matched: false,
            trapped: false,
        });
        c.matched |= matched;
        c.trapped |= trapped && !matched;
        if matched || trapped {
            hits.push(HitRow {
                rule_id,
                rule_version: item.version,
                ruleset_id: Uuid::parse_str(&item.ruleset_id).ok(),
                outcome: if matched { "match" } else { "trapped" },
                contribution: item.contribution,
                shadow: item.shadow,
            });
        }
    }
    let mut counters: Vec<_> = counters.into_values().collect();
    counters.sort_by_key(|c| c.rule_id);
    (hits, counters)
}

pub async fn evaluate(
    state: &AppState,
    tenant: TenantId,
    project: ProjectId,
    req: &EvaluateRequest,
    call_ctx: CallCtx,
    rule_timeout: Option<Duration>,
) -> AppResult<EvaluateResponse> {
    let serving = state.serving.get(tenant, project).await?;
    let catalog = state.catalogs.get(tenant, project).await?;
    let provider = CachedProvider::new(
        PgDataProvider::new(
            state.pool.clone(),
            tenant,
            project,
            catalog,
            state.graph.clone(),
            call_ctx,
        ),
        project,
        Some(state.ref_cache.clone()),
    );
    let ctx = EvalContext::new(
        engine_data(&req.context, req.customer_id),
        &req.event_type,
        req.occurred_at,
    )
    .with_event_id(req.event_id.to_string())
    .with_customer_id(req.customer_id.to_string());
    let timer = TokioTimer;
    let options = EvalOptions {
        rule_timeout: Some(rule_timeout.unwrap_or(state.settings.rule_timeout)),
        timer: Some(&timer),
    };
    let result = evaluate_rulesets(&serving.units, &ctx, &provider, &options).await;
    metrics::histogram!("rule_evaluate_seconds").record(result.duration_us as f64 / 1e6);
    let response = to_response(&result)?;

    if !req.dry_run {
        let (hits, counters) = hits_and_counters(&result);
        let pool = state.pool.clone();
        let (event_id, occurred_at) = (req.event_id, req.occurred_at);
        let write = async move {
            let mut tx = TenantTx::begin(&pool, tenant).await?;
            repo::record_evaluation(&mut tx, tenant, project, event_id, occurred_at, &hits, &counters)
                .await?;
            tx.commit().await
        };
        if state.settings.eval_async_writes {
            tokio::spawn(async move {
                if let Err(e) = write.await {
                    tracing::error!(error = %e, %event_id, "failed to record rule hits");
                    metrics::counter!("rule_hit_write_errors_total").increment(1);
                }
            });
        } else {
            write.await?;
        }
    }
    Ok(response)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use rule_engine::model::Action;
    use rule_engine::ruleset::{Actions as EngineActions, RulesetScore as EngineScore};
    use serde_json::json;

    fn item(rule: Uuid, outcome: &str, shadow: bool) -> TraceItem {
        TraceItem {
            rule_id: rule.to_string(),
            rule_code: "RL-X".into(),
            version: 1,
            ruleset_id: Uuid::nil().to_string(),
            ruleset_code: "RS-X".into(),
            kind: "velocity".into(),
            outcome: outcome.into(),
            contribution: 10.0,
            shadow,
            action: Action::ForceReview,
            trapped_reason: None,
            trace: json!({}),
            duration_us: 5,
        }
    }

    #[test]
    fn maps_engine_result_to_contract_and_counters() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let result = EngineResult {
            rules_score: 42.0,
            rulesets: vec![EngineScore {
                ruleset_id: Uuid::nil().to_string(),
                code: "RS-X".into(),
                score: 42.0,
                shadow: false,
            }],
            rule_results: vec![
                item(a, "match", false),
                item(a, "match", false),
                item(b, "trapped", true),
                item(b, "no_match", true),
            ],
            actions: EngineActions {
                force_decline: false,
                force_approve: false,
                force_review: true,
            },
            reasons: vec![],
            duration_us: 1500,
        };
        let r = to_response(&result).unwrap();
        assert_eq!(r.rule_results.len(), 4);
        assert_eq!(r.rule_results[0].outcome, RuleOutcome::Match);
        assert_eq!(r.rule_results[0].action, RuleAction::ForceReview);
        assert_eq!(r.rule_results[0].kind, RuleKind::Velocity);
        assert!(r.actions.force_review);
        assert!((r.duration_ms - 1.5).abs() < 1e-9);

        let (hits, counters) = hits_and_counters(&result);
        assert_eq!(hits.len(), 3);
        assert_eq!(counters.len(), 2);
        assert!(counters.iter().any(|c| c.rule_id == a && c.matched && !c.trapped));
        assert!(counters.iter().any(|c| c.rule_id == b && !c.matched && c.trapped));
    }
}
