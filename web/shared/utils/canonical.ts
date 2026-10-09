// Canonical fields a data-source mapping can target (docs/technical/feature-catalog.md §1, data-sources.md §3).

export const REQUIRED_EVENT_FIELDS = ['external_id', 'occurred_at', 'customer_external_id'] as const

export const CANONICAL_EVENT_FIELDS = [
  'external_id', 'occurred_at', 'customer_external_id', 'channel', 'status', 'amount', 'currency', 'merchant_id',
  'merchant_category', 'payment_method', 'instrument_fingerprint', 'card_bin', 'card_last4', 'issuer_country',
  'recipient_fingerprint', 'device_id', 'ip_address', 'user_agent', 'geo_country', 'geo_city', 'latitude', 'longitude',
  'promo_code', 'discount_amount', 'cashback_amount', 'ref_transaction_id', 'shipping_address', 'billing_address',
  'account_change_type', 'login_success', 'api_client_id',
] as const

export const CANONICAL_CUSTOMER_FIELDS = ['full_name', 'email', 'phone', 'registered_at', 'kyc_level', 'segment'] as const

export const EVENT_TYPES: string[] = ['transaction', 'login', 'account_change', 'promo_redemption', 'payout', 'registration', 'refund']

export interface TransformSpec {
  fn: string
  params: { name: string, type: 'string' | 'number', placeholder?: string }[]
}

export const TRANSFORMS: TransformSpec[] = [
  { fn: 'to_number', params: [{ name: 'locale', type: 'string', placeholder: 'id' }] },
  { fn: 'to_string', params: [] },
  { fn: 'to_bool', params: [] },
  { fn: 'parse_datetime', params: [{ name: 'format', type: 'string', placeholder: '%d/%m/%Y %H:%M:%S | rfc3339 | unix_s | unix_ms' }, { name: 'timezone', type: 'string', placeholder: 'Asia/Jakarta' }] },
  { fn: 'lowercase', params: [] },
  { fn: 'uppercase', params: [] },
  { fn: 'trim', params: [] },
  { fn: 'scale', params: [{ name: 'factor', type: 'number', placeholder: '0.01' }] },
  { fn: 'value_map', params: [{ name: 'map', type: 'string', placeholder: '{"A":"x"}' }, { name: 'default', type: 'string' }] },
  { fn: 'regex_extract', params: [{ name: 'pattern', type: 'string' }, { name: 'group', type: 'number', placeholder: '1' }] },
  { fn: 'concat', params: [{ name: 'sep', type: 'string', placeholder: ', ' }] },
  { fn: 'coalesce', params: [] },
  { fn: 'hash_pan', params: [] },
  { fn: 'hash_account', params: [] },
  { fn: 'pan_bin', params: [{ name: 'length', type: 'number', placeholder: '6' }] },
  { fn: 'pan_last4', params: [] },
  { fn: 'normalize_phone', params: [{ name: 'default_country', type: 'string', placeholder: 'ID' }] },
  { fn: 'normalize_email', params: [] },
]
