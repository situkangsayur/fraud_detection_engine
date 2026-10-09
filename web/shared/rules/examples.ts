// Canonical examples from docs/technical/rule-dsl.md §6 (plus a formula example from §3.1).
// Used as round-trip test fixtures, as "start from template" presets in the rule builder, and by the mock API.
import type { RuleEnvelope } from './dsl'

export const EXAMPLE_SIMPLE: RuleEnvelope = {
  code: 'RL-CARD-001',
  name: 'High amount with BIN/IP country mismatch',
  description: 'Amount above 5.000.000 while issuer country differs from geo country',
  kind: 'simple',
  typologies: ['carding'],
  event_types: ['transaction'],
  risk_score: 40,
  trapped_score: 0,
  action: 'score',
  on_trapped: 'ignore',
  missing_as_no_match: false,
  definition: {
    kind: 'simple',
    when: {
      all: [
        { left: { type: 'field', path: 'event.amount' }, op: 'gt', right: { type: 'const', value: 5000000 } },
        { left: { type: 'field', path: 'event.issuer_country' }, op: 'ne', right: { type: 'field', path: 'event.geo_country' } },
      ],
    },
    scoring: 'binary',
  },
}

export const EXAMPLE_FORMULA: RuleEnvelope = {
  code: 'RL-ATO-FORMULA',
  name: 'Formula risk index',
  description: 'F(x,y,z) = 2x + 2^y / z^2 compared with a threshold; weighted multi-field scoring',
  kind: 'simple',
  typologies: ['account_takeover'],
  event_types: ['transaction'],
  risk_score: 35,
  trapped_score: 10,
  action: 'score',
  on_trapped: 'score',
  missing_as_no_match: false,
  definition: {
    kind: 'simple',
    scoring: 'weighted',
    when: {
      any: [
        {
          left: {
            type: 'formula',
            expr: 'F(x,y,z) = 2x + 2^y / z^2',
            args: {
              x: { type: 'field', path: 'features.amount_zscore_30d' },
              y: { type: 'const', value: 3 },
              z: { type: 'field', path: 'features.cust_cnt_24h' },
            },
          },
          op: 'gte',
          right: { type: 'const', value: 22 },
          weight: 2,
        },
        { left: { type: 'field', path: 'features.is_new_device' }, op: 'eq', right: { type: 'const', value: 1 }, weight: 1 },
        { not: { left: { type: 'field', path: 'event.promo_code' }, op: 'is_null' } },
        {
          at_least: {
            n: 2,
            of: [
              { left: { type: 'field', path: 'features.recent_credential_change_24h' }, op: 'eq', right: { type: 'const', value: 1 } },
              { left: { type: 'field', path: 'event.channel' }, op: 'in', right: { type: 'const', value: ['web', 'api'] } },
              { left: { type: 'field', path: 'customer.full_name' }, op: 'similar', right: { type: 'const', value: 'Budi Santoso' }, options: { threshold: 0.9, method: 'jaro_winkler' } },
            ],
          },
        },
      ],
    },
  },
}

export const EXAMPLE_VELOCITY: RuleEnvelope = {
  code: 'RL-CARD-003',
  name: 'Card shared by many customers',
  description: 'Same card fingerprint used by >= 3 distinct customers in 30 days',
  kind: 'velocity',
  typologies: ['carding'],
  event_types: ['transaction'],
  risk_score: 45,
  trapped_score: 0,
  action: 'score',
  on_trapped: 'ignore',
  missing_as_no_match: false,
  definition: {
    kind: 'velocity',
    history_event_types: ['transaction'],
    group_by: ['instrument_fingerprint'],
    window: { duration: '30d' },
    aggregate: { fn: 'distinct_count', field: 'customer_id' },
    statistic: null,
    include_current: true,
    min_samples: 1,
    compare: { op: 'gte', right: { type: 'const', value: 3 } },
  },
}

export const EXAMPLE_VELOCITY_STAT: RuleEnvelope = {
  code: 'RL-ATO-004',
  name: 'Amount far above customer normal (gaussian tail)',
  kind: 'velocity',
  typologies: ['account_takeover', 'bank_account_takeover'],
  event_types: ['transaction', 'payout'],
  risk_score: 35,
  action: 'score',
  on_trapped: 'ignore',
  definition: {
    kind: 'velocity',
    history_event_types: ['transaction', 'payout'],
    group_by: ['customer_id'],
    window: { last_n: 50 },
    aggregate: { fn: 'avg', field: 'amount' },
    statistic: { fn: 'gaussian_tail', of: { type: 'field', path: 'event.amount' }, tail: 'upper' },
    include_current: false,
    min_samples: 5,
    compare: { op: 'lt', right: { type: 'const', value: 0.01 } },
  },
}

export const EXAMPLE_COMPOSITE: RuleEnvelope = {
  code: 'RL-PROMO-002',
  name: 'Promo farmed from one device',
  description: 'Same promo redeemed with discount by >= 3 customers on one device in 7 days',
  kind: 'composite',
  typologies: ['promo_abuse'],
  event_types: ['promo_redemption', 'transaction'],
  risk_score: 50,
  action: 'score',
  on_trapped: 'ignore',
  definition: {
    kind: 'composite',
    gate: { all: [{ left: { type: 'field', path: 'event.promo_code' }, op: 'is_not_null' }] },
    history_filter: {
      all: [
        { left: { type: 'hist', path: 'promo_code' }, op: 'eq', right: { type: 'field', path: 'event.promo_code' } },
        { left: { type: 'hist', path: 'discount_amount' }, op: 'gt', right: { type: 'const', value: 0 } },
      ],
    },
    velocity: {
      history_event_types: ['promo_redemption', 'transaction'],
      group_by: ['device_id'],
      window: { duration: '7d' },
      aggregate: { fn: 'distinct_count', field: 'customer_id' },
      compare: { op: 'gte', right: { type: 'const', value: 3 } },
    },
  },
}

export const EXAMPLE_REFERENCE_EXISTS: RuleEnvelope = {
  code: 'RL-BL-001',
  name: 'Blacklisted card',
  kind: 'reference',
  typologies: ['carding'],
  event_types: ['transaction'],
  risk_score: 100,
  action: 'force_decline',
  on_trapped: 'review',
  definition: {
    kind: 'reference',
    list: 'card_blacklist',
    key: { type: 'field', path: 'event.instrument_fingerprint' },
    mode: 'exists',
  },
}

export const EXAMPLE_REFERENCE_ATTRIBUTE: RuleEnvelope = {
  code: 'RL-MER-001',
  name: 'Amount above merchant limit',
  kind: 'reference',
  typologies: ['other'],
  event_types: ['transaction'],
  risk_score: 30,
  action: 'force_review',
  on_trapped: 'ignore',
  definition: {
    kind: 'reference',
    list: 'merchant_limits',
    key: { type: 'field', path: 'event.merchant_id' },
    mode: 'attribute',
    attribute_condition: { left: { type: 'field', path: 'event.amount' }, op: 'gt', right: { type: 'ref', path: 'max_amount' } },
  },
}

export const EXAMPLE_GRAPH: RuleEnvelope = {
  code: 'RL-GRAPH-001',
  name: 'Within 2 hops of a known fraudster',
  kind: 'graph',
  typologies: ['money_mule', 'promo_abuse'],
  event_types: [],
  risk_score: 60,
  action: 'score',
  on_trapped: 'ignore',
  definition: {
    kind: 'graph',
    metric: 'distance_to_fraud',
    link_kinds: ['phone', 'card', 'device', 'address', 'email', 'bank_account', 'ref_transaction'],
    include_similar: true,
    max_depth: 3,
    compare: { op: 'lte', right: { type: 'const', value: 2 } },
  },
}

export const RULE_EXAMPLES: RuleEnvelope[] = [
  EXAMPLE_SIMPLE,
  EXAMPLE_FORMULA,
  EXAMPLE_VELOCITY,
  EXAMPLE_VELOCITY_STAT,
  EXAMPLE_COMPOSITE,
  EXAMPLE_REFERENCE_EXISTS,
  EXAMPLE_REFERENCE_ATTRIBUTE,
  EXAMPLE_GRAPH,
]
