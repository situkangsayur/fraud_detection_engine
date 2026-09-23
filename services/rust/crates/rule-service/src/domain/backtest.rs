//! Backtest statistics (pure). The adapter replays historical events through the rule engine; this module turns
//! the per-event outcomes into hit rate, precision/recall against analyst labels, daily series and histograms.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use uuid::Uuid;

/// Label of a historical event (latest label wins, `core.event_labels`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    Fraud,
    Legit,
}

impl Label {
    pub fn parse(s: Option<&str>) -> Option<Label> {
        match s {
            Some("fraud") => Some(Label::Fraud),
            Some("legit") => Some(Label::Legit),
            _ => None,
        }
    }
}

/// One replayed event.
#[derive(Debug, Clone)]
pub struct Observation {
    pub event_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub matched: bool,
    pub trapped: bool,
    pub label: Option<Label>,
    /// Ruleset score (ruleset backtests only).
    pub score: Option<f64>,
    /// Decision derived from score + actions (ruleset backtests only).
    pub decision: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize, PartialEq, utoipa::ToSchema)]
pub struct DayPoint {
    pub date: NaiveDate,
    pub evaluated: u64,
    pub matched: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, utoipa::ToSchema)]
pub struct HistogramBucket {
    /// Lower bound of the bucket (0, 10, …, 90); the last bucket includes 100.
    pub bucket: u32,
    pub count: u64,
}

/// Response body of rule / ruleset backtests (api-contract §B).
#[derive(Debug, Clone, Serialize, PartialEq, Default, utoipa::ToSchema)]
pub struct BacktestReport {
    pub evaluated: u64,
    pub matched: u64,
    pub trapped: u64,
    pub hit_rate: f64,
    pub labeled_fraud_matched: u64,
    pub labeled_legit_matched: u64,
    pub labeled_fraud_total: u64,
    /// `fraud_matched / (fraud_matched + legit_matched)`; `None` when no labelled event matched.
    pub precision: Option<f64>,
    /// `fraud_matched / labelled fraud events`; `None` when no labelled fraud event was evaluated.
    pub recall: Option<f64>,
    pub sample_matches: Vec<Uuid>,
    pub by_day: Vec<DayPoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_histogram: Option<Vec<HistogramBucket>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_distribution: Option<BTreeMap<String, u64>>,
    /// True when `limit` truncated the event window (most recent events were replayed).
    pub truncated: bool,
}

const SAMPLE_LIMIT: usize = 20;

pub fn summarize(observations: &[Observation], truncated: bool) -> BacktestReport {
    let mut r = BacktestReport {
        truncated,
        ..Default::default()
    };
    let mut days: BTreeMap<NaiveDate, (u64, u64)> = BTreeMap::new();
    let mut histogram = [0u64; 10];
    let mut decisions: BTreeMap<String, u64> = BTreeMap::new();
    let mut has_scores = false;

    for o in observations {
        r.evaluated += 1;
        let day = days.entry(o.occurred_at.date_naive()).or_default();
        day.0 += 1;
        if o.trapped {
            r.trapped += 1;
        }
        if o.label == Some(Label::Fraud) {
            r.labeled_fraud_total += 1;
        }
        if o.matched {
            r.matched += 1;
            day.1 += 1;
            if r.sample_matches.len() < SAMPLE_LIMIT {
                r.sample_matches.push(o.event_id);
            }
            match o.label {
                Some(Label::Fraud) => r.labeled_fraud_matched += 1,
                Some(Label::Legit) => r.labeled_legit_matched += 1,
                None => {}
            }
        }
        if let Some(score) = o.score {
            has_scores = true;
            let idx = ((score.clamp(0.0, 100.0) / 10.0).floor() as usize).min(9);
            histogram[idx] += 1;
        }
        if let Some(d) = o.decision {
            *decisions.entry(d.to_string()).or_default() += 1;
        }
    }

    r.hit_rate = ratio(r.matched, r.evaluated).unwrap_or(0.0);
    r.precision = ratio(
        r.labeled_fraud_matched,
        r.labeled_fraud_matched + r.labeled_legit_matched,
    );
    r.recall = ratio(r.labeled_fraud_matched, r.labeled_fraud_total);
    r.by_day = days
        .into_iter()
        .map(|(date, (evaluated, matched))| DayPoint {
            date,
            evaluated,
            matched,
        })
        .collect();
    if has_scores {
        r.score_histogram = Some(
            histogram
                .iter()
                .enumerate()
                .map(|(i, &count)| HistogramBucket {
                    bucket: (i as u32) * 10,
                    count,
                })
                .collect(),
        );
        r.decision_distribution = Some(decisions);
    }
    r
}

fn ratio(num: u64, den: u64) -> Option<f64> {
    (den > 0).then(|| num as f64 / den as f64)
}

/// Decision thresholds used by ruleset backtests (defaults = architecture §3.1).
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, Serialize, utoipa::ToSchema)]
pub struct Thresholds {
    #[serde(default = "default_review")]
    pub review: f64,
    #[serde(default = "default_decline")]
    pub decline: f64,
}

fn default_review() -> f64 {
    50.0
}
fn default_decline() -> f64 {
    80.0
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            review: default_review(),
            decline: default_decline(),
        }
    }
}

/// Decision from a (rules-only) score and the rule actions, with the precedence of architecture §3.1.
pub fn decide(score: f64, action: Option<rule_engine::model::Action>, t: Thresholds) -> &'static str {
    use rule_engine::model::Action as A;
    match action {
        Some(A::ForceDecline) => "decline",
        Some(A::ForceApprove) => "approve",
        Some(A::ForceReview) => "review",
        _ if score >= t.decline => "decline",
        _ if score >= t.review => "review",
        _ => "approve",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use pretty_assertions::assert_eq;

    fn obs(day: u32, matched: bool, label: Option<Label>) -> Observation {
        Observation {
            event_id: Uuid::new_v4(),
            occurred_at: Utc.with_ymd_and_hms(2026, 9, day, 12, 0, 0).unwrap(),
            matched,
            trapped: false,
            label,
            score: None,
            decision: None,
        }
    }

    #[test]
    fn precision_recall_hit_rate() {
        let data = vec![
            obs(1, true, Some(Label::Fraud)),
            obs(1, true, Some(Label::Fraud)),
            obs(1, true, Some(Label::Legit)),
            obs(2, false, Some(Label::Fraud)),
            obs(2, false, None),
            obs(2, true, None),
        ];
        let r = summarize(&data, false);
        assert_eq!(r.evaluated, 6);
        assert_eq!(r.matched, 4);
        assert!((r.hit_rate - 4.0 / 6.0).abs() < 1e-9);
        assert!((r.precision.unwrap() - 2.0 / 3.0).abs() < 1e-9);
        assert!((r.recall.unwrap() - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(r.by_day.len(), 2);
        assert_eq!(r.by_day[0].matched, 3);
        assert_eq!(r.sample_matches.len(), 4);
        assert!(r.score_histogram.is_none());
    }

    #[test]
    fn no_labels_gives_none_metrics() {
        let r = summarize(&[obs(3, true, None)], true);
        assert_eq!(r.precision, None);
        assert_eq!(r.recall, None);
        assert!(r.truncated);
    }

    #[test]
    fn histogram_and_decisions() {
        let mut a = obs(1, true, None);
        a.score = Some(100.0);
        a.decision = Some(decide(100.0, None, Thresholds::default()));
        let mut b = obs(1, false, None);
        b.score = Some(55.0);
        b.decision = Some(decide(55.0, None, Thresholds::default()));
        let r = summarize(&[a, b], false);
        let h = r.score_histogram.unwrap();
        assert_eq!(h[9].count, 1);
        assert_eq!(h[5].count, 1);
        let d = r.decision_distribution.unwrap();
        assert_eq!(d.get("decline"), Some(&1));
        assert_eq!(d.get("review"), Some(&1));
    }

    #[test]
    fn action_precedence() {
        use rule_engine::model::Action;
        let t = Thresholds::default();
        assert_eq!(decide(10.0, Some(Action::ForceDecline), t), "decline");
        assert_eq!(decide(95.0, Some(Action::ForceApprove), t), "approve");
        assert_eq!(decide(0.0, Some(Action::ForceReview), t), "review");
        assert_eq!(decide(49.9, None, t), "approve");
    }
}
