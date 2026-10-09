//! Canonical event normalisation, shared by every ingest path (canonical API, webhook mapping,
//! batch import, simulation).
//!
//! Responsibilities:
//! * **PII first.** A raw `card_number` / `account_number` becomes a tenant-peppered fingerprint
//!   (+ BIN / last4) and the raw value is dropped **before anything else touches the event**.
//! * Normalise contact data for graph linking (email, phone) and codes (currency, countries).
//! * Validate lengths and formats, returning per-field errors (problem+json 422 / dead-letter).

use std::net::IpAddr;

use contracts::events::CanonicalEventIn;
use contracts::EventType;
use platform::config::Secret;
use platform::error::FieldError;
use platform::pii;

const MAX_ID_LEN: usize = 200;
const MAX_TEXT_LEN: usize = 1000;
/// Upper bound for the stored source payload (`source.*`).
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

/// A validated canonical event plus normalised contact data for the customer upsert.
#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedEvent {
    pub event: CanonicalEventIn,
    pub email_normalized: Option<String>,
    pub phone_normalized: Option<String>,
}

/// Validates and normalises `ev` in place. `pepper` is the **tenant** pepper.
pub fn normalize_event(
    mut ev: CanonicalEventIn,
    pepper: &Secret,
) -> Result<NormalizedEvent, Vec<FieldError>> {
    let mut errors = Vec::new();

    // --- PII: hash raw numbers immediately
    if let Some(pan) = ev.card_number.take() {
        match pii::hash_digits(pepper, &pan) {
            Some(fp) if pii::digits_only(&pan).len() >= 12 => {
                ev.instrument_fingerprint.get_or_insert(fp);
                if ev.card_bin.is_none() {
                    ev.card_bin = pii::pan_bin(&pan, 6);
                }
                if ev.card_last4.is_none() {
                    ev.card_last4 = pii::pan_last4(&pan);
                }
            }
            _ => errors.push(FieldError::new("card_number", "must contain 12-19 digits")),
        }
    }
    if let Some(acc) = ev.account_number.take() {
        match pii::hash_digits(pepper, &acc) {
            Some(fp) if pii::digits_only(&acc).len() >= 6 => {
                ev.recipient_fingerprint.get_or_insert(fp);
            }
            _ => errors.push(FieldError::new(
                "account_number",
                "must contain at least 6 digits",
            )),
        }
    }

    // --- identifiers
    ev.external_id = ev.external_id.trim().to_string();
    if ev.external_id.is_empty() || ev.external_id.len() > MAX_ID_LEN {
        errors.push(FieldError::new("external_id", "required, 1-200 characters"));
    }
    ev.customer.external_id = ev.customer.external_id.trim().to_string();
    if ev.customer.external_id.is_empty() || ev.customer.external_id.len() > MAX_ID_LEN {
        errors.push(FieldError::new(
            "customer.external_id",
            "required, 1-200 characters",
        ));
    }
    ev.event_type = ev.event_type.trim().to_lowercase();
    if !EventType::is_valid(&ev.event_type) {
        errors.push(FieldError::new(
            "event_type",
            "must be lower snake case, 2-40 characters (e.g. transaction, login)",
        ));
    }

    // --- codes
    if let Some(c) = ev.currency.as_mut() {
        *c = c.trim().to_uppercase();
        if c.len() != 3 || !c.chars().all(|ch| ch.is_ascii_alphabetic()) {
            errors.push(FieldError::new(
                "currency",
                "must be an ISO 4217 code (3 letters)",
            ));
        }
    }
    for (name, slot) in [
        ("issuer_country", &mut ev.issuer_country),
        ("geo_country", &mut ev.geo_country),
    ] {
        if let Some(c) = slot.as_mut() {
            *c = c.trim().to_uppercase();
            if c.len() != 2 || !c.chars().all(|ch| ch.is_ascii_alphabetic()) {
                errors.push(FieldError::new(name, "must be an ISO 3166-1 alpha-2 code"));
            }
        }
    }
    if let Some(ip) = ev.ip_address.as_mut() {
        *ip = ip.trim().to_string();
        if ip.parse::<IpAddr>().is_err() {
            errors.push(FieldError::new("ip_address", "not a valid IPv4/IPv6 address"));
        }
    }
    for (name, v) in [
        ("amount", ev.amount),
        ("discount_amount", ev.discount_amount),
        ("cashback_amount", ev.cashback_amount),
    ] {
        if let Some(x) = v {
            if !x.is_finite() || x.abs() > 1e16 {
                errors.push(FieldError::new(name, "out of range"));
            }
        }
    }
    if let Some(lat) = ev.latitude {
        if !(-90.0..=90.0).contains(&lat) {
            errors.push(FieldError::new("latitude", "must be between -90 and 90"));
        }
    }
    if let Some(lon) = ev.longitude {
        if !(-180.0..=180.0).contains(&lon) {
            errors.push(FieldError::new("longitude", "must be between -180 and 180"));
        }
    }

    // --- free text lengths
    for (name, slot) in [
        ("channel", &mut ev.channel),
        ("status", &mut ev.status),
        ("merchant_id", &mut ev.merchant_id),
        ("merchant_category", &mut ev.merchant_category),
        ("payment_method", &mut ev.payment_method),
        ("device_id", &mut ev.device_id),
        ("user_agent", &mut ev.user_agent),
        ("geo_city", &mut ev.geo_city),
        ("promo_code", &mut ev.promo_code),
        ("ref_transaction_id", &mut ev.ref_transaction_id),
        ("shipping_address", &mut ev.shipping_address),
        ("billing_address", &mut ev.billing_address),
        ("account_change_type", &mut ev.account_change_type),
        ("api_client_id", &mut ev.api_client_id),
    ] {
        if let Some(s) = slot.as_mut() {
            *s = s.trim().to_string();
            if s.is_empty() {
                *slot = None;
            } else if s.len() > MAX_TEXT_LEN {
                errors.push(FieldError::new(name, "too long (max 1000 characters)"));
            }
        }
    }
    for slot in [
        &mut ev.channel,
        &mut ev.payment_method,
        &mut ev.account_change_type,
    ] {
        if let Some(s) = slot.as_mut() {
            *s = s.to_lowercase();
        }
    }

    if let Some(p) = &ev.payload {
        let size = serde_json::to_vec(p).map(|v| v.len()).unwrap_or(0);
        if size > MAX_PAYLOAD_BYTES {
            errors.push(FieldError::new("payload", "source payload exceeds 64 KiB"));
        }
    }

    // --- contact data
    let email_normalized = ev.customer.email.as_deref().and_then(pii::normalize_email);
    let phone_normalized = ev
        .customer
        .phone
        .as_deref()
        .and_then(|p| pii::normalize_phone(p, "62"));
    if let Some(name) = &ev.customer.full_name {
        if name.len() > MAX_TEXT_LEN {
            errors.push(FieldError::new("customer.full_name", "too long"));
        }
    }

    if errors.is_empty() {
        Ok(NormalizedEvent {
            event: ev,
            email_normalized,
            phone_normalized,
        })
    } else {
        Err(errors)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use chrono::Utc;
    use contracts::events::CustomerIn;

    fn base() -> CanonicalEventIn {
        CanonicalEventIn {
            external_id: " T-1 ".into(),
            event_type: "Transaction".into(),
            occurred_at: Utc::now(),
            customer: CustomerIn {
                external_id: "C-1".into(),
                email: Some("Budi.S+promo@gmail.com".into()),
                phone: Some("0812 3456 7890".into()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn hashes_card_and_drops_raw() {
        let mut ev = base();
        ev.card_number = Some("4111 1111 1111 1111".into());
        ev.account_number = Some("1234567890".into());
        ev.currency = Some("idr".into());
        ev.geo_country = Some("id".into());
        let n = normalize_event(ev, &Secret::new("p")).unwrap();
        assert!(n.event.card_number.is_none());
        assert!(n.event.account_number.is_none());
        assert_eq!(n.event.card_bin.as_deref(), Some("411111"));
        assert_eq!(n.event.card_last4.as_deref(), Some("1111"));
        assert_eq!(n.event.instrument_fingerprint.as_ref().map(String::len), Some(64));
        assert!(n.event.recipient_fingerprint.is_some());
        assert_eq!(n.event.external_id, "T-1");
        assert_eq!(n.event.event_type, "transaction");
        assert_eq!(n.event.currency.as_deref(), Some("IDR"));
        assert_eq!(n.event.geo_country.as_deref(), Some("ID"));
        assert_eq!(n.email_normalized.as_deref(), Some("budis@gmail.com"));
        assert_eq!(n.phone_normalized.as_deref(), Some("+6281234567890"));
    }

    #[test]
    fn reports_invalid_fields() {
        let mut ev = base();
        ev.event_type = "Bad Type".into();
        ev.ip_address = Some("999.1.1.1".into());
        ev.currency = Some("RUPIAH".into());
        ev.latitude = Some(123.0);
        ev.card_number = Some("1234".into());
        let errs = normalize_event(ev, &Secret::new("p")).unwrap_err();
        let fields: Vec<_> = errs.iter().map(|e| e.field.as_str()).collect();
        for f in ["event_type", "ip_address", "currency", "latitude", "card_number"] {
            assert!(fields.contains(&f), "{f}");
        }
    }
}
