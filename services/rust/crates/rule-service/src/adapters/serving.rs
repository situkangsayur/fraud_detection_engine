//! Builds the evaluation units (`RulesetUnit`s) that are live for a project, and caches them.
//!
//! Rules **and rulesets** are versioned; what is live is derived from the approval ledger
//! ([`crate::domain::lifecycle::serving_from_ledger`]):
//!
//! * per ruleset: the approved *live* version (scores) and optionally an approved *shadow* version (traced only).
//!   Each is built from its immutable snapshot in `rules.ruleset_versions`, **never** from the editable draft
//!   rows (`rules.rulesets` / `rules.ruleset_rules`), so editing a live ruleset changes nothing until approved;
//! * per member rule: its own live version (+ shadow version), or the pinned version if the rule is served.
//!
//! The result is cached per project for a few seconds and invalidated by this instance on every workflow change.
//! Other instances converge within the TTL (a local cache without a message bus, a deliberate trade-off).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;
use platform::db::TenantTx;
use platform::{AppResult, ProjectId, TenantId};
use rule_engine::model::{Aggregation, RuleEnvelope};
use rule_engine::{RuleUnit, RulesetUnit};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::repo::{self, RuleRow, RulesetConfig, VersionRow};
use crate::domain::lifecycle::{serving_from_ledger, ApprovedVersion, Serving};

/// Everything live for one project.
#[derive(Debug, Clone, Default)]
pub struct ServingSet {
    pub units: Vec<RulesetUnit>,
    /// Number of (rule, version) units across all rulesets (metrics/logging).
    pub rule_count: usize,
}

pub fn parse_aggregation(s: &str) -> Aggregation {
    match s {
        "sum" => Aggregation::Sum,
        "max" => Aggregation::Max,
        "weighted_average" => Aggregation::WeightedAverage,
        _ => Aggregation::ProbabilisticOr,
    }
}

/// Parses a stored rule version into an engine envelope. `None` (logged) if the stored JSON no longer parses.
pub fn parse_envelope(rule: &RuleRow, v: &VersionRow) -> Option<RuleEnvelope> {
    match serde_json::from_value::<RuleEnvelope>(repo::envelope(rule, v)) {
        Ok(e) => Some(e),
        Err(e) => {
            tracing::error!(rule = %rule.code, version = v.version, error = %e, "stored rule version does not parse; skipped");
            None
        }
    }
}

/// Which rule versions a member contributes: `(version, shadow)` pairs.
///
/// * pinned member of a served rule → the pinned version (shadow iff it is the rule's shadow version);
/// * unpinned member → the rule's live version (scores) and shadow version (traced);
/// * retired / never-approved rule → nothing.
pub fn member_versions(pinned: Option<i32>, serving: Serving) -> Vec<(i32, bool)> {
    match pinned {
        Some(v) if serving.is_served() => vec![(v, serving.shadow_version == Some(v))],
        Some(_) => vec![],
        None => serving
            .live_version
            .map(|v| (v, false))
            .into_iter()
            .chain(serving.shadow_version.map(|v| (v, true)))
            .collect(),
    }
}

/// Assembles one evaluation unit from a ruleset snapshot. `resolve(member)` returns the rule units of a member.
pub fn unit_from_snapshot(
    ruleset_id: Uuid,
    code: &str,
    config: &RulesetConfig,
    shadow: bool,
    mut resolve: impl FnMut(&repo::SnapshotMember) -> Vec<RuleUnit>,
) -> RulesetUnit {
    let mut members: Vec<&repo::SnapshotMember> = config.members.iter().collect();
    members.sort_by_key(|m| m.position);
    RulesetUnit {
        ruleset_id: ruleset_id.to_string(),
        code: code.to_string(),
        aggregation: parse_aggregation(&config.aggregation),
        max_score: config.max_score,
        event_types: config.event_types.clone(),
        shadow,
        rules: members.into_iter().flat_map(&mut resolve).collect(),
    }
}

/// Loads the requested `(rule, version)` pairs as parsed engine envelopes.
async fn load_rule_versions(
    conn: &mut PgConnection,
    project: ProjectId,
    wanted: &HashSet<(Uuid, i32)>,
) -> AppResult<HashMap<(Uuid, i32), Arc<RuleEnvelope>>> {
    let (rule_ids, versions): (Vec<Uuid>, Vec<i32>) = wanted.iter().copied().unzip();
    let rules: Vec<RuleRow> = sqlx::query_as(
        "SELECT id, tenant_id, project_id, code, name, description, kind, typologies, event_types, current_version, \
         status, submitted_by, submitted_at, created_by, created_at, updated_at FROM rules.rules \
         WHERE project_id = $1 AND id = ANY($2)",
    )
    .bind(project.as_uuid())
    .bind(&rule_ids)
    .fetch_all(&mut *conn)
    .await?;
    let version_rows: Vec<VersionRow> = sqlx::query_as(
        "SELECT v.rule_id, v.version, v.definition, v.risk_score, v.trapped_score, v.action, v.on_trapped, \
         v.missing_as_no_match, v.change_note, v.created_by, v.created_at \
         FROM rules.rule_versions v JOIN UNNEST($1::uuid[], $2::int4[]) AS n(rid, ver) \
         ON v.rule_id = n.rid AND v.version = n.ver",
    )
    .bind(&rule_ids)
    .bind(&versions)
    .fetch_all(&mut *conn)
    .await?;
    let rules_by_id: HashMap<Uuid, RuleRow> = rules.into_iter().map(|r| (r.id, r)).collect();
    let mut envelopes = HashMap::new();
    for v in &version_rows {
        if let Some(env) = rules_by_id
            .get(&v.rule_id)
            .and_then(|rule| parse_envelope(rule, v))
        {
            envelopes.insert((v.rule_id, v.version), Arc::new(env));
        }
    }
    Ok(envelopes)
}

fn rule_units(
    member: &repo::SnapshotMember,
    pairs: &[(i32, bool)],
    envelopes: &HashMap<(Uuid, i32), Arc<RuleEnvelope>>,
) -> Vec<RuleUnit> {
    pairs
        .iter()
        .filter_map(|&(version, shadow)| {
            envelopes.get(&(member.rule_id, version)).map(|env| RuleUnit {
                rule_id: member.rule_id.to_string(),
                version,
                rule: env.clone(),
                weight: member.weight,
                shadow,
            })
        })
        .collect()
}

/// Loads the serving set of a project inside an existing tenant transaction.
pub async fn load(conn: &mut PgConnection, project: ProjectId) -> AppResult<ServingSet> {
    // 1. Ledger → approved versions per rule and per ruleset.
    let mut rule_ledgers: HashMap<Uuid, Vec<ApprovedVersion>> = HashMap::new();
    let mut ruleset_ledgers: HashMap<Uuid, Vec<ApprovedVersion>> = HashMap::new();
    for (subject_type, id, version, target) in repo::approved_ledger(&mut *conn, project).await? {
        let Some(version) = version else { continue };
        let map = if subject_type == "rule" {
            &mut rule_ledgers
        } else {
            &mut ruleset_ledgers
        };
        map.entry(id)
            .or_default()
            .push(ApprovedVersion { version, target });
    }

    // 2. Served (ruleset, version, shadow) triples of non-retired rulesets.
    let rulesets: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, code FROM rules.rulesets WHERE project_id = $1 AND status <> 'retired' ORDER BY code",
    )
    .bind(project.as_uuid())
    .fetch_all(&mut *conn)
    .await?;
    let mut served: Vec<(Uuid, String, i32, bool)> = Vec::new();
    for (id, code) in rulesets {
        let s = serving_from_ledger(ruleset_ledgers.get(&id).map(Vec::as_slice).unwrap_or(&[]), false);
        if let Some(v) = s.live_version {
            served.push((id, code.clone(), v, false));
        }
        if let Some(v) = s.shadow_version {
            served.push((id, code, v, true));
        }
    }
    if served.is_empty() {
        return Ok(ServingSet::default());
    }

    // 3. Snapshots of exactly those versions.
    let ids: Vec<Uuid> = served.iter().map(|s| s.0).collect();
    let versions: Vec<i32> = served.iter().map(|s| s.2).collect();
    let rows: Vec<repo::RulesetVersionRow> = sqlx::query_as(
        "SELECT v.ruleset_id, v.version, v.config, v.change_note, v.created_by, v.created_at \
         FROM rules.ruleset_versions v JOIN UNNEST($1::uuid[], $2::int4[]) AS n(rid, ver) \
         ON v.ruleset_id = n.rid AND v.version = n.ver",
    )
    .bind(&ids)
    .bind(&versions)
    .fetch_all(&mut *conn)
    .await?;
    let mut configs: HashMap<(Uuid, i32), RulesetConfig> = HashMap::new();
    for row in rows {
        match row.parsed() {
            Ok(cfg) => {
                configs.insert((row.ruleset_id, row.version), cfg);
            }
            Err(e) => tracing::error!(error = %e, "ruleset snapshot skipped"),
        }
    }

    // 4. Member rules: their own serving state decides which rule versions run.
    let member_ids: Vec<Uuid> = configs
        .values()
        .flat_map(|c| c.members.iter().map(|m| m.rule_id))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let statuses: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, status FROM rules.rules WHERE project_id = $1 AND id = ANY($2)")
            .bind(project.as_uuid())
            .bind(&member_ids)
            .fetch_all(&mut *conn)
            .await?;
    let rule_serving: HashMap<Uuid, Serving> = statuses
        .into_iter()
        .map(|(id, status)| {
            let ledger = rule_ledgers.get(&id).map(Vec::as_slice).unwrap_or(&[]);
            (id, serving_from_ledger(ledger, status == "retired"))
        })
        .collect();
    let pairs_of = |m: &repo::SnapshotMember| {
        member_versions(
            m.pinned_version,
            rule_serving.get(&m.rule_id).copied().unwrap_or_default(),
        )
    };
    let wanted: HashSet<(Uuid, i32)> = configs
        .values()
        .flat_map(|c| c.members.iter())
        .flat_map(|m| pairs_of(m).into_iter().map(move |(v, _)| (m.rule_id, v)))
        .collect();
    let envelopes = load_rule_versions(&mut *conn, project, &wanted).await?;

    // 5. Assemble.
    let mut units = Vec::new();
    for (id, code, version, shadow) in served {
        let Some(cfg) = configs.get(&(id, version)) else {
            continue;
        };
        units.push(unit_from_snapshot(id, &code, cfg, shadow, |m| {
            rule_units(m, &pairs_of(m), &envelopes)
        }));
    }
    let rule_count = units.iter().map(|u| u.rules.len()).sum();
    Ok(ServingSet { units, rule_count })
}

/// Evaluation unit of one ruleset version **as a candidate** (backtests): members use their pinned version or
/// the rule's current (latest) version, whether approved or not, so a draft can be measured before approval.
pub async fn candidate_unit(
    conn: &mut PgConnection,
    project: ProjectId,
    ruleset_id: Uuid,
    code: &str,
    version: i32,
) -> AppResult<RulesetUnit> {
    let config = repo::ruleset_version(&mut *conn, ruleset_id, version)
        .await?
        .parsed()?;
    let ids: Vec<Uuid> = config.members.iter().map(|m| m.rule_id).collect();
    let current: Vec<(Uuid, i32, String)> = sqlx::query_as(
        "SELECT id, current_version, status FROM rules.rules WHERE project_id = $1 AND id = ANY($2)",
    )
    .bind(project.as_uuid())
    .bind(&ids)
    .fetch_all(&mut *conn)
    .await?;
    let chosen: HashMap<Uuid, i32> = current
        .into_iter()
        .filter(|(_, _, status)| status != "retired")
        .map(|(id, v, _)| (id, v))
        .collect();
    let wanted: HashSet<(Uuid, i32)> = config
        .members
        .iter()
        .filter_map(|m| {
            chosen
                .get(&m.rule_id)
                .map(|cur| (m.rule_id, m.pinned_version.unwrap_or(*cur)))
        })
        .collect();
    let envelopes = load_rule_versions(&mut *conn, project, &wanted).await?;
    Ok(unit_from_snapshot(
        ruleset_id,
        code,
        &config,
        false,
        |m| match chosen.get(&m.rule_id) {
            Some(cur) => rule_units(m, &[(m.pinned_version.unwrap_or(*cur), false)], &envelopes),
            None => vec![],
        },
    ))
}

/// Per-project cache of serving sets.
#[derive(Clone)]
pub struct ServingCache {
    pool: PgPool,
    cache: Cache<ProjectId, Arc<ServingSet>>,
}

impl std::fmt::Debug for ServingCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServingCache").finish_non_exhaustive()
    }
}

impl ServingCache {
    pub fn new(pool: PgPool, ttl: Duration) -> Self {
        Self {
            pool,
            cache: Cache::builder().max_capacity(10_000).time_to_live(ttl).build(),
        }
    }

    pub async fn get(&self, tenant: TenantId, project: ProjectId) -> AppResult<Arc<ServingSet>> {
        if let Some(hit) = self.cache.get(&project).await {
            return Ok(hit);
        }
        let mut tx = TenantTx::begin(&self.pool, tenant).await?;
        let set = Arc::new(load(&mut tx, project).await?);
        tx.commit().await?;
        self.cache.insert(project, set.clone()).await;
        Ok(set)
    }

    pub async fn invalidate(&self, project: ProjectId) {
        self.cache.invalidate(&project).await;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use repo::SnapshotMember;

    #[test]
    fn member_versions_rules() {
        let live = Serving {
            live_version: Some(2),
            shadow_version: Some(3),
        };
        assert_eq!(member_versions(None, live), vec![(2, false), (3, true)]);
        assert_eq!(member_versions(Some(1), live), vec![(1, false)]);
        assert_eq!(member_versions(Some(3), live), vec![(3, true)]);
        assert!(member_versions(Some(1), Serving::default()).is_empty());
        assert!(member_versions(None, Serving::default()).is_empty());
    }

    #[test]
    fn unit_orders_members_by_position_and_uses_snapshot_config() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let cfg = RulesetConfig {
            name: "x".into(),
            description: None,
            event_types: vec!["transaction".into()],
            typologies: vec![],
            aggregation: "sum".into(),
            max_score: 70.0,
            members: vec![
                SnapshotMember {
                    rule_id: b,
                    weight: 2.0,
                    pinned_version: None,
                    position: 1,
                },
                SnapshotMember {
                    rule_id: a,
                    weight: 1.0,
                    pinned_version: None,
                    position: 0,
                },
            ],
        };
        let env: Arc<RuleEnvelope> = Arc::new(
            serde_json::from_value(serde_json::json!({
                "code": "RL-X", "name": "x", "kind": "simple", "risk_score": 10,
                "definition": {"kind": "simple", "when": {"left": {"type": "field", "path": "event.amount"},
                               "op": "gt", "right": {"type": "const", "value": 1}}}
            }))
            .unwrap(),
        );
        let unit = unit_from_snapshot(Uuid::nil(), "RS-X", &cfg, true, |m| {
            vec![RuleUnit {
                rule_id: m.rule_id.to_string(),
                version: 1,
                rule: env.clone(),
                weight: m.weight,
                shadow: false,
            }]
        });
        assert_eq!(unit.rules[0].rule_id, a.to_string());
        assert_eq!(unit.rules[1].weight, 2.0);
        assert_eq!(unit.aggregation, Aggregation::Sum);
        assert_eq!(unit.max_score, 70.0);
        assert!(unit.shadow);
    }
}
