//! Feature set v1 aggregates in **one** round trip.
//!
//! One `SELECT` with `LATERAL` sub-queries; each sub-query hits a project-leading index
//! (`(project_id, customer_id, occurred_at)`, `(project_id, device_id, occurred_at)`, …). The
//! customer's 30-day window is scanned once with `FILTER` clauses for all customer aggregates.
//!
//! The current event is already persisted (pipeline step 4), so "exclude current" filters use
//! `e.id <> $3`; distinct-customer counts intentionally include it. For simulations the event id is
//! a fresh UUID that does not exist, which gives the same semantics.

use contracts::events::CanonicalEventIn;
use platform::db::TenantTx;
use platform::error::AppResult;
use platform::ProjectId;
use uuid::Uuid;

use crate::domain::features::RawAggregates;

const SQL: &str = r#"
WITH cur AS (
    SELECT $1::uuid AS pid, $2::uuid AS cid, $3::uuid AS eid, $4::timestamptz AS ts,
           $5::text AS device, $6::text AS ip, $7::text AS instr, $8::text AS recip,
           $9::text AS promo, $10::text AS api
)
SELECT
    c.cust_cnt_1h, c.cust_cnt_24h, c.cust_txn_sum_24h, c.cust_txn_avg_30d, c.cust_txn_std_30d, c.cust_txn_n_30d,
    c.cust_distinct_devices_24h, c.cust_distinct_ips_24h, c.cust_distinct_instruments_7d,
    c.failed_logins_24h, c.recent_credential_change_24h, c.promo_cnt_cust_30d,
    (SELECT count(DISTINCT e.customer_id) FROM core.events e
      WHERE cur.instr IS NOT NULL AND e.project_id = cur.pid AND e.instrument_fingerprint = cur.instr
        AND e.occurred_at > cur.ts - interval '30 days' AND e.occurred_at <= cur.ts) AS instrument_distinct_customers_30d,
    (SELECT count(DISTINCT e.customer_id) FROM core.events e
      WHERE cur.device IS NOT NULL AND e.project_id = cur.pid AND e.device_id = cur.device
        AND e.occurred_at > cur.ts - interval '30 days' AND e.occurred_at <= cur.ts) AS device_distinct_customers_30d,
    (SELECT count(DISTINCT e.customer_id) FROM core.events e
      WHERE cur.ip IS NOT NULL AND e.project_id = cur.pid AND e.ip_address = cur.ip::inet
        AND e.occurred_at > cur.ts - interval '24 hours' AND e.occurred_at <= cur.ts) AS ip_distinct_customers_24h,
    (SELECT count(*) FROM core.events e
      WHERE cur.device IS NOT NULL AND e.project_id = cur.pid AND e.device_id = cur.device AND e.id <> cur.eid
        AND e.event_type = 'promo_redemption'
        AND e.occurred_at > cur.ts - interval '30 days' AND e.occurred_at <= cur.ts) AS promo_cnt_device_30d,
    (SELECT count(DISTINCT e.customer_id) FROM core.events e
      WHERE cur.device IS NOT NULL AND cur.promo IS NOT NULL AND e.project_id = cur.pid AND e.device_id = cur.device
        AND e.promo_code = cur.promo
        AND e.occurred_at > cur.ts - interval '7 days' AND e.occurred_at <= cur.ts) AS promo_distinct_customers_same_code_device_7d,
    (SELECT count(*) FROM core.events e
      WHERE cur.api IS NOT NULL AND e.project_id = cur.pid AND e.api_client_id = cur.api AND e.id <> cur.eid
        AND e.occurred_at > cur.ts - interval '5 minutes' AND e.occurred_at <= cur.ts) AS api_client_cnt_5m,
    (SELECT count(DISTINCT e.customer_id) FROM core.events e
      WHERE cur.api IS NOT NULL AND e.project_id = cur.pid AND e.api_client_id = cur.api
        AND e.occurred_at > cur.ts - interval '5 minutes' AND e.occurred_at <= cur.ts) AS api_client_distinct_customers_5m,
    (cur.device IS NOT NULL AND EXISTS (SELECT 1 FROM core.events e
      WHERE e.project_id = cur.pid AND e.device_id = cur.device AND e.customer_id = cur.cid AND e.id <> cur.eid
        AND e.occurred_at <= cur.ts)) AS seen_device_before,
    (cur.instr IS NOT NULL AND EXISTS (SELECT 1 FROM core.events e
      WHERE e.project_id = cur.pid AND e.instrument_fingerprint = cur.instr AND e.customer_id = cur.cid
        AND e.id <> cur.eid AND e.occurred_at <= cur.ts)) AS seen_instrument_before,
    (cur.recip IS NOT NULL AND EXISTS (SELECT 1 FROM core.events e
      WHERE e.project_id = cur.pid AND e.recipient_fingerprint = cur.recip AND e.customer_id = cur.cid
        AND e.id <> cur.eid AND e.occurred_at <= cur.ts)) AS seen_recipient_before,
    (SELECT max(e.occurred_at) FROM core.events e
      WHERE e.project_id = cur.pid AND e.customer_id = cur.cid AND e.id <> cur.eid AND e.event_type = 'login'
        AND e.login_success IS TRUE AND e.occurred_at <= cur.ts) AS last_login_at,
    (SELECT max(e.occurred_at) FROM core.events e
      WHERE e.project_id = cur.pid AND e.customer_id = cur.cid AND e.id <> cur.eid
        AND e.event_type = 'account_change' AND e.occurred_at <= cur.ts) AS last_account_change_at,
    (SELECT min(e.occurred_at) FROM core.events e
      WHERE e.project_id = cur.pid AND e.customer_id = cur.cid AND e.id <> cur.eid) AS first_event_at,
    (SELECT e.geo_country::text FROM core.events e
      WHERE e.project_id = cur.pid AND e.customer_id = cur.cid AND e.id <> cur.eid AND e.geo_country IS NOT NULL
        AND e.occurred_at > cur.ts - interval '90 days' AND e.occurred_at <= cur.ts
      GROUP BY e.geo_country ORDER BY count(*) DESC, e.geo_country LIMIT 1) AS usual_geo_country
FROM cur
CROSS JOIN LATERAL (
    SELECT
        count(*) FILTER (WHERE e.occurred_at > cur.ts - interval '1 hour') AS cust_cnt_1h,
        count(*) FILTER (WHERE e.occurred_at > cur.ts - interval '24 hours') AS cust_cnt_24h,
        COALESCE(sum(e.amount) FILTER (WHERE e.event_type = 'transaction'
                 AND e.occurred_at > cur.ts - interval '24 hours'), 0)::float8 AS cust_txn_sum_24h,
        (avg(e.amount) FILTER (WHERE e.event_type = 'transaction'))::float8 AS cust_txn_avg_30d,
        (stddev_samp(e.amount) FILTER (WHERE e.event_type = 'transaction'))::float8 AS cust_txn_std_30d,
        count(e.amount) FILTER (WHERE e.event_type = 'transaction') AS cust_txn_n_30d,
        count(DISTINCT e.device_id) FILTER (WHERE e.occurred_at > cur.ts - interval '24 hours') AS cust_distinct_devices_24h,
        count(DISTINCT e.ip_address) FILTER (WHERE e.occurred_at > cur.ts - interval '24 hours') AS cust_distinct_ips_24h,
        count(DISTINCT e.instrument_fingerprint) FILTER (WHERE e.occurred_at > cur.ts - interval '7 days') AS cust_distinct_instruments_7d,
        count(*) FILTER (WHERE e.event_type = 'login' AND e.login_success IS FALSE
                         AND e.occurred_at > cur.ts - interval '24 hours') AS failed_logins_24h,
        COALESCE(bool_or(e.event_type = 'account_change'
                 AND e.account_change_type IN ('password', 'email', 'phone', '2fa')
                 AND e.occurred_at > cur.ts - interval '24 hours'), false) AS recent_credential_change_24h,
        count(*) FILTER (WHERE e.event_type = 'promo_redemption') AS promo_cnt_cust_30d
    FROM core.events e
    WHERE e.project_id = cur.pid AND e.customer_id = cur.cid AND e.id <> cur.eid
      AND e.occurred_at > cur.ts - interval '30 days' AND e.occurred_at <= cur.ts
) c
"#;

/// Runs the aggregate query for the current event.
pub async fn fetch(
    tx: &mut TenantTx<'_>,
    project: ProjectId,
    event_id: Uuid,
    customer_id: Uuid,
    ev: &CanonicalEventIn,
) -> AppResult<RawAggregates> {
    let row = sqlx::query_as::<_, RawAggregates>(SQL)
        .bind(project.as_uuid())
        .bind(customer_id)
        .bind(event_id)
        .bind(ev.occurred_at)
        .bind(&ev.device_id)
        .bind(&ev.ip_address)
        .bind(&ev.instrument_fingerprint)
        .bind(&ev.recipient_fingerprint)
        .bind(&ev.promo_code)
        .bind(&ev.api_client_id)
        .fetch_one(&mut ***tx)
        .await?;
    Ok(row)
}
