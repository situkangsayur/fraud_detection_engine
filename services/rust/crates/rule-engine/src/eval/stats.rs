//! Statistical velocity functions (rule-dsl §6.2): z-score, gaussian tail, percentile rank, linear trend,
//! Poisson tail. Pure functions over history series; all "not enough data" situations trap.

use statrs::distribution::{ContinuousCDF, DiscreteCDF, Normal, Poisson};

use super::Trap;
use crate::model::{Tail, TrendOutput};
use crate::ports::BucketPoint;

fn insufficient(n: usize, min: usize) -> Trap {
    Trap::error(format!("insufficient_history: {n} samples < {min} required"))
}

fn zero_variance() -> Trap {
    Trap::error("zero_variance")
}

/// Mean and sample standard deviation (n − 1). Requires at least `max(min_samples, 2)` values.
pub fn mean_std(values: &[f64], min_samples: usize) -> Result<(f64, f64), Trap> {
    let required = min_samples.max(2);
    if values.len() < required {
        return Err(insufficient(values.len(), required));
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0);
    Ok((mean, var.sqrt()))
}

/// `(x − mean) / std` of the history.
pub fn zscore(x: f64, values: &[f64], min_samples: usize) -> Result<f64, Trap> {
    let (mean, std) = mean_std(values, min_samples)?;
    if std == 0.0 {
        return Err(zero_variance());
    }
    Ok((x - mean) / std)
}

/// Tail probability of `x` under N(mean, std) fitted on the history.
pub fn gaussian_tail(x: f64, values: &[f64], tail: Tail, min_samples: usize) -> Result<f64, Trap> {
    let z = zscore(x, values, min_samples)?;
    let std_normal = Normal::new(0.0, 1.0).map_err(|e| Trap::error(e.to_string()))?;
    Ok(match tail {
        Tail::Upper => std_normal.cdf(-z),
        Tail::Lower => std_normal.cdf(z),
        Tail::Two => (2.0 * std_normal.cdf(-z.abs())).min(1.0),
    })
}

/// Fraction of history values ≤ `x` (0..1).
pub fn percentile_rank(x: f64, values: &[f64], min_samples: usize) -> Result<f64, Trap> {
    let required = min_samples.max(1);
    if values.len() < required {
        return Err(insufficient(values.len(), required));
    }
    let below = values.iter().filter(|v| **v <= x).count();
    Ok(below as f64 / values.len() as f64)
}

/// Ordinary least squares over the history buckets (all but the last); the last bucket is the current one.
pub fn linear_trend(buckets: &[BucketPoint], output: TrendOutput, min_samples: usize) -> Result<f64, Trap> {
    let Some((current, history)) = buckets.split_last() else {
        return Err(insufficient(0, min_samples.max(2)));
    };
    let required = match output {
        TrendOutput::ResidualZ => min_samples.max(3),
        TrendOutput::Slope | TrendOutput::Forecast => min_samples.max(2),
    };
    if history.len() < required {
        return Err(insufficient(history.len(), required));
    }
    let n = history.len() as f64;
    let t_mean = (n - 1.0) / 2.0;
    let y_mean = history.iter().map(|b| b.value).sum::<f64>() / n;
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for (i, b) in history.iter().enumerate() {
        let dt = i as f64 - t_mean;
        sxy += dt * (b.value - y_mean);
        sxx += dt * dt;
    }
    if sxx == 0.0 {
        return Err(insufficient(history.len(), required));
    }
    let slope = sxy / sxx;
    let intercept = y_mean - slope * t_mean;
    let forecast = intercept + slope * n;
    match output {
        TrendOutput::Slope => Ok(slope),
        TrendOutput::Forecast => Ok(forecast),
        TrendOutput::ResidualZ => {
            let ssr: f64 = history
                .iter()
                .enumerate()
                .map(|(i, b)| (b.value - (intercept + slope * i as f64)).powi(2))
                .sum();
            let sigma = (ssr / (n - 2.0)).sqrt();
            if sigma == 0.0 {
                return Err(zero_variance());
            }
            Ok((current.value - forecast) / sigma)
        }
    }
}

/// `P(N ≥ current)` with `N ~ Poisson(λ)`, λ = mean of the history bucket counts.
pub fn poisson_tail(buckets: &[BucketPoint], min_samples: usize) -> Result<f64, Trap> {
    let Some((current, history)) = buckets.split_last() else {
        return Err(insufficient(0, min_samples.max(1)));
    };
    let required = min_samples.max(1);
    if history.len() < required {
        return Err(insufficient(history.len(), required));
    }
    let lambda = history.iter().map(|b| b.value).sum::<f64>() / history.len() as f64;
    let x = current.value.max(0.0).round() as u64;
    if x == 0 {
        return Ok(1.0);
    }
    if lambda <= 0.0 {
        // Never happened before and happens now: as unlikely as it gets.
        return Ok(0.0);
    }
    let dist = Poisson::new(lambda).map_err(|e| Trap::error(e.to_string()))?;
    Ok((1.0 - dist.cdf(x - 1)).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use chrono::{TimeZone, Utc};

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn buckets(values: &[f64]) -> Vec<BucketPoint> {
        values
            .iter()
            .enumerate()
            .map(|(i, v)| BucketPoint {
                start: Utc.timestamp_opt(i as i64 * 3600, 0).unwrap(),
                value: *v,
            })
            .collect()
    }

    #[test]
    fn zscore_hand_computed() {
        // mean 5, sample std sqrt(10/4) = 1.5811
        let h = [3.0, 4.0, 5.0, 6.0, 7.0];
        let z = zscore(10.0, &h, 5).unwrap();
        assert!(close(z, 5.0 / 1.581_138_83), "{z}");
        assert!(zscore(10.0, &h, 6)
            .unwrap_err()
            .reason
            .starts_with("insufficient_history"));
        assert_eq!(
            zscore(1.0, &[2.0, 2.0, 2.0], 2).unwrap_err().reason,
            "zero_variance"
        );
    }

    #[test]
    fn gaussian_tails() {
        let h = [-1.0, 1.0, -1.0, 1.0]; // mean 0, std = sqrt(4/3)
        let std = (4.0f64 / 3.0).sqrt();
        let upper = gaussian_tail(std * 1.96, &h, Tail::Upper, 2).unwrap();
        assert!(close(upper, 0.024_997_9), "{upper}");
        let lower = gaussian_tail(-std * 1.96, &h, Tail::Lower, 2).unwrap();
        assert!(close(lower, 0.024_997_9));
        let two = gaussian_tail(std * 1.96, &h, Tail::Two, 2).unwrap();
        assert!(close(two, 0.049_995_8), "{two}");
    }

    #[test]
    fn percentile_rank_counts() {
        assert!(close(
            percentile_rank(3.0, &[1.0, 2.0, 3.0, 4.0], 1).unwrap(),
            0.75
        ));
        assert!(percentile_rank(3.0, &[], 1).is_err());
    }

    #[test]
    fn linear_trend_outputs() {
        // history 1,2,3,4 → slope 1, forecast 5; current 9 → residual σ = 0 → zero variance
        let b = buckets(&[1.0, 2.0, 3.0, 4.0, 9.0]);
        assert!(close(linear_trend(&b, TrendOutput::Slope, 2).unwrap(), 1.0));
        assert!(close(linear_trend(&b, TrendOutput::Forecast, 2).unwrap(), 5.0));
        assert_eq!(
            linear_trend(&b, TrendOutput::ResidualZ, 3).unwrap_err().reason,
            "zero_variance"
        );
        // history 1,3,2,4: slope 0.8, intercept 1.3, residuals -0.3,0.9,-0.9,0.3 → SSR 1.8, σ = sqrt(0.9)
        let b = buckets(&[1.0, 3.0, 2.0, 4.0, 10.0]);
        let forecast = 1.3 + 0.8 * 4.0;
        let rz = linear_trend(&b, TrendOutput::ResidualZ, 3).unwrap();
        assert!(close(rz, (10.0 - forecast) / 0.9f64.sqrt()), "{rz}");
        assert!(linear_trend(&buckets(&[1.0, 2.0]), TrendOutput::Slope, 2).is_err());
    }

    #[test]
    fn poisson_tail_values() {
        // λ = 2, P(N ≥ 5) = 1 − Σ_{k=0}^{4} e^-2 2^k/k! = 0.052653
        let b = buckets(&[1.0, 3.0, 2.0, 2.0, 5.0]);
        assert!(
            close(poisson_tail(&b, 4).unwrap(), 0.052_653),
            "{}",
            poisson_tail(&b, 4).unwrap()
        );
        assert!(close(poisson_tail(&buckets(&[1.0, 0.0]), 1).unwrap(), 1.0));
        assert!(close(poisson_tail(&buckets(&[0.0, 0.0, 3.0]), 1).unwrap(), 0.0));
        assert!(poisson_tail(&buckets(&[1.0, 2.0]), 5).is_err());
    }
}
