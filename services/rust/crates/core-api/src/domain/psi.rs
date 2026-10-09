//! Population Stability Index for feature drift (`GET /analytics/drift`).
//!
//! PSI = Σ (rᵢ − bᵢ) · ln(rᵢ / bᵢ) over bins, where bᵢ/rᵢ are the baseline/recent shares. Bins are
//! the baseline deciles (10 quantile bins), so each baseline bin holds ~10 % of the data. Empty
//! bins get a small epsilon to keep the logarithm finite. Rule of thumb: < 0.1 stable,
//! 0.1–0.25 moderate, > 0.25 significant shift.

const EPS: f64 = 1e-4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftStatus {
    Stable,
    Moderate,
    Significant,
}

impl DriftStatus {
    pub fn of(psi: f64) -> Self {
        if psi < 0.1 {
            Self::Stable
        } else if psi < 0.25 {
            Self::Moderate
        } else {
            Self::Significant
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Moderate => "moderate",
            Self::Significant => "significant",
        }
    }
}

/// Decile edges of `sorted` (ascending, non-empty). Duplicate edges are collapsed.
fn decile_edges(sorted: &[f64]) -> Vec<f64> {
    let n = sorted.len();
    let mut edges: Vec<f64> = (1..10)
        .map(|k| {
            let idx = ((k * n) / 10).min(n - 1);
            sorted[idx]
        })
        .collect();
    edges.dedup_by(|a, b| a == b);
    edges
}

fn shares(values: &[f64], edges: &[f64]) -> Vec<f64> {
    let mut counts = vec![0usize; edges.len() + 1];
    // Bins are upper-inclusive: (-inf, e1], (e1, e2], ..., (ek, +inf).
    for v in values {
        let bin = edges.partition_point(|e| e < v);
        counts[bin] += 1;
    }
    let total = values.len().max(1) as f64;
    counts.into_iter().map(|c| c as f64 / total).collect()
}

/// PSI of `recent` against `baseline`. `None` when either side is empty or has no finite values.
pub fn psi(baseline: &[f64], recent: &[f64]) -> Option<f64> {
    let mut base: Vec<f64> = baseline.iter().copied().filter(|x| x.is_finite()).collect();
    let rec: Vec<f64> = recent.iter().copied().filter(|x| x.is_finite()).collect();
    if base.is_empty() || rec.is_empty() {
        return None;
    }
    base.sort_by(f64::total_cmp);
    let edges = decile_edges(&base);
    let b = shares(&base, &edges);
    let r = shares(&rec, &edges);
    let value = b
        .iter()
        .zip(&r)
        .map(|(bi, ri)| {
            let (bi, ri) = (bi.max(EPS), ri.max(EPS));
            (ri - bi) * (ri / bi).ln()
        })
        .sum::<f64>();
    Some((value * 10_000.0).round() / 10_000.0)
}

pub fn mean(xs: &[f64]) -> Option<f64> {
    if xs.is_empty() {
        None
    } else {
        Some(xs.iter().sum::<f64>() / xs.len() as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_distributions_are_stable() {
        let base: Vec<f64> = (0..1000).map(f64::from).collect();
        let p = psi(&base, &base).unwrap_or(f64::NAN);
        assert!(p < 0.01, "{p}");
        assert_eq!(DriftStatus::of(p), DriftStatus::Stable);
    }

    #[test]
    fn shifted_distribution_is_significant() {
        let base: Vec<f64> = (0..1000).map(f64::from).collect();
        let shifted: Vec<f64> = (0..1000).map(|x| f64::from(x) + 600.0).collect();
        let p = psi(&base, &shifted).unwrap_or(0.0);
        assert!(p > 0.25, "{p}");
        assert_eq!(DriftStatus::of(p), DriftStatus::Significant);
    }

    #[test]
    fn constant_feature_and_empty_inputs() {
        assert_eq!(psi(&[], &[1.0]), None);
        assert_eq!(psi(&[1.0], &[]), None);
        let p = psi(&[0.0; 50], &[0.0; 50]).unwrap_or(f64::NAN);
        assert!(p.abs() < 1e-9);
        let p = psi(&[0.0; 50], &[1.0; 50]).unwrap_or(0.0);
        assert!(p > 0.25);
        assert_eq!(mean(&[1.0, 3.0]), Some(2.0));
    }
}
