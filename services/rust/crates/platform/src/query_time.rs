//! Lenient date/time query parameters: `from=2026-09-23` and `from=2026-09-23T08:00:00Z` are both accepted.
//!
//! Use with `#[serde(default, deserialize_with = "…")]` on `Option<DateTime<Utc>>` fields. A bare date means
//! the start of that day (UTC) for `from`, and the end of that day (exclusive: next midnight) for `to`.

use chrono::{DateTime, Days, NaiveDate, NaiveTime, Utc};
use serde::{de, Deserialize, Deserializer};

fn parse(raw: &str, end_of_day: bool) -> Result<DateTime<Utc>, String> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Ok(dt.with_timezone(&Utc));
    }
    let day = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| format!("expected RFC 3339 date-time or YYYY-MM-DD, got `{raw}`"))?;
    let day = if end_of_day {
        day.checked_add_days(Days::new(1)).ok_or("date out of range")?
    } else {
        day
    };
    Ok(day.and_time(NaiveTime::MIN).and_utc())
}

fn opt<'de, D: Deserializer<'de>>(d: D, end_of_day: bool) -> Result<Option<DateTime<Utc>>, D::Error> {
    match Option::<String>::deserialize(d)? {
        Some(raw) if !raw.trim().is_empty() => {
            parse(raw.trim(), end_of_day).map(Some).map_err(de::Error::custom)
        }
        _ => Ok(None),
    }
}

/// For range starts: a bare date is that day's midnight.
pub fn opt_from<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
    opt(d, false)
}

/// For range ends: a bare date includes the whole day (next midnight).
pub fn opt_to<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
    opt(d, true)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Q {
        #[serde(default, deserialize_with = "opt_from")]
        from: Option<DateTime<Utc>>,
        #[serde(default, deserialize_with = "opt_to")]
        to: Option<DateTime<Utc>>,
    }

    fn q(s: &str) -> Result<Q, serde_json::Error> {
        serde_json::from_str(s)
    }

    #[test]
    fn accepts_dates_and_datetimes() {
        let r = q(r#"{"from":"2026-09-23","to":"2026-09-30"}"#).unwrap();
        assert_eq!(r.from.unwrap().to_rfc3339(), "2026-09-23T00:00:00+00:00");
        assert_eq!(r.to.unwrap().to_rfc3339(), "2026-10-01T00:00:00+00:00");
        let r = q(r#"{"from":"2026-09-23T08:30:00+07:00"}"#).unwrap();
        assert_eq!(r.from.unwrap().to_rfc3339(), "2026-09-23T01:30:00+00:00");
    }

    #[test]
    fn missing_or_empty_is_none_and_garbage_is_an_error() {
        let r = q("{}").unwrap();
        assert!(r.from.is_none() && r.to.is_none());
        assert!(q(r#"{"from":""}"#).unwrap().from.is_none());
        assert!(q(r#"{"from":"yesterday"}"#).is_err());
    }
}
