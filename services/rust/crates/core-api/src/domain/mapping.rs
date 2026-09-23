//! Data-source mapping engine (docs/technical/data-sources.md §3).
//!
//! A mapping is **data**, not code: a JSON document stored per data source (versioned), which says
//! how to turn one record of the customer's own shape into a [`CanonicalEventIn`] (plus customer
//! and optional label). This is what lets a tenant plug in their dataset "as is" without us
//! writing an adapter class per customer (the Java way would be one `RecordMapper` implementation
//! per source, compiled and deployed; here the "implementation" is interpreted at runtime).
//!
//! The module is **pure**: no IO, no clock (except parsing), deterministic. That makes it easy to
//! unit-test every transform and lets the same code run for webhook ingest, batch ingest and the
//! UI preview endpoint.
//!
//! Design:
//! * serde models ([`Mapping`], [`FieldSpec`], [`Transform`]) with `deny_unknown_fields`, so typos in
//!   a mapping are rejected at save time instead of silently ignored;
//! * [`Transform`] is an enum. Each variant is one step of a pipeline, applied by a `match`. In Java this would
//!   be a `Transform` interface with 18 implementing classes; in Rust a closed enum gives exhaustive
//!   matching (the compiler tells us when a new transform is not handled everywhere);
//! * errors are collected **per field** ([`FieldError`]) so the UI can highlight the exact mapping
//!   entry that failed.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use contracts::common::Typology;
use contracts::events::{CanonicalEventIn, CustomerIn};
use once_cell::sync::Lazy;
use platform::config::Secret;
use platform::error::FieldError;
use platform::pii;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

/// Canonical event keys a mapping may target (feature-catalog.md §1, plus raw `card_number` /
/// `account_number` which are hashed immediately by normalisation).
pub const CANONICAL_EVENT_KEYS: &[&str] = &[
    "external_id",
    "occurred_at",
    "customer_external_id",
    "channel",
    "status",
    "amount",
    "currency",
    "merchant_id",
    "merchant_category",
    "payment_method",
    "instrument_fingerprint",
    "card_bin",
    "card_last4",
    "issuer_country",
    "recipient_fingerprint",
    "device_id",
    "ip_address",
    "user_agent",
    "geo_country",
    "geo_city",
    "latitude",
    "longitude",
    "promo_code",
    "discount_amount",
    "cashback_amount",
    "ref_transaction_id",
    "shipping_address",
    "billing_address",
    "account_change_type",
    "login_success",
    "api_client_id",
    "card_number",
    "account_number",
];

const REQUIRED_EVENT_KEYS: &[&str] = &["external_id", "occurred_at", "customer_external_id"];
const NUMERIC_EVENT_KEYS: &[&str] = &[
    "amount",
    "discount_amount",
    "cashback_amount",
    "latitude",
    "longitude",
];

// ---------------------------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------------------------

/// A complete mapping document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    /// Event type: usually `{ "from": "...", "value_map": {...}, "default": "transaction" }` or `{ "const": "login" }`.
    #[serde(default)]
    pub event_type: Option<FieldSpec>,
    #[serde(default)]
    pub event: BTreeMap<String, FieldSpec>,
    #[serde(default)]
    pub customer: CustomerSpec,
    #[serde(default)]
    pub label: Option<LabelSpec>,
    /// Source paths removed from the stored payload (e.g. `cvv`, raw `card_number`).
    #[serde(default)]
    pub drop_fields: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomerSpec {
    #[serde(default)]
    pub full_name: Option<FieldSpec>,
    #[serde(default)]
    pub email: Option<FieldSpec>,
    #[serde(default)]
    pub phone: Option<FieldSpec>,
    #[serde(default)]
    pub registered_at: Option<FieldSpec>,
    #[serde(default)]
    pub kyc_level: Option<FieldSpec>,
    #[serde(default)]
    pub segment: Option<FieldSpec>,
    #[serde(default)]
    pub attributes: BTreeMap<String, FieldSpec>,
}

/// Where a value comes from and how it is transformed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    #[serde(default)]
    pub from: Option<FromSpec>,
    #[serde(default, rename = "const")]
    pub constant: Option<Value>,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub transform: Vec<Transform>,
    #[serde(default)]
    pub value_map: Option<Map<String, Value>>,
}

/// One source path or several (for `concat` / `coalesce`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FromSpec {
    One(String),
    Many(Vec<String>),
}

impl FromSpec {
    fn paths(&self) -> Vec<&str> {
        match self {
            Self::One(p) => vec![p.as_str()],
            Self::Many(ps) => ps.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelSpec {
    pub from: String,
    pub fraud_values: Vec<Value>,
    #[serde(default)]
    pub fraud_type: Option<FieldSpec>,
}

/// One transform step (data-sources.md §3 table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "fn", rename_all = "snake_case", deny_unknown_fields)]
pub enum Transform {
    ToNumber {
        /// `id` (1.500.000,50), `en` (1,500,000.50) or absent = auto-detect.
        #[serde(default)]
        locale: Option<String>,
    },
    ToString,
    ToBool,
    ParseDatetime {
        /// strftime format, or `rfc3339` / `unix_s` / `unix_ms`. Absent = auto-detect.
        #[serde(default)]
        format: Option<String>,
        /// IANA zone for naive date-times (default: project timezone).
        #[serde(default)]
        timezone: Option<String>,
    },
    Lowercase,
    Uppercase,
    Trim,
    Scale {
        factor: f64,
    },
    ValueMap {
        map: Map<String, Value>,
        #[serde(default)]
        default: Option<Value>,
    },
    RegexExtract {
        pattern: String,
        #[serde(default)]
        group: Option<usize>,
    },
    Concat {
        #[serde(default)]
        sep: Option<String>,
    },
    Coalesce,
    HashPan,
    HashAccount,
    PanBin {
        #[serde(default)]
        length: Option<usize>,
    },
    PanLast4,
    NormalizePhone {
        #[serde(default)]
        default_country: Option<String>,
    },
    NormalizeEmail,
}

impl Transform {
    fn is_hashing(&self) -> bool {
        matches!(self, Self::HashPan | Self::HashAccount)
    }
}

/// Context needed to apply a mapping (everything else is in the record itself).
#[derive(Debug, Clone)]
pub struct MappingCtx {
    /// Tenant pepper (`pii::tenant_pepper`) used by `hash_pan` / `hash_account`.
    pub pepper: Secret,
    /// Project timezone for naive date-times.
    pub default_tz: Tz,
    /// Data source `default_event_type`, used when the mapping yields no event type.
    pub default_event_type: Option<String>,
}

/// Label extracted from a labelled dataset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MappedLabel {
    pub fraud: bool,
    pub fraud_type: Option<String>,
}

/// Output of [`apply_mapping`].
#[derive(Debug, Clone, PartialEq)]
pub struct MappedRecord {
    /// Canonical event; `payload` holds the source record minus dropped/PII fields.
    pub event: CanonicalEventIn,
    pub label: Option<MappedLabel>,
}

// ---------------------------------------------------------------------------------------------
// Validation (save time)
// ---------------------------------------------------------------------------------------------

/// Parses and validates a mapping document. Returns all problems at once.
pub fn validate_mapping(doc: &Value) -> Result<Mapping, Vec<FieldError>> {
    let mapping: Mapping = serde_json::from_value(doc.clone()).map_err(|e| {
        vec![FieldError::new(
            "mapping",
            format!("invalid mapping document: {e}"),
        )]
    })?;
    let mut errors = Vec::new();

    for key in mapping.event.keys() {
        if !CANONICAL_EVENT_KEYS.contains(&key.as_str()) {
            errors.push(FieldError::new(
                format!("event.{key}"),
                "unknown canonical event field",
            ));
        }
    }
    for key in REQUIRED_EVENT_KEYS {
        match mapping.event.get(*key) {
            Some(spec) if spec.from.is_some() || spec.constant.is_some() => {}
            _ => errors.push(FieldError::new(
                format!("event.{key}"),
                "required field must be mapped (`from` or `const`)",
            )),
        }
    }
    if let Some(et) = &mapping.event_type {
        if et.from.is_none() && et.constant.is_none() && et.default.is_none() {
            errors.push(FieldError::new(
                "event_type",
                "needs `from`, `const` or `default`",
            ));
        }
        if let Some(Value::String(c)) = &et.constant {
            if !contracts::EventType::is_valid(c) {
                errors.push(FieldError::new("event_type.const", "invalid event type"));
            }
        }
    }

    let mut check_spec = |path: String, spec: &FieldSpec| {
        if spec.from.is_none() && spec.constant.is_none() && spec.default.is_none() {
            errors.push(FieldError::new(
                path.clone(),
                "needs `from`, `const` or `default`",
            ));
        }
        if let Some(FromSpec::Many(ps)) = &spec.from {
            if ps.is_empty() {
                errors.push(FieldError::new(format!("{path}.from"), "empty path list"));
            }
        }
        for (i, t) in spec.transform.iter().enumerate() {
            let tpath = format!("{path}.transform[{i}]");
            match t {
                Transform::RegexExtract { pattern, .. } => {
                    if let Err(e) = Regex::new(pattern) {
                        errors.push(FieldError::new(tpath, format!("invalid regex: {e}")));
                    }
                }
                Transform::ParseDatetime {
                    timezone: Some(tz), ..
                } => {
                    if tz.parse::<Tz>().is_err() {
                        errors.push(FieldError::new(tpath, format!("unknown timezone `{tz}`")));
                    }
                }
                Transform::PanBin { length: Some(l) } if *l != 6 && *l != 8 => {
                    errors.push(FieldError::new(tpath, "pan_bin length must be 6 or 8"));
                }
                Transform::Scale { factor } if !factor.is_finite() => {
                    errors.push(FieldError::new(tpath, "scale factor must be finite"));
                }
                _ => {}
            }
        }
    };
    if let Some(et) = &mapping.event_type {
        check_spec("event_type".into(), et);
    }
    for (k, spec) in &mapping.event {
        check_spec(format!("event.{k}"), spec);
    }
    let c = &mapping.customer;
    for (name, spec) in [
        ("full_name", &c.full_name),
        ("email", &c.email),
        ("phone", &c.phone),
        ("registered_at", &c.registered_at),
        ("kyc_level", &c.kyc_level),
        ("segment", &c.segment),
    ] {
        if let Some(s) = spec {
            check_spec(format!("customer.{name}"), s);
        }
    }
    for (k, spec) in &c.attributes {
        check_spec(format!("customer.attributes.{k}"), spec);
    }
    if let Some(l) = &mapping.label {
        if let Some(ft) = &l.fraud_type {
            check_spec("label.fraud_type".into(), ft);
        }
    }
    if mapping.label.as_ref().is_some_and(|l| l.fraud_values.is_empty()) {
        errors.push(FieldError::new("label.fraud_values", "must not be empty"));
    }

    if errors.is_empty() {
        Ok(mapping)
    } else {
        Err(errors)
    }
}

/// All source paths a mapping reads (used to register fields and for the UI).
pub fn referenced_paths(mapping: &Mapping) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |spec: &FieldSpec| {
        if let Some(f) = &spec.from {
            out.extend(f.paths().into_iter().map(String::from));
        }
    };
    if let Some(et) = &mapping.event_type {
        push(et);
    }
    mapping.event.values().for_each(&mut push);
    let c = &mapping.customer;
    [
        &c.full_name,
        &c.email,
        &c.phone,
        &c.registered_at,
        &c.kyc_level,
        &c.segment,
    ]
    .into_iter()
    .flatten()
    .for_each(&mut push);
    c.attributes.values().for_each(&mut push);
    if let Some(l) = &mapping.label {
        out.push(l.from.clone());
    }
    out.sort();
    out.dedup();
    out
}

// ---------------------------------------------------------------------------------------------
// Application (runtime)
// ---------------------------------------------------------------------------------------------

/// Applies `mapping` to one raw `record`.
pub fn apply_mapping(
    mapping: &Mapping,
    record: &Value,
    ctx: &MappingCtx,
) -> Result<MappedRecord, Vec<FieldError>> {
    let mut errors = Vec::new();
    let eval = |errors: &mut Vec<FieldError>, path: &str, spec: &FieldSpec| -> Value {
        match eval_field(spec, record, ctx) {
            Ok(v) => v,
            Err(msg) => {
                errors.push(FieldError::new(path, msg));
                Value::Null
            }
        }
    };

    // --- event type
    let et_value = mapping
        .event_type
        .as_ref()
        .map(|s| eval(&mut errors, "event_type", s))
        .unwrap_or(Value::Null);
    let event_type = match value_to_string(&et_value) {
        Some(s) if !s.is_empty() => Some(s),
        _ => ctx.default_event_type.clone(),
    };

    // --- event fields
    let mut fields: BTreeMap<&str, Value> = BTreeMap::new();
    for (key, spec) in &mapping.event {
        let v = eval(&mut errors, &format!("event.{key}"), spec);
        fields.insert(key.as_str(), v);
    }

    // --- customer
    let c = &mapping.customer;
    let mut cust_vals: BTreeMap<&str, Value> = BTreeMap::new();
    for (name, spec) in [
        ("full_name", &c.full_name),
        ("email", &c.email),
        ("phone", &c.phone),
        ("registered_at", &c.registered_at),
        ("kyc_level", &c.kyc_level),
        ("segment", &c.segment),
    ] {
        if let Some(s) = spec {
            cust_vals.insert(name, eval(&mut errors, &format!("customer.{name}"), s));
        }
    }
    let mut attributes = Map::new();
    for (k, spec) in &c.attributes {
        let v = eval(&mut errors, &format!("customer.attributes.{k}"), spec);
        if !v.is_null() {
            attributes.insert(k.clone(), v);
        }
    }

    // --- label
    let label = match &mapping.label {
        None => None,
        Some(l) => {
            let raw = extract(record, &l.from).cloned().unwrap_or(Value::Null);
            if raw.is_null() {
                None
            } else {
                let fraud = l.fraud_values.iter().any(|fv| loose_eq(fv, &raw));
                let fraud_type = if fraud {
                    let ft = l
                        .fraud_type
                        .as_ref()
                        .map(|s| eval(&mut errors, "label.fraud_type", s))
                        .and_then(|v| value_to_string(&v))
                        .map(|s| s.to_lowercase());
                    Some(
                        ft.filter(|s| Typology::parse(s).is_some())
                            .unwrap_or_else(|| "other".into()),
                    )
                } else {
                    None
                };
                Some(MappedLabel { fraud, fraud_type })
            }
        }
    };

    // --- convert to canonical types
    let mut ev = CanonicalEventIn::default();
    let str_field = |fields: &BTreeMap<&str, Value>, k: &str| fields.get(k).and_then(value_to_string);

    match event_type {
        Some(t) => ev.event_type = t,
        None => errors.push(FieldError::new(
            "event_type",
            "no event type (map it or set a default)",
        )),
    }
    match str_field(&fields, "external_id") {
        Some(s) if !s.is_empty() => ev.external_id = s,
        _ => errors.push(FieldError::new("event.external_id", "required value is missing")),
    }
    match fields.get("occurred_at") {
        Some(v) if !v.is_null() => match value_to_datetime(v, ctx.default_tz) {
            Ok(dt) => ev.occurred_at = dt,
            Err(e) => errors.push(FieldError::new("event.occurred_at", e)),
        },
        _ => errors.push(FieldError::new("event.occurred_at", "required value is missing")),
    }
    match str_field(&fields, "customer_external_id") {
        Some(s) if !s.is_empty() => ev.customer.external_id = s,
        _ => errors.push(FieldError::new(
            "event.customer_external_id",
            "required value is missing",
        )),
    }
    for key in NUMERIC_EVENT_KEYS {
        if let Some(v) = fields.get(key).filter(|v| !v.is_null()) {
            match value_to_f64(v, None) {
                Ok(n) => set_numeric(&mut ev, key, n),
                Err(e) => errors.push(FieldError::new(format!("event.{key}"), e)),
            }
        }
    }
    if let Some(v) = fields.get("login_success").filter(|v| !v.is_null()) {
        match value_to_bool(v) {
            Ok(b) => ev.login_success = Some(b),
            Err(e) => errors.push(FieldError::new("event.login_success", e)),
        }
    }
    for key in CANONICAL_EVENT_KEYS {
        if REQUIRED_EVENT_KEYS.contains(key) || NUMERIC_EVENT_KEYS.contains(key) || *key == "login_success" {
            continue;
        }
        if let Some(s) = str_field(&fields, key) {
            set_string(&mut ev, key, s);
        }
    }

    // customer
    let cust = &mut ev.customer;
    cust.full_name = cust_vals.get("full_name").and_then(value_to_string);
    cust.email = cust_vals.get("email").and_then(value_to_string);
    cust.phone = cust_vals.get("phone").and_then(value_to_string);
    cust.segment = cust_vals.get("segment").and_then(value_to_string);
    if let Some(v) = cust_vals.get("registered_at").filter(|v| !v.is_null()) {
        match value_to_datetime(v, ctx.default_tz) {
            Ok(dt) => cust.registered_at = Some(dt),
            Err(e) => errors.push(FieldError::new("customer.registered_at", e)),
        }
    }
    if let Some(v) = cust_vals.get("kyc_level").filter(|v| !v.is_null()) {
        match value_to_f64(v, None) {
            Ok(n) if (0.0..=32767.0).contains(&n) => cust.kyc_level = Some(n as i16),
            Ok(_) => errors.push(FieldError::new("customer.kyc_level", "out of range")),
            Err(e) => errors.push(FieldError::new("customer.kyc_level", e)),
        }
    }
    if !attributes.is_empty() {
        cust.attributes = Some(attributes);
    }

    // payload = source record minus dropped and hashed-PII paths
    let mut payload = match record {
        Value::Object(m) => Value::Object(m.clone()),
        other => {
            let mut m = Map::new();
            m.insert("value".into(), other.clone());
            Value::Object(m)
        }
    };
    for p in pii_paths(mapping) {
        remove_path(&mut payload, &p);
    }
    for p in &mapping.drop_fields {
        remove_path(&mut payload, p);
    }
    if let Value::Object(m) = payload {
        ev.payload = Some(m);
    }

    if errors.is_empty() {
        Ok(MappedRecord { event: ev, label })
    } else {
        Err(errors)
    }
}

/// Source paths that feed a hashing transform or the raw card/account fields: never stored.
fn pii_paths(mapping: &Mapping) -> Vec<String> {
    let mut out = Vec::new();
    for (key, spec) in &mapping.event {
        let raw_pii = key == "card_number" || key == "account_number";
        if raw_pii || spec.transform.iter().any(Transform::is_hashing) {
            if let Some(f) = &spec.from {
                out.extend(f.paths().into_iter().map(String::from));
            }
        }
        // bin/last4 derived from the same PAN path are covered by the hashing entry of the same path.
        if spec
            .transform
            .iter()
            .any(|t| matches!(t, Transform::PanBin { .. } | Transform::PanLast4))
        {
            if let Some(f) = &spec.from {
                out.extend(f.paths().into_iter().map(String::from));
            }
        }
    }
    out
}

fn set_numeric(ev: &mut CanonicalEventIn, key: &str, n: f64) {
    match key {
        "amount" => ev.amount = Some(n),
        "discount_amount" => ev.discount_amount = Some(n),
        "cashback_amount" => ev.cashback_amount = Some(n),
        "latitude" => ev.latitude = Some(n),
        "longitude" => ev.longitude = Some(n),
        _ => {}
    }
}

fn set_string(ev: &mut CanonicalEventIn, key: &str, s: String) {
    let slot = match key {
        "channel" => &mut ev.channel,
        "status" => &mut ev.status,
        "currency" => &mut ev.currency,
        "merchant_id" => &mut ev.merchant_id,
        "merchant_category" => &mut ev.merchant_category,
        "payment_method" => &mut ev.payment_method,
        "instrument_fingerprint" => &mut ev.instrument_fingerprint,
        "card_bin" => &mut ev.card_bin,
        "card_last4" => &mut ev.card_last4,
        "issuer_country" => &mut ev.issuer_country,
        "recipient_fingerprint" => &mut ev.recipient_fingerprint,
        "device_id" => &mut ev.device_id,
        "ip_address" => &mut ev.ip_address,
        "user_agent" => &mut ev.user_agent,
        "geo_country" => &mut ev.geo_country,
        "geo_city" => &mut ev.geo_city,
        "promo_code" => &mut ev.promo_code,
        "ref_transaction_id" => &mut ev.ref_transaction_id,
        "shipping_address" => &mut ev.shipping_address,
        "billing_address" => &mut ev.billing_address,
        "account_change_type" => &mut ev.account_change_type,
        "api_client_id" => &mut ev.api_client_id,
        "card_number" => &mut ev.card_number,
        "account_number" => &mut ev.account_number,
        _ => return,
    };
    if !s.is_empty() {
        *slot = Some(s);
    }
}

/// Evaluates one field spec: source value(s) → transforms → value_map → default.
fn eval_field(spec: &FieldSpec, record: &Value, ctx: &MappingCtx) -> Result<Value, String> {
    let mut v = if let Some(c) = &spec.constant {
        c.clone()
    } else {
        match &spec.from {
            Some(FromSpec::One(p)) => extract(record, p).cloned().unwrap_or(Value::Null),
            Some(FromSpec::Many(ps)) => Value::Array(
                ps.iter()
                    .map(|p| extract(record, p).cloned().unwrap_or(Value::Null))
                    .collect(),
            ),
            None => Value::Null,
        }
    };
    for t in &spec.transform {
        v = apply_transform(t, v, ctx)?;
    }
    if let Some(map) = &spec.value_map {
        if let Some(key) = value_to_string(&v) {
            match map.get(&key) {
                Some(mapped) => v = mapped.clone(),
                None => {
                    if let Some(d) = &spec.default {
                        v = d.clone();
                    }
                }
            }
        }
    }
    if is_empty(&v) {
        if let Some(d) = &spec.default {
            v = d.clone();
        }
    }
    Ok(v)
}

fn is_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(a) => a.iter().all(is_empty),
        _ => false,
    }
}

static REGEX_CACHE: Lazy<Mutex<HashMap<String, Regex>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn cached_regex(pattern: &str) -> Result<Regex, String> {
    let mut cache = REGEX_CACHE
        .lock()
        .map_err(|_| "regex cache poisoned".to_string())?;
    if let Some(r) = cache.get(pattern) {
        return Ok(r.clone());
    }
    let r = Regex::new(pattern).map_err(|e| format!("invalid regex: {e}"))?;
    if cache.len() > 512 {
        cache.clear();
    }
    cache.insert(pattern.to_string(), r.clone());
    Ok(r)
}

/// Applies one transform. `Null` passes through every transform except `coalesce`/`concat`.
pub fn apply_transform(t: &Transform, v: Value, ctx: &MappingCtx) -> Result<Value, String> {
    if v.is_null() && !matches!(t, Transform::Coalesce | Transform::Concat { .. }) {
        return Ok(Value::Null);
    }
    match t {
        Transform::ToNumber { locale } => {
            let n = value_to_f64(&v, locale.as_deref())?;
            Ok(num(n))
        }
        Transform::ToString => Ok(value_to_string(&v).map(Value::String).unwrap_or(Value::Null)),
        Transform::ToBool => Ok(Value::Bool(value_to_bool(&v)?)),
        Transform::ParseDatetime { format, timezone } => {
            let tz = match timezone {
                Some(t) => t.parse::<Tz>().map_err(|_| format!("unknown timezone `{t}`"))?,
                None => ctx.default_tz,
            };
            let dt = match format.as_deref() {
                None => value_to_datetime(&v, tz)?,
                Some("rfc3339") => {
                    let s = value_to_string(&v).unwrap_or_default();
                    DateTime::parse_from_rfc3339(s.trim())
                        .map(|d| d.with_timezone(&Utc))
                        .map_err(|e| format!("not RFC 3339: {e}"))?
                }
                Some("unix_s") => epoch(value_to_f64(&v, None)?, false)?,
                Some("unix_ms") => epoch(value_to_f64(&v, None)?, true)?,
                Some(fmt) => parse_with_format(&value_to_string(&v).unwrap_or_default(), fmt, tz)?,
            };
            Ok(Value::String(dt.to_rfc3339()))
        }
        Transform::Lowercase => Ok(map_str(v, |s| s.to_lowercase())),
        Transform::Uppercase => Ok(map_str(v, |s| s.to_uppercase())),
        Transform::Trim => Ok(map_str(v, |s| s.trim().to_string())),
        Transform::Scale { factor } => {
            let n = value_to_f64(&v, None)?;
            Ok(num(n * factor))
        }
        Transform::ValueMap { map, default } => {
            let key = value_to_string(&v).unwrap_or_default();
            Ok(map.get(&key).cloned().or_else(|| default.clone()).unwrap_or(v))
        }
        Transform::RegexExtract { pattern, group } => {
            let re = cached_regex(pattern)?;
            let s = value_to_string(&v).unwrap_or_default();
            let g = group.unwrap_or(1);
            Ok(re
                .captures(&s)
                .and_then(|c| c.get(g).or_else(|| c.get(0)))
                .map(|m| Value::String(m.as_str().to_string()))
                .unwrap_or(Value::Null))
        }
        Transform::Concat { sep } => {
            let parts: Vec<String> = match &v {
                Value::Array(a) => a
                    .iter()
                    .filter_map(value_to_string)
                    .filter(|s| !s.is_empty())
                    .collect(),
                other => value_to_string(other).into_iter().collect(),
            };
            if parts.is_empty() {
                Ok(Value::Null)
            } else {
                Ok(Value::String(parts.join(sep.as_deref().unwrap_or(""))))
            }
        }
        Transform::Coalesce => Ok(match v {
            Value::Array(a) => a.into_iter().find(|x| !is_empty(x)).unwrap_or(Value::Null),
            other => other,
        }),
        Transform::HashPan | Transform::HashAccount => {
            let s = value_to_string(&v).unwrap_or_default();
            if pii::digits_only(&s).len() < 6 {
                return Err("value has fewer than 6 digits".into());
            }
            Ok(pii::hash_digits(&ctx.pepper, &s)
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        Transform::PanBin { length } => {
            let s = value_to_string(&v).unwrap_or_default();
            Ok(pii::pan_bin(&s, length.unwrap_or(6))
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        Transform::PanLast4 => {
            let s = value_to_string(&v).unwrap_or_default();
            Ok(pii::pan_last4(&s).map(Value::String).unwrap_or(Value::Null))
        }
        Transform::NormalizePhone { default_country } => {
            let s = value_to_string(&v).unwrap_or_default();
            let cc = country_calling_code(default_country.as_deref().unwrap_or("ID"));
            Ok(pii::normalize_phone(&s, cc)
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        Transform::NormalizeEmail => {
            let s = value_to_string(&v).unwrap_or_default();
            Ok(pii::normalize_email(&s).map(Value::String).unwrap_or(Value::Null))
        }
    }
}

/// ISO country → calling code (the common ones in the region); digits are accepted as-is.
pub fn country_calling_code(country: &str) -> &str {
    match country.to_ascii_uppercase().as_str() {
        "ID" => "62",
        "MY" => "60",
        "SG" => "65",
        "PH" => "63",
        "TH" => "66",
        "VN" => "84",
        "US" | "CA" => "1",
        "GB" => "44",
        "AU" => "61",
        "IN" => "91",
        _ if country.chars().all(|c| c.is_ascii_digit()) && !country.is_empty() => country,
        _ => "62",
    }
}

fn map_str(v: Value, f: impl Fn(&str) -> String) -> Value {
    match v {
        Value::String(s) => Value::String(f(&s)),
        other => other,
    }
}

fn num(n: f64) -> Value {
    Number::from_f64(n).map(Value::Number).unwrap_or(Value::Null)
}

// ---------------------------------------------------------------------------------------------
// Path extraction
// ---------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum Seg<'a> {
    Key(&'a str),
    Index(usize),
}

fn parse_path(path: &str) -> Option<Vec<Seg<'_>>> {
    let mut segs = Vec::new();
    for part in path.split('.') {
        if part.is_empty() {
            return None;
        }
        let (name, mut rest) = match part.find('[') {
            Some(i) => (&part[..i], &part[i..]),
            None => (part, ""),
        };
        if !name.is_empty() {
            segs.push(Seg::Key(name));
        }
        while !rest.is_empty() {
            let close = rest.find(']')?;
            let idx: usize = rest.get(1..close)?.parse().ok()?;
            segs.push(Seg::Index(idx));
            rest = &rest[close + 1..];
            if !rest.is_empty() && !rest.starts_with('[') {
                return None;
            }
        }
    }
    Some(segs)
}

/// Extracts `a.b[0].c` from a JSON record. A top-level key equal to the whole path wins (flat CSV
/// columns such as `"user.id"`).
pub fn extract<'v>(record: &'v Value, path: &str) -> Option<&'v Value> {
    if let Some(v) = record.as_object().and_then(|m| m.get(path)) {
        return Some(v);
    }
    let segs = parse_path(path)?;
    let mut cur = record;
    for s in segs {
        cur = match s {
            Seg::Key(k) => cur.as_object()?.get(k)?,
            Seg::Index(i) => cur.as_array()?.get(i)?,
        };
    }
    Some(cur)
}

/// Removes a path from a JSON value (object keys are removed; array elements are nulled).
pub fn remove_path(record: &mut Value, path: &str) {
    if let Some(m) = record.as_object_mut() {
        if m.remove(path).is_some() {
            return;
        }
    }
    let Some(segs) = parse_path(path) else { return };
    let Some((last, parents)) = segs.split_last() else {
        return;
    };
    let mut cur = record;
    for s in parents {
        let next = match s {
            Seg::Key(k) => cur.as_object_mut().and_then(|m| m.get_mut(*k)),
            Seg::Index(i) => cur.as_array_mut().and_then(|a| a.get_mut(*i)),
        };
        match next {
            Some(n) => cur = n,
            None => return,
        }
    }
    match last {
        Seg::Key(k) => {
            if let Some(m) = cur.as_object_mut() {
                m.remove(*k);
            }
        }
        Seg::Index(i) => {
            if let Some(slot) = cur.as_array_mut().and_then(|a| a.get_mut(*i)) {
                *slot = Value::Null;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Coercions
// ---------------------------------------------------------------------------------------------

pub fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.trim().to_string()),
        Value::Number(n) => Some(match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
            _ => n.to_string(),
        }),
        Value::Bool(b) => Some(b.to_string()),
        other => Some(other.to_string()),
    }
}

fn loose_eq(a: &Value, b: &Value) -> bool {
    match (value_to_string(a), value_to_string(b)) {
        (Some(x), Some(y)) => x.eq_ignore_ascii_case(&y),
        _ => false,
    }
}

pub fn value_to_bool(v: &Value) -> Result<bool, String> {
    match v {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => Ok(n.as_f64().is_some_and(|f| f != 0.0)),
        Value::String(s) => match s.trim().to_lowercase().as_str() {
            "true" | "1" | "yes" | "y" | "ya" | "t" | "success" | "berhasil" => Ok(true),
            "false" | "0" | "no" | "n" | "tidak" | "f" | "failed" | "gagal" => Ok(false),
            other => Err(format!("cannot interpret `{other}` as boolean")),
        },
        _ => Err("cannot interpret value as boolean".into()),
    }
}

/// Parses a number, handling Indonesian (`1.500.000,50`) and English (`1,500,000.50`) grouping and
/// currency prefixes (`Rp`, `IDR`, `$`).
pub fn value_to_f64(v: &Value, locale: Option<&str>) -> Result<f64, String> {
    match v {
        Value::Number(n) => n.as_f64().ok_or_else(|| "not a finite number".into()),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => parse_number(s, locale),
        _ => Err("expected a number".into()),
    }
}

pub fn parse_number(raw: &str, locale: Option<&str>) -> Result<f64, String> {
    let mut s: String = raw.trim().to_string();
    for prefix in ["Rp.", "Rp", "rp", "IDR", "idr", "$", "USD"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim().to_string();
        }
    }
    s.retain(|c| !c.is_whitespace() && c != '_' && c != '\'');
    let negative = s.starts_with('-') || (s.starts_with('(') && s.ends_with(')'));
    s = s.trim_matches(|c| c == '-' || c == '(' || c == ')').to_string();
    if s.is_empty() {
        return Err(format!("`{raw}` is not a number"));
    }
    let normalised = match locale.map(str::to_ascii_lowercase).as_deref() {
        Some("id") | Some("de") => s.replace('.', "").replace(',', "."),
        Some("en") => s.replace(',', ""),
        _ => {
            let dots = s.matches('.').count();
            let commas = s.matches(',').count();
            match (dots, commas) {
                (0, 0) => s,
                (_, 0) if dots > 1 => s.replace('.', ""),
                (0, _) if commas > 1 => s.replace(',', ""),
                (0, 1) => {
                    // "1,5" → decimal; "1,500" → thousands (3 digits after the comma)
                    let after = s.split(',').nth(1).map(str::len).unwrap_or(0);
                    if after == 3 {
                        s.replace(',', "")
                    } else {
                        s.replace(',', ".")
                    }
                }
                (_, 0) => s,
                _ => {
                    // both present: the last one is the decimal separator
                    let last_dot = s.rfind('.').unwrap_or(0);
                    let last_comma = s.rfind(',').unwrap_or(0);
                    if last_comma > last_dot {
                        s.replace('.', "").replace(',', ".")
                    } else {
                        s.replace(',', "")
                    }
                }
            }
        }
    };
    let n: f64 = normalised
        .parse()
        .map_err(|_| format!("`{raw}` is not a number"))?;
    if !n.is_finite() {
        return Err(format!("`{raw}` is not a finite number"));
    }
    Ok(if negative { -n } else { n })
}

fn epoch(n: f64, millis: bool) -> Result<DateTime<Utc>, String> {
    let ms = if millis { n } else { n * 1000.0 };
    DateTime::<Utc>::from_timestamp_millis(ms as i64).ok_or_else(|| "epoch out of range".to_string())
}

const AUTO_FORMATS: &[&str] = &[
    "%Y-%m-%d %H:%M:%S%.f",
    "%Y-%m-%dT%H:%M:%S%.f",
    "%Y-%m-%d %H:%M",
    "%Y/%m/%d %H:%M:%S",
    "%d/%m/%Y %H:%M:%S",
    "%d/%m/%Y %H:%M",
    "%d-%m-%Y %H:%M:%S",
    "%d-%m-%Y %H:%M",
    "%d.%m.%Y %H:%M:%S",
];
const AUTO_DATE_FORMATS: &[&str] = &["%Y-%m-%d", "%d/%m/%Y", "%d-%m-%Y", "%Y/%m/%d", "%d.%m.%Y"];

/// Converts a JSON value into a UTC date-time: RFC 3339, common Indonesian/ISO formats
/// (interpreted in `tz` when naive) or an epoch number (seconds, or milliseconds if > 1e11).
pub fn value_to_datetime(v: &Value, tz: Tz) -> Result<DateTime<Utc>, String> {
    match v {
        Value::Number(n) => {
            let f = n.as_f64().ok_or("invalid epoch")?;
            epoch(f, f.abs() > 1e11)
        }
        Value::String(s) => {
            let s = s.trim();
            if let Ok(d) = DateTime::parse_from_rfc3339(s) {
                return Ok(d.with_timezone(&Utc));
            }
            if let Ok(d) = DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f %z") {
                return Ok(d.with_timezone(&Utc));
            }
            if s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() {
                let f: f64 = s.parse().map_err(|_| "invalid epoch")?;
                return epoch(f, f > 1e11);
            }
            for fmt in AUTO_FORMATS {
                if let Ok(n) = NaiveDateTime::parse_from_str(s, fmt) {
                    return localize(n, tz);
                }
            }
            for fmt in AUTO_DATE_FORMATS {
                if let Ok(d) = NaiveDate::parse_from_str(s, fmt) {
                    return localize(d.and_hms_opt(0, 0, 0).ok_or("invalid date")?, tz);
                }
            }
            Err(format!("unrecognised date-time `{s}`"))
        }
        _ => Err("expected a date-time string or epoch number".into()),
    }
}

fn parse_with_format(s: &str, fmt: &str, tz: Tz) -> Result<DateTime<Utc>, String> {
    let s = s.trim();
    if fmt.contains("%z") || fmt.contains("%:z") {
        return DateTime::parse_from_str(s, fmt)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| format!("`{s}` does not match `{fmt}`: {e}"));
    }
    if let Ok(n) = NaiveDateTime::parse_from_str(s, fmt) {
        return localize(n, tz);
    }
    NaiveDate::parse_from_str(s, fmt)
        .map_err(|e| format!("`{s}` does not match `{fmt}`: {e}"))
        .and_then(|d| localize(d.and_hms_opt(0, 0, 0).ok_or("invalid date")?, tz))
}

fn localize(n: NaiveDateTime, tz: Tz) -> Result<DateTime<Utc>, String> {
    tz.from_local_datetime(&n)
        .earliest()
        .map(|d| d.with_timezone(&Utc))
        .ok_or_else(|| "non-existent local time".to_string())
}

/// Builds a canonical event from an already-canonical JSON body (the `canonical` data source and
/// `POST /events`). Kept here so that both ingest paths share the same error format.
pub fn canonical_from_json(v: &Value) -> Result<CanonicalEventIn, Vec<FieldError>> {
    serde_json::from_value::<CanonicalEventIn>(v.clone())
        .map_err(|e| vec![FieldError::new("event", format!("invalid canonical event: {e}"))])
}

/// Convenience used by tests and the preview endpoint.
pub fn customer_is_empty(c: &CustomerIn) -> bool {
    c.external_id.is_empty()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn ctx() -> MappingCtx {
        MappingCtx {
            pepper: Secret::new("pepper"),
            default_tz: chrono_tz::Asia::Jakarta,
            default_event_type: Some("transaction".into()),
        }
    }

    fn doc_example() -> Value {
        json!({
          "event_type": { "from": "trx_type", "value_map": { "PURCHASE": "transaction", "LOGIN": "login",
                          "VOUCHER": "promo_redemption" }, "default": "transaction" },
          "event": {
            "external_id": { "from": "trx_id" },
            "occurred_at": { "from": "created", "transform": [{ "fn": "parse_datetime", "format": "%d/%m/%Y %H:%M:%S", "timezone": "Asia/Jakarta" }] },
            "customer_external_id": { "from": "user.id", "transform": [{ "fn": "to_string" }] },
            "amount": { "from": "total", "transform": [{ "fn": "to_number" }, { "fn": "scale", "factor": 0.01 }] },
            "currency": { "const": "IDR" },
            "instrument_fingerprint": { "from": "card_number", "transform": [{ "fn": "hash_pan" }] },
            "card_bin": { "from": "card_number", "transform": [{ "fn": "pan_bin" }] },
            "card_last4": { "from": "card_number", "transform": [{ "fn": "pan_last4" }] },
            "shipping_address": { "from": ["ship.street", "ship.city"], "transform": [{ "fn": "concat", "sep": ", " }] }
          },
          "customer": {
            "full_name": { "from": "user.name" },
            "email": { "from": "user.email" },
            "phone": { "from": "user.hp", "transform": [{ "fn": "normalize_phone", "default_country": "ID" }] },
            "registered_at": { "from": "user.join_date", "transform": [{ "fn": "parse_datetime", "format": "%Y-%m-%d" }] },
            "attributes": { "monthly_income": { "from": "user.income", "transform": [{ "fn": "to_number", "locale": "id" }] } }
          },
          "label": { "from": "is_fraud", "fraud_values": [1, "1", "true", "Y"],
                     "fraud_type": { "from": "fraud_category", "default": "other" } },
          "drop_fields": ["cvv", "card_number"]
        })
    }

    fn record() -> Value {
        json!({
            "trx_id": "TX-9", "trx_type": "PURCHASE", "created": "23/09/2026 14:30:00",
            "total": "1.500.000", "card_number": "4111 1111 1111 1111", "cvv": "123",
            "ship": { "street": "Jl. Sudirman 1", "city": "Jakarta" },
            "user": { "id": 42, "name": "Budi", "email": "Budi@Example.com", "hp": "0812-3456-7890",
                      "join_date": "2025-01-02", "income": "12.500.000,50" },
            "is_fraud": "Y", "fraud_category": "carding"
        })
    }

    #[test]
    fn doc_example_validates_and_applies() {
        let m = validate_mapping(&doc_example()).expect("valid");
        let out = apply_mapping(&m, &record(), &ctx()).expect("applies");
        let ev = out.event;
        assert_eq!(ev.external_id, "TX-9");
        assert_eq!(ev.event_type, "transaction");
        assert_eq!(ev.customer.external_id, "42");
        // 14:30 WIB = 07:30 UTC
        assert_eq!(ev.occurred_at.to_rfc3339(), "2026-09-23T07:30:00+00:00");
        // "1.500.000" auto-detected as thousands → 1_500_000 × 0.01
        assert_eq!(ev.amount, Some(15000.0));
        assert_eq!(ev.currency.as_deref(), Some("IDR"));
        assert_eq!(ev.card_bin.as_deref(), Some("411111"));
        assert_eq!(ev.card_last4.as_deref(), Some("1111"));
        let fp = ev.instrument_fingerprint.clone().unwrap();
        assert_eq!(fp.len(), 64);
        assert_eq!(ev.shipping_address.as_deref(), Some("Jl. Sudirman 1, Jakarta"));
        assert_eq!(ev.customer.phone.as_deref(), Some("+6281234567890"));
        assert_eq!(
            ev.customer
                .attributes
                .as_ref()
                .and_then(|a| a.get("monthly_income")),
            Some(&json!(12500000.5))
        );
        // PII removed from the stored payload, other fields kept
        let payload = ev.payload.unwrap();
        assert!(!payload.contains_key("card_number"));
        assert!(!payload.contains_key("cvv"));
        assert_eq!(payload.get("trx_id"), Some(&json!("TX-9")));
        assert_eq!(
            out.label,
            Some(MappedLabel {
                fraud: true,
                fraud_type: Some("carding".into())
            })
        );
    }

    #[test]
    fn pan_hash_is_stable_and_pepper_dependent() {
        let t = Transform::HashPan;
        let a = apply_transform(&t, json!("4111-1111-1111-1111"), &ctx()).unwrap();
        let b = apply_transform(&t, json!("4111111111111111"), &ctx()).unwrap();
        assert_eq!(a, b);
        let other = MappingCtx {
            pepper: Secret::new("other"),
            ..ctx()
        };
        assert_ne!(apply_transform(&t, json!("4111111111111111"), &other).unwrap(), a);
        assert!(apply_transform(&t, json!("12"), &ctx()).is_err());
    }

    #[test]
    fn unknown_event_value_falls_back_to_default() {
        let m = validate_mapping(&doc_example()).unwrap();
        let mut r = record();
        r["trx_type"] = json!("SOMETHING_ELSE");
        let out = apply_mapping(&m, &r, &ctx()).unwrap();
        assert_eq!(out.event.event_type, "transaction");
    }

    #[test]
    fn missing_required_fields_are_reported_per_field() {
        let m = validate_mapping(&doc_example()).unwrap();
        let r = json!({ "trx_type": "LOGIN" });
        let errs = apply_mapping(&m, &r, &ctx()).unwrap_err();
        let fields: Vec<_> = errs.iter().map(|e| e.field.as_str()).collect();
        assert!(fields.contains(&"event.external_id"));
        assert!(fields.contains(&"event.occurred_at"));
        assert!(fields.contains(&"event.customer_external_id"));
    }

    #[test]
    fn validation_rejects_unknown_and_missing() {
        let bad = json!({
            "event": { "externalid": { "from": "x" }, "occurred_at": { "from": "t",
              "transform": [{ "fn": "regex_extract", "pattern": "(" }] } }
        });
        let errs = validate_mapping(&bad).unwrap_err();
        let fields: Vec<_> = errs.iter().map(|e| e.field.as_str()).collect();
        assert!(fields.contains(&"event.externalid"));
        assert!(fields.contains(&"event.external_id"));
        assert!(fields.contains(&"event.customer_external_id"));
        assert!(fields
            .iter()
            .any(|f| f.starts_with("event.occurred_at.transform")));
        // unknown transform fn is a document error
        let bad_fn =
            json!({ "event": { "external_id": { "from": "x", "transform": [{ "fn": "explode" }] } } });
        assert!(validate_mapping(&bad_fn).is_err());
    }

    #[test]
    fn indonesian_and_english_numbers() {
        assert_eq!(parse_number("1.500.000,50", Some("id")).unwrap(), 1_500_000.5);
        assert_eq!(parse_number("Rp 1.500.000", None).unwrap(), 1_500_000.0);
        assert_eq!(parse_number("1,500,000.50", None).unwrap(), 1_500_000.5);
        assert_eq!(parse_number("1.500.000,50", None).unwrap(), 1_500_000.5);
        assert_eq!(parse_number("1,5", None).unwrap(), 1.5);
        assert_eq!(parse_number("12,500", None).unwrap(), 12_500.0);
        assert_eq!(parse_number("-250", None).unwrap(), -250.0);
        assert_eq!(parse_number("(250)", None).unwrap(), -250.0);
        assert!(parse_number("abc", None).is_err());
    }

    #[test]
    fn datetime_formats() {
        let tz = chrono_tz::Asia::Jakarta;
        let cases = [
            (json!("2026-09-23T10:00:00Z"), "2026-09-23T10:00:00+00:00"),
            (json!("2026-09-23 17:00:00"), "2026-09-23T10:00:00+00:00"),
            (json!("23/09/2026 17:00"), "2026-09-23T10:00:00+00:00"),
            (json!("23-09-2026 17:00:00"), "2026-09-23T10:00:00+00:00"),
            (json!("2026-09-23"), "2026-09-22T17:00:00+00:00"),
            (json!(1_790_157_600), "2026-09-23T10:00:00+00:00"),
            (json!(1_790_157_600_000_i64), "2026-09-23T10:00:00+00:00"),
            (json!("1790157600"), "2026-09-23T10:00:00+00:00"),
        ];
        for (v, expected) in cases {
            assert_eq!(value_to_datetime(&v, tz).unwrap().to_rfc3339(), expected, "{v}");
        }
        assert!(value_to_datetime(&json!("kemarin"), tz).is_err());
    }

    #[test]
    fn path_extraction() {
        let r = json!({ "a": { "b": [ { "c": 1 }, { "c": 2 } ] }, "user.id": "flat" });
        assert_eq!(extract(&r, "a.b[1].c"), Some(&json!(2)));
        assert_eq!(extract(&r, "a.b[0]"), Some(&json!({ "c": 1 })));
        assert_eq!(extract(&r, "user.id"), Some(&json!("flat")));
        assert_eq!(extract(&r, "a.x"), None);
        assert_eq!(extract(&r, "a.b[9].c"), None);
        assert_eq!(extract(&r, "a..b"), None);
        let mut r2 = r.clone();
        remove_path(&mut r2, "a.b[0].c");
        assert_eq!(r2["a"]["b"][0], json!({}));
        remove_path(&mut r2, "a.b[1]");
        assert_eq!(r2["a"]["b"][1], Value::Null);
    }

    #[test]
    fn transforms_misc() {
        let c = ctx();
        let t = |t: Transform, v: Value| apply_transform(&t, v, &c).unwrap();
        assert_eq!(t(Transform::ToBool, json!("ya")), json!(true));
        assert_eq!(t(Transform::ToBool, json!("gagal")), json!(false));
        assert_eq!(t(Transform::ToString, json!(42)), json!("42"));
        assert_eq!(t(Transform::Uppercase, json!("id")), json!("ID"));
        assert_eq!(t(Transform::Coalesce, json!([null, "", "x", "y"])), json!("x"));
        assert_eq!(
            t(
                Transform::RegexExtract {
                    pattern: r"INV-(\d+)".into(),
                    group: None
                },
                json!("ref INV-778")
            ),
            json!("778")
        );
        assert_eq!(
            t(
                Transform::PanBin { length: Some(8) },
                json!("5555 4444 3333 1111")
            ),
            json!("55554444")
        );
        assert_eq!(
            t(Transform::NormalizeEmail, json!("A.B+x@gmail.com")),
            json!("ab@gmail.com")
        );
        assert_eq!(t(Transform::ToNumber { locale: None }, Value::Null), Value::Null);
        assert_eq!(
            t(
                Transform::ParseDatetime {
                    format: Some("unix_ms".into()),
                    timezone: None
                },
                json!(0)
            ),
            json!("1970-01-01T00:00:00+00:00")
        );
    }

    #[test]
    fn referenced_paths_lists_all_sources() {
        let m = validate_mapping(&doc_example()).unwrap();
        let p = referenced_paths(&m);
        for expected in ["trx_id", "user.id", "ship.city", "is_fraud", "card_number"] {
            assert!(p.contains(&expected.to_string()), "{expected}");
        }
    }
}
