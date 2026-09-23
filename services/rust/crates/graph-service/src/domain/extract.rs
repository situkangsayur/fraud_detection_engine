//! Entity extraction: turns a [`GraphLinksRequest`] into normalised [`EntityKey`]s.
//!
//! Normalisation is what makes linking work: `0812-3456-7890`, `+62 812 3456 7890` and
//! `6281234567890` must become the same phone entity; `J.Doe+promo@gmail.com` and
//! `jdoe@gmail.com` the same email. The rules live in `platform::pii` so core-api, rule-service
//! and graph-service normalise identically.
//!
//! Card and bank-account values arrive as tenant-peppered HMAC fingerprints (core-api hashes them
//! at ingest); raw numbers never reach this service.

use std::collections::BTreeMap;

use contracts::graph::{GraphLinksRequest, LinkKind};
use platform::pii;

use super::model::{limits, EntityKey};

/// Extracts and normalises every linkable entity of the request, deduplicated and sorted.
pub fn extract_entities(req: &GraphLinksRequest) -> Vec<EntityKey> {
    let mut out: BTreeMap<(&'static str, String), EntityKey> = BTreeMap::new();
    let mut push = |kind: LinkKind, value: Option<String>, display: &dyn Fn(&str) -> String| {
        if let Some(v) = value.filter(|v| !v.is_empty() && v.len() <= limits::MAX_VALUE_LEN) {
            let d = display(&v);
            out.entry((kind.as_str(), v.clone())).or_insert(EntityKey {
                kind,
                value: v,
                display: d,
            });
        }
    };

    let c = &req.customer;
    let e = &req.event;

    push(
        LinkKind::Email,
        c.email.as_deref().and_then(pii::normalize_email),
        &|v| pii::mask_email(v),
    );
    push(
        LinkKind::Phone,
        c.phone.as_deref().and_then(|p| pii::normalize_phone(p, "62")),
        &|v| pii::mask_phone(v),
    );
    push(LinkKind::Device, trimmed(e.device_id.as_deref()), &|v| {
        pii::mask_text(v, 8)
    });
    push(
        LinkKind::Ip,
        e.ip_address.as_deref().and_then(normalize_ip),
        &|v| mask_ip(v),
    );
    let card_display = pii::mask_pan(e.card_bin.as_deref(), e.card_last4.as_deref());
    push(
        LinkKind::Card,
        trimmed(e.instrument_fingerprint.as_deref()).map(|s| s.to_ascii_lowercase()),
        &|_| card_display.clone(),
    );
    push(
        LinkKind::BankAccount,
        trimmed(e.recipient_fingerprint.as_deref()).map(|s| s.to_ascii_lowercase()),
        &|v| format!("acct:{}", pii::mask_text(v, 8)),
    );
    for addr in [&e.shipping_address, &e.billing_address] {
        push(
            LinkKind::Address,
            addr.as_deref().and_then(pii::normalize_address),
            &|v| pii::mask_text(v, 24),
        );
    }
    push(
        LinkKind::RefTransaction,
        trimmed(e.ref_transaction_id.as_deref()),
        &|v| v.to_string(),
    );
    push(LinkKind::ApiClient, trimmed(e.api_client_id.as_deref()), &|v| {
        v.to_string()
    });

    out.into_values().collect()
}

fn trimmed(v: Option<&str>) -> Option<String> {
    v.map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

// IP helpers live in `platform::pii` (shared with core-api); re-exported for local callers.
pub use platform::pii::{mask_ip, normalize_ip};

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use chrono::Utc;
    use contracts::graph::{GraphCustomer, GraphEventLinks};
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    fn req() -> GraphLinksRequest {
        GraphLinksRequest {
            customer: GraphCustomer {
                id: Uuid::new_v4(),
                external_id: "C1".into(),
                risk_label: "unknown".into(),
                email: Some("J.Doe+promo@Gmail.com".into()),
                phone: Some("0812-3456-7890".into()),
            },
            event: GraphEventLinks {
                id: Uuid::new_v4(),
                occurred_at: Utc::now(),
                device_id: Some(" dev-123456789 ".into()),
                ip_address: Some("::ffff:10.1.2.3".into()),
                instrument_fingerprint: Some("ABCDEF".into()),
                card_bin: Some("411111".into()),
                card_last4: Some("1111".into()),
                recipient_fingerprint: None,
                shipping_address: Some("Jl. Sudirman No. 1".into()),
                billing_address: Some("jalan sudirman nomor 1".into()),
                ref_transaction_id: Some("".into()),
                api_client_id: None,
            },
        }
    }

    #[test]
    fn extracts_normalised_deduplicated_entities() {
        let keys = extract_entities(&req());
        let kinds: Vec<&str> = keys.iter().map(|k| k.kind.as_str()).collect();
        // shipping and billing normalise to the same address → one entity; empty ref ignored
        assert_eq!(kinds, vec!["address", "card", "device", "email", "ip", "phone"]);
        let get = |k: &str| keys.iter().find(|e| e.kind.as_str() == k).unwrap();
        assert_eq!(get("email").value, "jdoe@gmail.com");
        assert_eq!(get("phone").value, "+6281234567890");
        assert_eq!(get("ip").value, "10.1.2.3");
        assert_eq!(get("ip").display, "10.1.2.x");
        assert_eq!(get("card").value, "abcdef");
        assert_eq!(get("card").display, "411111********1111");
        assert_eq!(get("device").value, "dev-123456789");
    }

    #[test]
    fn invalid_ip_is_ignored_and_ipv6_masked() {
        assert_eq!(normalize_ip("not-an-ip"), None);
        assert_eq!(mask_ip("2001:db8:1:2:3:4:5:6"), "2001:db8:1:2:…");
    }
}
