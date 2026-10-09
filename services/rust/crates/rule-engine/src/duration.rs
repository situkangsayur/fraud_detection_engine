//! Window / bucket durations such as `"30d"`, `"1h"`, `"90s"` (rule-dsl §6.2).

use std::fmt;

use serde::{Deserialize, Serialize};

/// A duration written as `<n>(s|m|h|d|w)`. Stored with its original text so JSON round-trips unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DurationSpec {
    raw: String,
    seconds: i64,
}

impl DurationSpec {
    /// Parses `"30d"` style durations. The amount must be a positive integer.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        let Some(unit) = raw.chars().last() else {
            return Err("empty duration".to_string());
        };
        let amount_text = &raw[..raw.len() - unit.len_utf8()];
        let amount: i64 = amount_text
            .parse()
            .map_err(|_| format!("invalid duration '{raw}': expected <n>(s|m|h|d|w), e.g. \"30d\""))?;
        if amount <= 0 {
            return Err(format!("invalid duration '{raw}': amount must be > 0"));
        }
        let unit_seconds = match unit {
            's' => 1,
            'm' => 60,
            'h' => 3_600,
            'd' => 86_400,
            'w' => 604_800,
            _ => {
                return Err(format!(
                    "invalid duration unit '{unit}' in '{raw}': use s, m, h, d or w"
                ))
            }
        };
        let seconds = amount
            .checked_mul(unit_seconds)
            .ok_or_else(|| format!("duration '{raw}' is too large"))?;
        Ok(Self {
            raw: raw.to_string(),
            seconds,
        })
    }

    /// Total length in seconds.
    pub fn seconds(&self) -> i64 {
        self.seconds
    }

    /// Original text, e.g. `"30d"`.
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl TryFrom<String> for DurationSpec {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<DurationSpec> for String {
    fn from(value: DurationSpec) -> Self {
        value.raw
    }
}

impl fmt::Display for DurationSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parses_units() {
        assert_eq!(DurationSpec::parse("90s").unwrap().seconds(), 90);
        assert_eq!(DurationSpec::parse("5m").unwrap().seconds(), 300);
        assert_eq!(DurationSpec::parse("1h").unwrap().seconds(), 3_600);
        assert_eq!(DurationSpec::parse("30d").unwrap().seconds(), 2_592_000);
        assert_eq!(DurationSpec::parse("2w").unwrap().seconds(), 1_209_600);
    }

    #[test]
    fn rejects_bad_input() {
        for bad in ["", "d", "0d", "-1d", "1y", "1.5h", "abc"] {
            assert!(DurationSpec::parse(bad).is_err(), "{bad} should fail");
        }
    }

    #[test]
    fn serde_round_trip() {
        let d: DurationSpec = serde_json::from_str("\"7d\"").unwrap();
        assert_eq!(serde_json::to_string(&d).unwrap(), "\"7d\"");
        assert!(serde_json::from_str::<DurationSpec>("\"7x\"").is_err());
    }
}
