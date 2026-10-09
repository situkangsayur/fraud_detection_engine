// Deterministic demo data for MOCK_API mode (seeded PRNG → stable screenshots & smoke tests).
// Shapes follow shared/types/api.ts (i.e. docs/technical/api-contract.md).
import type {
  AuditEntry, Case, Cluster, Customer, DataSource, DecisionOut, Engine, EventSummary, FraudType, Label, GraphCommunity,
  IngestJob, LlmReport, MappingVersion, MlAlgorithm, MlModel, Project, ProjectMember, ProjectSettings, Proposal,
  ProjectionPoint, ReferenceEntry, ReferenceList, Regulation, Rule, RuleVersion, Ruleset, Tenant, UserSummary,
} from '#shared/types/api'
import { RULE_EXAMPLES } from '#shared/rules/examples'
import type { RuleEnvelope } from '#shared/rules/dsl'

// ---------------------------------------------------------------- PRNG
export function mulberry32(seed: number) {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6D2B79F5) >>> 0
    let t = a
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

const rand = mulberry32(42)
const pick = <T>(arr: readonly T[]): T => arr[Math.floor(rand() * arr.length)] as T
const gauss = () => {
  const u = 1 - rand()
  const v = rand()
  return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v)
}

let idCounter = 0
export function mockId(prefix: string): string {
  idCounter += 1
  const hex = (idCounter * 2654435761 >>> 0).toString(16).padStart(8, '0')
  const tag = Array.from(prefix).map(c => c.charCodeAt(0).toString(16)).join('').padEnd(4, '0').slice(0, 4)
  return `${hex}-${tag}-4000-8000-${idCounter.toString(16).padStart(12, '0')}`
}

const NOW = Date.UTC(2026, 8, 23, 9, 0, 0)
export const iso = (msAgo: number) => new Date(NOW - msAgo).toISOString()
const HOUR = 3_600_000
const DAY = 24 * HOUR

// ---------------------------------------------------------------- tenants, users, projects
export const TENANT: Tenant = { id: mockId('tnt'), slug: 'demo', name: 'Demo Marketplace', status: 'active', users: 4, created_at: iso(90 * DAY) }
export const TENANT_2: Tenant = { id: mockId('tnt'), slug: 'bank-nusantara', name: 'Bank Nusantara', status: 'active', users: 0, created_at: iso(40 * DAY) }

export const USERS: UserSummary[] = [
  { id: mockId('usr'), email: 'admin@fraud.local', full_name: 'Platform Admin', tenant_id: null, tenant_role: 'member', is_platform_admin: true, is_active: true },
  { id: mockId('usr'), email: 'owner@demo.local', full_name: 'Hendri (Tenant Admin)', tenant_id: TENANT.id, tenant_role: 'tenant_admin', is_platform_admin: false, is_active: true },
  { id: mockId('usr'), email: 'analyst@demo.local', full_name: 'Sari Analyst', tenant_id: TENANT.id, tenant_role: 'member', is_platform_admin: false, is_active: true },
  { id: mockId('usr'), email: 'approver@demo.local', full_name: 'Budi Approver', tenant_id: TENANT.id, tenant_role: 'member', is_platform_admin: false, is_active: true },
  { id: mockId('usr'), email: 'viewer@demo.local', full_name: 'Dewi Viewer', tenant_id: TENANT.id, tenant_role: 'member', is_platform_admin: false, is_active: true },
]

const defaultMl = (): Project['ml_config'] => ({
  supervised: { algorithm: 'mlp_backprop', params: { hidden_layers: [64, 32], dropout: 0.2, lr: 0.001, epochs: 50 } },
  unsupervised: { anomaly_algorithm: 'isolation_forest', anomaly_params: { n_estimators: 200 }, clustering_algorithm: 'hdbscan', clustering_params: { min_cluster_size: 15 } },
  features: { include: ['*'], exclude: [], extra_source_fields: [] },
})
const defaultLlm = (): Project['llm_config'] => ({ chat_model: 'qwen2.5:7b-instruct', temperature: 0.1, language: 'id', system_prompt_extra: '' })
const defaultGraph = (): Project['graph_config'] => ({
  link_kinds: ['email', 'phone', 'device', 'card', 'bank_account', 'address', 'ref_transaction'],
  include_similar: true, max_depth: 3, supernode_degree_cap: 50, similarity_threshold: 0.85,
})

function project(slug: string, name: string, stage: Project['stage'], context: string, daysAgo: number): Project {
  return {
    id: mockId('prj'), tenant_id: TENANT.id, slug, name, description: `${name} protection`, stage,
    business_context: context, timezone: 'Asia/Jakarta', currency: 'IDR', status: 'active',
    ml_config: defaultMl(), llm_config: defaultLlm(), graph_config: defaultGraph(), created_at: iso(daysAgo * DAY),
  }
}

export const PROJECTS: Project[] = [
  project('checkout', 'Checkout', 'pre_payment', 'Checkout marketplace sebelum otorisasi pembayaran kartu, VA dan e-wallet.', 80),
  project('post-payment', 'Post Payment', 'post_payment', 'Setelah pembayaran sukses, sebelum barang dikirim oleh seller.', 70),
  project('returns', 'Returns & Refund', 'returns', 'Permintaan retur dan refund, termasuk klaim barang tidak diterima.', 60),
  project('promo', 'Promo & Cashback', 'promo', 'Redeem voucher flash sale dan pencairan cashback.', 50),
]

export const MEMBERS: Record<string, ProjectMember[]> = Object.fromEntries(PROJECTS.map(p => [p.id, [
  { user_id: USERS[1]!.id, email: USERS[1]!.email, full_name: USERS[1]!.full_name, role: 'project_admin' },
  { user_id: USERS[2]!.id, email: USERS[2]!.email, full_name: USERS[2]!.full_name, role: 'analyst' },
  { user_id: USERS[3]!.id, email: USERS[3]!.email, full_name: USERS[3]!.full_name, role: 'approver' },
  { user_id: USERS[4]!.id, email: USERS[4]!.email, full_name: USERS[4]!.full_name, role: 'viewer' },
]]))

export const DEFAULT_SETTINGS = (): ProjectSettings => ({
  decision_thresholds: { review: 50, decline: 80 },
  engine_combination: 'noisy_or',
  engine_weights: { rules: 0.45, supervised: 0.3, unsupervised: 0.1, graph: 0.15 },
  graph_scores: { fraud_distance_scores: { 1: 90, 2: 70, 3: 40 }, shared_fraud_entity_score: 60 },
  timeouts: { graph_ms: 100, ml_ms: 200, rules_ms: 300 },
  cases: { auto_create_on: ['review', 'decline'] },
  rules_unavailable_decision: 'review',
})
export const SETTINGS: Record<string, ProjectSettings> = Object.fromEntries(PROJECTS.map(p => [p.id, DEFAULT_SETTINGS()]))

// ---------------------------------------------------------------- customers & events
const FIRST = ['Budi', 'Sari', 'Andi', 'Dewi', 'Rizky', 'Putri', 'Agus', 'Maya', 'Joko', 'Lina', 'Fajar', 'Nina']
const LAST = ['Santoso', 'Wijaya', 'Pratama', 'Lestari', 'Saputra', 'Hidayat', 'Kusuma', 'Siregar']
const EVENT_TYPES = ['transaction', 'transaction', 'transaction', 'login', 'promo_redemption', 'account_change', 'payout']
const CHANNELS = ['web', 'mobile_app', 'mobile_app', 'api']
const FRAUD_TYPES: FraudType[] = ['carding', 'account_takeover', 'promo_abuse', 'bank_account_takeover', 'system_breach', 'refund_abuse']

/** Internal mock record (API shapes are produced by the route handlers in index.ts). */
export interface MockEvent extends EventSummary {
  event: Record<string, unknown>
  payload: Record<string, unknown>
  features: Record<string, unknown> | null
  decision_detail: DecisionOut | null
  labels: Label[]
  case_id: string | null
}

export interface MockCase extends Case {
  customer_external_id: string
  final_score: number | null
  decision: DecisionOut['decision'] | null
}

export interface ProjectData {
  customers: Customer[]
  events: MockEvent[]
  cases: MockCase[]
}

const projectData = new Map<string, ProjectData>()

function engineScore(v: number) { return Math.round(Math.max(0, Math.min(100, v)) * 10) / 10 }

/** Scales raw reason weights so contributions attribute final_score (they sum to it), top 8. */
function attribute<R extends { contribution: number }>(final: number, raw: R[]): R[] {
  const list = raw.length ? raw : []
  const total = list.reduce((a, r) => a + r.contribution, 0)
  if (!total) return []
  return list.map(r => ({ ...r, contribution: Math.round((r.contribution / total) * final * 10) / 10 }))
    .sort((a, b) => b.contribution - a.contribution).slice(0, 8)
}

function makeDecision(pid: string, ev: { id: string, external_id: string, created_at: string }, fraudish: boolean, ruleIds: Rule[]): DecisionOut {
  const base = fraudish ? 55 + rand() * 40 : rand() * 45
  const scores: Record<Engine, number> = {
    rules: engineScore(base + gauss() * 10),
    supervised: engineScore(base + gauss() * 12),
    unsupervised: engineScore(base * 0.7 + gauss() * 15),
    graph: engineScore(fraudish && rand() > 0.5 ? 70 : rand() * 20),
  }
  const w = { rules: 0.45, supervised: 0.3, unsupervised: 0.1, graph: 0.15 }
  const final = engineScore((Object.keys(w) as Engine[]).reduce((s, k) => s + w[k] * scores[k], 0))
  const decision = final >= 80 ? 'decline' : final >= 50 ? 'review' : 'approve'
  const matched = ruleIds.filter(() => rand() < (fraudish ? 0.35 : 0.05)).slice(0, 4)
  const trapped = ruleIds.find(r => r.kind === 'velocity' && rand() < 0.08)
  return {
    event_id: ev.id,
    external_id: ev.external_id,
    project_id: pid,
    decision,
    final_score: final,
    engine_scores: scores,
    reasons: attribute(final, [
      ...matched.map(r => ({ code: r.code, engine: 'rules' as const, contribution: engineScore(r.envelope.risk_score * 0.8), message: r.name })),
      ...(scores.supervised > 60 ? [{ code: 'ML_SUPERVISED_HIGH', engine: 'supervised' as const, contribution: engineScore(scores.supervised * 0.3), message: 'Supervised model fraud probability is high' }] : []),
      ...(scores.graph >= 70 ? [{ code: 'GRAPH_FRAUD_DISTANCE_2', engine: 'graph' as const, contribution: 10.5, message: 'Within 2 hops of a confirmed fraudster' }] : []),
    ]),
    rule_results: [
      ...matched.map(r => ({
        rule_id: r.id, rule_code: r.code, version: r.current_version, ruleset_code: 'RS-CORE', kind: r.kind, outcome: 'match' as const,
        contribution: r.envelope.risk_score, shadow: false, action: r.envelope.action ?? 'score', trapped_reason: null,
        trace: r.kind === 'velocity' ? { value: 4, op: 'gte', right: 3, samples: 4, window: '30d' } : { left: 6250000, op: 'gt', right: 5000000 },
        duration_us: Math.round(200 + rand() * 900),
      })),
      ...(trapped
        ? [{
            rule_id: trapped.id, rule_code: trapped.code, version: trapped.current_version, ruleset_code: 'RS-CORE', kind: trapped.kind,
            outcome: 'trapped' as const, contribution: 0, shadow: false, action: 'score', trapped_reason: 'insufficient_history (3 < min_samples 5)',
            trace: { samples: 3, min_samples: 5 }, duration_us: 640,
          }]
        : []),
      ...ruleIds.filter(r => r.status === 'shadow').slice(0, 1).map(r => ({
        rule_id: r.id, rule_code: r.code, version: r.current_version, ruleset_code: 'RS-SHADOW', kind: r.kind, outcome: 'match' as const,
        contribution: r.envelope.risk_score, shadow: true, action: 'score', trapped_reason: null, trace: { value: 5, op: 'gte', right: 3 }, duration_us: 410,
      })),
    ],
    ml: {
      fraud_probability: scores.supervised / 100,
      anomaly_score: scores.unsupervised / 100,
      cluster_id: fraudish ? 3 : Math.floor(rand() * 3),
      cluster_fraud_rate: fraudish ? 0.42 : 0.03,
      supervised_model: { id: 'model-sup', version: 3, algorithm: 'mlp_backprop' },
      unsupervised_model: { id: 'model-uns', version: 1 },
    },
    graph: {
      distance_to_fraud: scores.graph >= 70 ? 2 : null, fraud_neighbors_1: 0, fraud_neighbors_2: scores.graph >= 70 ? 1 : 0,
      component_size: fraudish ? 7 : 1 + Math.floor(rand() * 3), shared_entity_count: fraudish ? 2 : 0, degree: fraudish ? 3 : 0,
      community_fraud_rate: fraudish ? 0.3 : 0.01,
    },
    degraded: rand() < 0.02 ? ['supervised'] : [],
    case_id: null,
    latency_ms: Math.round(20 + rand() * 80),
    persisted: true,
    created_at: ev.created_at,
  }
}

export function dataFor(pid: string): ProjectData {
  const cached = projectData.get(pid)
  if (cached) return cached
  const rules = RULES.get(pid) ?? []
  const customers: Customer[] = Array.from({ length: 60 }, (_, i) => {
    const name = `${pick(FIRST)} ${pick(LAST)}`
    return {
      id: mockId('cus'),
      external_id: `CUST-${String(1000 + i)}`,
      full_name: name,
      email: `${name.toLowerCase().replace(/\s/g, '.')}${i}@mail.test`,
      phone: `+62812${String(10000000 + Math.floor(rand() * 89999999))}`,
      risk_label: i % 17 === 3 ? 'fraud' : i % 5 === 0 ? 'legit' : 'unknown',
      status: 'active',
      registered_at: iso((10 + Math.floor(rand() * 300)) * DAY),
      stats: { events_30d: Math.floor(rand() * 40), declines_30d: Math.floor(rand() * 3), avg_score_30d: Math.round(rand() * 60) },
    }
  })
  const events: MockEvent[] = Array.from({ length: 240 }, (_, i): MockEvent => {
    const c = pick(customers)
    const type = pick(EVENT_TYPES)
    const fraudish = c.risk_label === 'fraud' || rand() < 0.08
    const occurred = iso(Math.floor(rand() * 14 * DAY))
    const id = mockId('evt')
    const amount = type === 'login' || type === 'account_change' ? null : Math.round((fraudish ? 2_000_000 + rand() * 9_000_000 : 50_000 + rand() * 1_500_000) / 100) * 100
    const event = {
      event_type: type, external_id: `TRX-${String(500000 + i)}`, occurred_at: occurred, channel: pick(CHANNELS), status: 'success',
      amount, currency: amount === null ? null : 'IDR', payment_method: type === 'transaction' ? pick(['card', 'ewallet', 'va']) : null,
      card_bin: type === 'transaction' ? '411111' : null, card_last4: type === 'transaction' ? String(1000 + Math.floor(rand() * 8999)) : null,
      issuer_country: 'ID', geo_country: fraudish && rand() > 0.5 ? 'SG' : 'ID', device_id: `dev-${Math.floor(rand() * 90)}`,
      ip_address: `36.72.${Math.floor(rand() * 255)}.${Math.floor(rand() * 255)}`, promo_code: type === 'promo_redemption' ? 'FLASH99' : null,
      discount_amount: type === 'promo_redemption' ? 50_000 : null,
    }
    const decision = type === 'login' && !fraudish ? null : makeDecision(pid, { id, external_id: event.external_id, created_at: occurred }, fraudish, rules)
    return {
      id, external_id: event.external_id, event_type: type, occurred_at: occurred, customer_id: c.id, customer_external_id: c.external_id,
      amount, currency: event.currency, channel: event.channel, decision: decision?.decision ?? null, final_score: decision?.final_score ?? null,
      data_source_id: 'ds-canonical',
      event,
      payload: { order: { id: `ORD-${i}`, items: [{ sku: `SKU-${Math.floor(rand() * 500)}`, qty: 1 + Math.floor(rand() * 3) }], voucher: event.promo_code }, user: { id: c.external_id, tier: pick(['silver', 'gold']) } },
      features: {
        amount: amount ?? 0, log_amount: Math.log1p(amount ?? 0), hour_of_day: new Date(occurred).getUTCHours(), is_night: 0,
        account_age_days: 120, cust_cnt_1h: fraudish ? 6 : 1, cust_cnt_24h: fraudish ? 14 : 3, amount_zscore_30d: fraudish ? 3.4 : 0.2,
        is_new_device: fraudish ? 1 : 0, bin_country_mismatch: event.geo_country !== 'ID' ? 1 : 0, discount_ratio: type === 'promo_redemption' ? 0.3 : 0,
        graph_distance_to_fraud: fraudish ? 2 : 99, event_type: type, channel: event.channel,
      },
      decision_detail: decision,
      labels: c.risk_label === 'fraud' && rand() > 0.5 ? [{ id: mockId('lbl'), subject_type: 'event', subject_id: id, label: 'fraud', fraud_type: pick(FRAUD_TYPES), source: 'analyst', notes: null, created_by: USERS[2]!.id, created_at: occurred }] : [],
      case_id: null,
    }
  }).sort((a, b) => b.occurred_at.localeCompare(a.occurred_at))

  const cases: MockCase[] = events.filter(e => e.decision === 'review' || e.decision === 'decline').slice(0, 25).map((e, i) => {
    const id = mockId('cas')
    e.case_id = id
    if (e.decision_detail) e.decision_detail.case_id = id
    return {
      id, customer_id: e.customer_id, customer_external_id: e.customer_external_id, event_id: e.id, decision_id: mockId('dec'),
      status: i < 12 ? 'open' : i < 18 ? 'in_review' : i < 22 ? 'resolved_fraud' : 'resolved_legit',
      priority: e.decision === 'decline' ? 1 : 3, typologies: [pick(FRAUD_TYPES)], assigned_to: i % 3 === 0 ? USERS[2]!.id : null,
      notes: i % 4 === 0 ? [{ at: e.occurred_at, by: USERS[2]!.full_name, text: 'Customer contacted, awaiting KTP verification.' }] : [],
      event_ids: [e.id], created_at: e.occurred_at, updated_at: e.occurred_at, resolved_at: i >= 18 ? e.occurred_at : null,
      final_score: e.final_score, decision: e.decision,
    }
  })
  const data = { customers, events, cases }
  projectData.set(pid, data)
  return data
}

// ---------------------------------------------------------------- rules, rulesets, lists
export const RULES = new Map<string, Rule[]>()
export const RULE_VERSIONS = new Map<string, RuleVersion[]>()
export const RULESETS = new Map<string, Ruleset[]>()
export const REF_LISTS = new Map<string, ReferenceList[]>() // key: project id or `tenant`
export const REF_ENTRIES = new Map<string, ReferenceEntry[]>()

export function ruleFromEnvelope(env: RuleEnvelope, status: Rule['status'], version = 1): Rule {
  const id = mockId('rul')
  const rule: Rule = {
    id, code: env.code, name: env.name, description: env.description ?? null, kind: env.kind, typologies: env.typologies as FraudType[],
    event_types: env.event_types, status, current_version: version, submitted_by: null, created_by: USERS[2]!.id,
    created_at: iso(20 * DAY), updated_at: iso(2 * DAY), envelope: env,
    stats_7d: { evaluated: 12000 + Math.floor(rand() * 5000), matched: Math.floor(rand() * 300), trapped: env.kind === 'velocity' ? Math.floor(rand() * 20) : 0 },
    serving: { live_version: status === 'active' ? version : null, shadow_version: status === 'shadow' ? version : null },
  }
  const versions: RuleVersion[] = Array.from({ length: version }, (_, i) => ({
    version: i + 1,
    envelope: i + 1 === version ? env : { ...env, risk_score: Math.max(5, env.risk_score - 10 * (version - i - 1)) },
    change_note: i === 0 ? 'Initial version' : 'Tuned risk score after backtest',
    created_by: USERS[2]!.id,
    created_at: iso((20 - i * 5) * DAY),
  }))
  RULE_VERSIONS.set(id, versions)
  return rule
}

for (const p of PROJECTS) {
  const statuses: Rule['status'][] = ['active', 'active', 'active', 'shadow', 'active', 'active', 'pending_approval', 'draft']
  const rules = RULE_EXAMPLES.map((env, i) => ruleFromEnvelope(env, statuses[i] ?? 'draft', i % 3 === 0 ? 2 : 1))
  RULES.set(p.id, rules)
  RULESETS.set(p.id, [
    {
      id: mockId('rst'), code: 'RS-CORE', name: 'Core protection', description: 'Main checkout ruleset', event_types: ['transaction'],
      typologies: ['carding', 'account_takeover'], aggregation: 'probabilistic_or', max_score: 100, version: 3, status: 'active',
      rules: rules.slice(0, 6).map(r => ({ rule_id: r.id, rule_code: r.code, rule_name: r.name, weight: 1, pinned_version: null })),
      created_at: iso(30 * DAY), updated_at: iso(3 * DAY),
    },
    {
      id: mockId('rst'), code: 'RS-PROMO', name: 'Promo abuse', description: 'Voucher & cashback farming', event_types: ['promo_redemption', 'transaction'],
      typologies: ['promo_abuse'], aggregation: 'sum', max_score: 100, version: 1, status: 'shadow',
      rules: rules.filter(r => r.typologies.includes('promo_abuse')).map(r => ({ rule_id: r.id, rule_code: r.code, rule_name: r.name, weight: 1.5, pinned_version: null })),
      created_at: iso(10 * DAY), updated_at: iso(1 * DAY),
    },
  ])
  const lists: ReferenceList[] = [
    { id: mockId('lst'), name: 'card_blacklist', description: 'Confirmed fraudulent cards', list_type: 'blacklist', key_kind: 'card_fingerprint', columns: [{ name: 'reason', type: 'string' }], scope: 'project', entry_count: 3, created_at: iso(30 * DAY) },
    { id: mockId('lst'), name: 'merchant_limits', description: 'Per-merchant max amount', list_type: 'lookup', key_kind: 'merchant_id', columns: [{ name: 'max_amount', type: 'number' }], scope: 'project', entry_count: 2, created_at: iso(25 * DAY) },
  ]
  REF_LISTS.set(p.id, lists)
  REF_ENTRIES.set(lists[0]!.id, [0, 1, 2].map(i => ({ id: i + 1, key: `a3f9c2${i}e1b7d0...`, attributes: { reason: 'chargeback' }, valid_from: iso(20 * DAY), valid_until: null, reason: 'Chargeback confirmed', created_at: iso(20 * DAY) })))
  REF_ENTRIES.set(lists[1]!.id, [{ id: 10, key: 'MRC-001', attributes: { max_amount: 10_000_000 }, valid_from: iso(20 * DAY), valid_until: null, reason: null, created_at: iso(20 * DAY) }, { id: 11, key: 'MRC-002', attributes: { max_amount: 2_500_000 }, valid_from: iso(20 * DAY), valid_until: null, reason: null, created_at: iso(20 * DAY) }])
}
const tenantList: ReferenceList = { id: mockId('lst'), name: 'company_blacklist_devices', description: 'Tenant-wide device blacklist', list_type: 'blacklist', key_kind: 'device_id', columns: [], scope: 'tenant', entry_count: 1, created_at: iso(40 * DAY) }
REF_LISTS.set('tenant', [tenantList])
REF_ENTRIES.set(tenantList.id, [{ id: 99, key: 'dev-13', attributes: {}, valid_from: iso(10 * DAY), valid_until: null, reason: 'Emulator farm', created_at: iso(10 * DAY) }])

// ---------------------------------------------------------------- ML
const MLP_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  properties: {
    hidden_layers: { type: 'array', title: 'Hidden layers', description: 'Neurons per hidden layer', items: { type: 'integer', minimum: 1, maximum: 1024 }, minItems: 1, maxItems: 6, default: [64, 32] },
    activation: { type: 'string', enum: ['relu', 'gelu', 'tanh'], default: 'relu' },
    dropout: { type: 'number', minimum: 0, maximum: 0.9, default: 0.2 },
    batch_norm: { type: 'boolean', default: true },
    lr: { type: 'number', title: 'Learning rate', exclusiveMinimum: 0, maximum: 1, default: 0.001 },
    epochs: { type: 'integer', minimum: 1, maximum: 1000, default: 50 },
    batch_size: { type: 'integer', minimum: 8, maximum: 8192, default: 256 },
    patience: { type: 'integer', minimum: 1, maximum: 100, default: 5, description: 'Early stopping patience (epochs without val PR-AUC improvement)' },
    pos_weight: { type: ['number', 'null'], minimum: 0, default: null, description: 'Class weight for fraud; null = auto (neg/pos ratio)' },
  },
}

export const ALGORITHMS: MlAlgorithm[] = [
  { name: 'mlp_backprop', kind: 'supervised', version: '1.0.0', display_name: 'Neural network (MLP, backprop)', description: 'PyTorch multilayer perceptron with early stopping on validation PR-AUC.', param_schema: MLP_SCHEMA, source: 'builtin', status: 'available', error: null },
  { name: 'logistic_regression', kind: 'supervised', version: '1.0.0', display_name: 'Logistic regression', description: 'Balanced-class baseline.', param_schema: { type: 'object', properties: { C: { type: 'number', exclusiveMinimum: 0, default: 1 }, max_iter: { type: 'integer', minimum: 10, default: 500 } } }, source: 'builtin', status: 'available', error: null },
  { name: 'gradient_boosting', kind: 'supervised', version: '1.0.0', display_name: 'Gradient boosting', description: 'HistGradientBoostingClassifier.', param_schema: { type: 'object', properties: { max_iter: { type: 'integer', minimum: 10, default: 300 }, learning_rate: { type: 'number', exclusiveMinimum: 0, maximum: 1, default: 0.1 }, max_depth: { type: ['integer', 'null'], minimum: 1, default: null } } }, source: 'builtin', status: 'available', error: null },
  { name: 'random_forest', kind: 'supervised', version: '1.0.0', display_name: 'Random forest', description: null, param_schema: { type: 'object', properties: { n_estimators: { type: 'integer', minimum: 10, default: 300 }, max_depth: { type: ['integer', 'null'], default: null } } }, source: 'builtin', status: 'available', error: null },
  { name: 'isolation_forest', kind: 'anomaly', version: '1.0.0', display_name: 'Isolation forest', description: 'Tree-based anomaly detector.', param_schema: { type: 'object', properties: { n_estimators: { type: 'integer', minimum: 10, maximum: 2000, default: 200 }, contamination: { type: ['number', 'string'], default: 'auto', description: '"auto" or fraction (0, 0.5]' } } }, source: 'builtin', status: 'available', error: null },
  { name: 'local_outlier_factor', kind: 'anomaly', version: '1.0.0', display_name: 'Local outlier factor', description: null, param_schema: { type: 'object', properties: { n_neighbors: { type: 'integer', minimum: 2, default: 20 } } }, source: 'builtin', status: 'available', error: null },
  { name: 'autoencoder', kind: 'anomaly', version: '1.0.0', display_name: 'Autoencoder', description: 'Reconstruction-error anomaly score.', param_schema: { type: 'object', properties: { bottleneck: { type: 'integer', minimum: 2, default: 8 }, epochs: { type: 'integer', minimum: 1, default: 30 } } }, source: 'builtin', status: 'available', error: null },
  { name: 'hdbscan', kind: 'clustering', version: '1.0.0', display_name: 'HDBSCAN', description: 'Density clustering; noise = -1.', param_schema: { type: 'object', properties: { min_cluster_size: { type: 'integer', minimum: 2, default: 15 }, min_samples: { type: ['integer', 'null'], minimum: 1, default: null } } }, source: 'builtin', status: 'available', error: null },
  { name: 'kmeans', kind: 'clustering', version: '1.0.0', display_name: 'K-means', description: null, param_schema: { type: 'object', properties: { n_clusters: { type: 'integer', minimum: 2, maximum: 100, default: 8 } } }, source: 'builtin', status: 'available', error: null },
  { name: 'dbscan', kind: 'clustering', version: '1.0.0', display_name: 'DBSCAN', description: null, param_schema: { type: 'object', properties: { eps: { type: 'number', exclusiveMinimum: 0, default: 0.5 }, min_samples: { type: 'integer', minimum: 1, default: 5 } } }, source: 'builtin', status: 'available', error: null },
  { name: 'gaussian_mixture', kind: 'clustering', version: '1.0.0', display_name: 'Gaussian mixture', description: null, param_schema: { type: 'object', properties: { n_components: { type: 'integer', minimum: 1, default: 6 } } }, source: 'builtin', status: 'available', error: null },
  { name: 'knn_anomaly', kind: 'anomaly', version: '0.1.0', display_name: 'kNN distance anomaly (plugin)', description: 'Example external plugin from /plugins.', param_schema: { type: 'object', properties: { k: { type: 'integer', minimum: 1, maximum: 200, default: 10 } } }, source: 'plugin', status: 'available', error: null },
  { name: 'broken_plugin', kind: 'supervised', version: '0.0.1', display_name: 'Broken plugin', description: null, param_schema: { type: 'object' }, source: 'plugin', status: 'invalid', error: 'smoke test failed: predict_proba returned shape (10, 2), expected (10,)' },
]

export const MODELS = new Map<string, MlModel[]>()
for (const p of PROJECTS) {
  const sup = (version: number, status: MlModel['status'], auc: number): MlModel => ({
    id: mockId('mdl'), kind: 'supervised', version, algorithms: { supervised: { name: 'mlp_backprop', version: '1.0.0' } },
    params: { hidden_layers: [64, 32], dropout: 0.2, lr: 0.001, epochs: 50 }, feature_set_version: 1,
    feature_names: ['amount', 'log_amount', 'cust_cnt_1h', 'cust_cnt_24h', 'amount_zscore_30d', 'is_new_device', 'bin_country_mismatch', 'graph_distance_to_fraud'],
    metrics: {
      roc_auc: auc, pr_auc: auc - 0.18, split: 'time', n: 19_547,
      thresholds: [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9].map(t => ({ threshold: t, precision: Math.min(0.98, 0.35 + t * 0.65), recall: Math.max(0.05, 0.98 - t * 0.9), f1: 0.55 + 0.2 * Math.sin(t * Math.PI), flagged_rate: 0.08 * (1 - t) })),
      confusion_matrix: { tp: 412, fp: 88, tn: 18_950, fn: 97 },
      calibration: Array.from({ length: 10 }, (_, i) => ({ bin_lower: i / 10, bin_upper: (i + 1) / 10, mean_predicted: (i + 0.5) / 10, observed_rate: Math.min(1, Math.max(0, (i + 0.5) / 10 + gauss() * 0.04)), count: 2000 - i * 150 })),
      feature_importance: [['amount_zscore_30d', 0.21], ['cust_cnt_1h', 0.17], ['is_new_device', 0.14], ['bin_country_mismatch', 0.12], ['graph_distance_to_fraud', 0.11], ['log_amount', 0.09]].map(([f, v]) => ({ feature: f as string, importance: v as number })),
      class_balance: { fraud: 509, legit: 19_038, fraud_rate: 0.026 },
    },
    training_history: {
      train_loss: Array.from({ length: 18 }, (_, i) => 0.6 * Math.exp(-i / 6) + 0.08),
      val_loss: Array.from({ length: 18 }, (_, i) => 0.62 * Math.exp(-i / 7) + 0.1 + (i > 14 ? (i - 14) * 0.004 : 0)),
      val_pr_auc: Array.from({ length: 18 }, (_, i) => Math.min(auc - 0.18, 0.3 + i * 0.03)),
      best_epoch: 15, epochs_run: 18,
    },
    status, progress: 1, trained_rows: 19_547, error: null,
    training_started_at: iso((10 - version) * DAY), training_finished_at: iso((10 - version) * DAY - 20 * 60_000), activated_at: status === 'active' ? iso(5 * DAY) : null,
  })
  MODELS.set(p.id, [
    sup(3, 'active', 0.94),
    sup(4, 'ready', 0.955),
    sup(2, 'archived', 0.91),
    {
      id: mockId('mdl'), kind: 'unsupervised', version: 1,
      algorithms: { anomaly: { name: 'isolation_forest', version: '1.0.0' }, clustering: { name: 'hdbscan', version: '1.0.0' } },
      params: { n_estimators: 200, min_cluster_size: 15 }, feature_set_version: 1, feature_names: ['amount', 'cust_cnt_24h', 'discount_ratio', 'graph_component_size'],
      metrics: { n_clusters: 6, noise_ratio: 0.07, silhouette: 0.41 }, training_history: {}, status: 'active', progress: 1, trained_rows: 48_210, error: null,
      training_started_at: iso(6 * DAY), training_finished_at: iso(6 * DAY - 9 * 60_000), activated_at: iso(6 * DAY),
    },
  ])
}

export function clustersFor(modelId: string): Cluster[] {
  const names = [null, null, 'Promo farm', 'Night-time ATO', null, 'Card testing burst']
  return [-1, 0, 1, 2, 3, 4, 5].map(cid => ({
    model_id: modelId, cluster_id: cid,
    size: cid === -1 ? 3_400 : Math.round(2_000 + rand() * 12_000),
    fraud_rate: cid === -1 ? 0.18 : cid === 2 || cid === 3 || cid === 5 ? 0.3 + rand() * 0.3 : rand() * 0.03,
    labeled_count: Math.round(rand() * 400),
    profile: { amount: Math.round(100_000 + rand() * 3_000_000), cust_cnt_24h: Math.round(1 + rand() * 15), discount_ratio: Math.round(rand() * 50) / 100, graph_component_size: Math.round(1 + rand() * 12) },
    top_features: [{ feature: 'discount_ratio', smd: 1.8, cluster_mean: 0.31, overall_mean: 0.06 }, { feature: 'cust_cnt_24h', smd: 1.2, cluster_mean: 9.4, overall_mean: 3.1 }, { feature: 'is_night', smd: -0.7, cluster_mean: 0.02, overall_mean: 0.21 }].slice(0, 2 + (cid % 2)),
    label: cid === -1 ? 'Outliers' : names[cid] ?? null,
    notes: null,
  }))
}
export const CLUSTER_LABELS = new Map<string, { label: string | null, notes: string | null }>()

export function projectionFor(pid: string, limit: number): ProjectionPoint[] {
  const events = dataFor(pid).events
  const centers = [[-4, -2], [3, 3], [0, 5], [5, -4], [-3, 4], [6, 1]] as const
  return Array.from({ length: Math.min(limit, 900) }, (_, i) => {
    const c = i % 11 === 0 ? -1 : i % centers.length
    const [cx, cy] = c === -1 ? [gauss() * 7, gauss() * 7] : centers[c]!
    const anomaly = c === -1 ? 0.6 + rand() * 0.4 : c === 3 ? 0.4 + rand() * 0.3 : rand() * 0.35
    return {
      event_id: events[i % events.length]!.id,
      x: Math.round((cx + gauss() * 0.9) * 100) / 100,
      y: Math.round((cy + gauss() * 0.9) * 100) / 100,
      cluster_id: c,
      anomaly_score: Math.round(anomaly * 1000) / 1000,
      label: c === 3 && rand() > 0.6 ? 'fraud' : rand() > 0.97 ? 'legit' : null,
    }
  })
}

export function communitiesFor(pid: string): GraphCommunity[] {
  const customers = dataFor(pid).customers
  return Array.from({ length: 12 }, (_, i) => {
    const size = 3 + Math.floor(rand() * 30)
    const fraud = i % 4 === 0 ? Math.floor(size * 0.4) : Math.floor(rand() * 2)
    return { community_id: i, size, fraud_count: fraud, fraud_rate: fraud / size, customer_ids: customers.slice(i * 3, i * 3 + 5).map(c => c.id) }
  }).sort((a, b) => b.fraud_rate - a.fraud_rate)
}

// ---------------------------------------------------------------- LLM
export const REGULATIONS: Regulation[] = (() => {
  const v1: Regulation = { id: mockId('reg'), code: 'POJK-12-2024', title: 'Penerapan Strategi Anti Fraud bagi Lembaga Jasa Keuangan', doc_type: 'regulation', issuer: 'OJK', version: 1, effective_date: '2024-07-01', supersedes_id: null, file_name: 'pojk-12-2024.pdf', status: 'superseded', chunk_count: 214, summary: 'Kewajiban LJK menerapkan strategi anti fraud: pencegahan, deteksi, investigasi, pemantauan.', error: null, created_at: iso(60 * DAY) }
  const v2: Regulation = { id: mockId('reg'), code: 'POJK-12-2024', title: 'Penerapan Strategi Anti Fraud (Perubahan)', doc_type: 'regulation', issuer: 'OJK', version: 2, effective_date: '2026-01-01', supersedes_id: v1.id, file_name: 'pojk-12-2024-perubahan.pdf', status: 'indexed', chunk_count: 231, summary: 'Menambah kewajiban pemantauan transaksi real-time dan pelaporan insiden fraud maksimal 3 hari kerja.', error: null, created_at: iso(12 * DAY) }
  v2.changes = {
    id: mockId('chg'), previous_regulation_id: v1.id, diff_summary: 'Pasal 8 dan 15 diubah; Pasal 15A baru tentang pemantauan real-time.',
    changed_sections: [
      { section: 'Pasal 8 ayat (2)', change: 'modified', before: 'LJK melakukan pemantauan transaksi secara berkala.', after: 'LJK melakukan pemantauan transaksi secara real-time untuk transaksi di atas Rp10.000.000.' },
      { section: 'Pasal 15A', change: 'added', before: null, after: 'LJK wajib menerapkan verifikasi tambahan untuk perubahan data kredensial yang diikuti transaksi dalam 24 jam.' },
      { section: 'Pasal 21', change: 'removed', before: 'Laporan fraud disampaikan paling lambat 5 hari kerja.', after: null },
    ],
  }
  const sop: Regulation = { id: mockId('reg'), code: 'SOP-VOUCHER-2026', title: 'SOP Penggunaan Voucher & Cashback', doc_type: 'internal_policy', issuer: 'internal', version: 1, effective_date: '2026-03-01', supersedes_id: null, file_name: 'sop-voucher.pdf', status: 'indexed', chunk_count: 18, summary: 'Satu voucher per perangkat/alamat/NIK; cashback maksimal Rp500.000 per bulan.', error: null, created_at: iso(30 * DAY) }
  return [v2, sop, v1]
})()
export const PROJECT_REGULATIONS = new Map<string, string[]>(PROJECTS.map(p => [p.id, [REGULATIONS[0]!.id, REGULATIONS[1]!.id]]))

export const REPORTS = new Map<string, LlmReport[]>()
export const PROPOSALS = new Map<string, Proposal[]>()
for (const p of PROJECTS) {
  const rules = RULES.get(p.id)!
  const reportId = mockId('rpt')
  REPORTS.set(p.id, [
    {
      id: reportId, report_type: 'recommend_rules', title: 'Rekomendasi rule — 30 hari terakhir', status: 'done', params: { since_days: 30, max_rules: 5 },
      content_md: '## Ringkasan\n\nTerdapat **kenaikan 38%** redeem voucher `FLASH99` dari perangkat yang sama dalam 7 hari terakhir, terkonsentrasi pada **cluster 2 (Promo farm)** dengan fraud rate 41%.\n\n## Rekomendasi\n\n1. Tambah rule composite: redeem promo sama oleh ≥3 customer dari device yang sama dalam 7 hari (lihat proposal).\n2. Turunkan threshold `RL-CARD-003` dari 3 → 2 customer per kartu: backtest menunjukkan precision naik dari 0.61 → 0.68.\n\n## Referensi regulasi\n\n- SOP-VOUCHER-2026 §3: *satu voucher per perangkat*.',
      structured: { recommendations: 2 }, model: 'qwen2.5:7b-instruct', error: null, created_at: iso(2 * DAY), finished_at: iso(2 * DAY - 90_000),
    },
    {
      id: mockId('rpt'), report_type: 'regulation_impact', title: 'Dampak perubahan POJK-12-2024 v2', status: 'done', params: { regulation_id: REGULATIONS[0]!.id },
      content_md: '## Perubahan yang berdampak\n\n- **Pasal 15A (baru)**: verifikasi tambahan bila ada perubahan kredensial diikuti transaksi dalam 24 jam → saat ini *belum ada rule* yang menutup kondisi ini secara eksplisit.\n- **Pasal 8(2)**: pemantauan real-time > Rp10 juta sudah dipenuhi oleh scoring real-time.\n',
      structured: null, model: 'qwen2.5:7b-instruct', error: null, created_at: iso(10 * DAY), finished_at: iso(10 * DAY - 60_000),
    },
    { id: mockId('rpt'), report_type: 'fraud_situation', title: 'Situasi fraud minggu ini', status: 'running', params: { since_days: 7 }, content_md: null, structured: null, model: 'qwen2.5:7b-instruct', error: null, created_at: iso(5 * 60_000), finished_at: null },
  ])
  const promoRule = rules.find(r => r.code === 'RL-PROMO-002')!
  const cardRule = rules.find(r => r.code === 'RL-CARD-003')!
  PROPOSALS.set(p.id, [
    {
      id: mockId('prp'), source: 'llm', proposal_type: 'new_rule', target_rule_id: null, target_rule_code: null,
      definition: {
        code: 'RL-ATO-15A', name: 'Credential change followed by transaction (POJK 15A)', description: 'Pasal 15A: verifikasi tambahan bila kredensial berubah lalu transaksi dalam 24 jam',
        kind: 'simple', typologies: ['account_takeover'], event_types: ['transaction', 'payout'], risk_score: 40, trapped_score: 0, action: 'force_review', on_trapped: 'ignore', missing_as_no_match: true,
        definition: { kind: 'simple', scoring: 'binary', when: { all: [
          { left: { type: 'field', path: 'features.recent_credential_change_24h' }, op: 'eq', right: { type: 'const', value: 1 } },
          { left: { type: 'field', path: 'event.amount' }, op: 'gt', right: { type: 'const', value: 1_000_000 } },
        ] } },
      },
      rationale: 'POJK-12-2024 v2 Pasal 15A mewajibkan verifikasi tambahan. Data 30 hari: 61% ATO terlabel terjadi < 24 jam setelah perubahan kredensial.',
      citations: [{ regulation_id: REGULATIONS[0]!.id, code: 'POJK-12-2024', section: 'Pasal 15A', excerpt: 'LJK wajib menerapkan verifikasi tambahan untuk perubahan data kredensial yang diikuti transaksi dalam 24 jam.' }],
      evidence: { labeled_ato_30d: 88, within_24h_of_change: 54 },
      validation: { valid: true, errors: [], referenced_fields: ['features.recent_credential_change_24h', 'event.amount'], referenced_lists: [] },
      backtest: { evaluated: 48_210, matched: 312, trapped: 0, hit_rate: 0.0065, labeled_fraud_matched: 51, labeled_legit_matched: 19, precision: 0.73, recall: 0.58, sample_matches: [], by_day: Array.from({ length: 14 }, (_, i) => ({ date: iso((14 - i) * DAY).slice(0, 10), matched: 15 + Math.floor(rand() * 15), evaluated: 3400 })) },
      report_id: reportId, llm_model: 'qwen2.5:7b-instruct', status: 'pending', created_at: iso(2 * DAY), reviewed_by: null, reviewed_at: null, review_comment: null, applied_rule_id: null,
    },
    {
      id: mockId('prp'), source: 'llm', proposal_type: 'tune_threshold', target_rule_id: cardRule.id, target_rule_code: cardRule.code,
      definition: { ...cardRule.envelope, definition: { ...(cardRule.envelope.definition as Extract<RuleEnvelope['definition'], { kind: 'velocity' }>), compare: { op: 'gte', right: { type: 'const', value: 2 } } } },
      rationale: 'Backtest 30 hari: threshold 2 menaikkan recall 0.44 → 0.57 dengan precision tetap > 0.65.',
      citations: [], evidence: { current_precision: 0.61, proposed_precision: 0.68 },
      validation: { valid: true, errors: [], referenced_fields: [], referenced_lists: [] },
      backtest: { evaluated: 48_210, matched: 540, trapped: 0, hit_rate: 0.011, labeled_fraud_matched: 90, labeled_legit_matched: 42, precision: 0.68, recall: 0.57, sample_matches: [], by_day: [] },
      report_id: reportId, llm_model: 'qwen2.5:7b-instruct', status: 'pending', created_at: iso(2 * DAY), reviewed_by: null, reviewed_at: null, review_comment: null, applied_rule_id: null,
    },
    {
      id: mockId('prp'), source: 'analyst', proposal_type: 'retire_rule', target_rule_id: promoRule.id, target_rule_code: promoRule.code, definition: null,
      rationale: 'Duplicate of the new promo farm composite rule.', citations: [], evidence: {}, validation: {}, backtest: null, report_id: null, llm_model: null,
      status: 'rejected', created_at: iso(8 * DAY), reviewed_by: USERS[3]!.id, reviewed_at: iso(7 * DAY), review_comment: 'Keep both until the new rule leaves shadow mode.', applied_rule_id: null,
    },
  ])
}

// ---------------------------------------------------------------- data sources
export const DATA_SOURCES = new Map<string, DataSource[]>()
export const MAPPINGS = new Map<string, MappingVersion[]>()
export const JOBS = new Map<string, IngestJob[]>()
for (const p of PROJECTS) {
  const canonical: DataSource = { id: mockId('dsr'), slug: 'canonical', name: 'Canonical API', description: 'Events posted in canonical shape', kind: 'internal', default_event_type: null, mode: 'score', connection: {}, inferred_schema: null, api_key_prefix: null, is_active: true, active_mapping_version: null, created_at: p.created_at }
  const webhook: DataSource = { id: mockId('dsr'), slug: 'shop-orders', name: 'Shop orders webhook', description: 'Order events from the storefront', kind: 'webhook', default_event_type: 'transaction', mode: 'score', connection: {}, inferred_schema: { fields: [
    { path: 'trx_id', inferred_type: 'string', null_ratio: 0, distinct_ratio: 1, sample_values: ['T-1001', 'T-1002'] },
    { path: 'created', inferred_type: 'datetime', datetime_format: '%d/%m/%Y %H:%M:%S', null_ratio: 0, distinct_ratio: 0.99, sample_values: ['21/09/2026 10:11:12'] },
    { path: 'total', inferred_type: 'integer', null_ratio: 0, distinct_ratio: 0.8, sample_values: [15000000, 250000] },
    { path: 'user.id', inferred_type: 'string', null_ratio: 0, distinct_ratio: 0.4, sample_values: ['U-9'] },
    { path: 'user.hp', inferred_type: 'string', null_ratio: 0.02, distinct_ratio: 0.4, sample_values: ['0812****789'], pii: 'phone' },
    { path: 'card_number', inferred_type: 'string', null_ratio: 0.3, distinct_ratio: 0.5, sample_values: ['4111********1111'], pii: 'pan' },
  ] }, api_key_prefix: 'fpk_7Hc2Qm9x', is_active: true, active_mapping_version: 2, created_at: iso(45 * DAY) }
  const file: DataSource = { id: mockId('dsr'), slug: 'legacy-export', name: 'Legacy CSV export', description: 'Historical labelled orders (2025)', kind: 'file', default_event_type: 'transaction', mode: 'load_only', connection: {}, inferred_schema: null, api_key_prefix: null, is_active: true, active_mapping_version: 1, created_at: iso(30 * DAY) }
  const pg: DataSource = { id: mockId('dsr'), slug: 'erp-payments', name: 'ERP payments (Postgres)', description: null, kind: 'postgres', default_event_type: 'payout', mode: 'score', connection: { host: 'erp-db.internal', port: 5432, database: 'erp', user: 'fraud_reader', password_env: 'SRC_ERP_DB_PASSWORD', table: 'payments', poll: { enabled: true, interval_seconds: 60, cursor_field: 'updated_at', batch_size: 500 } }, inferred_schema: null, api_key_prefix: null, is_active: true, active_mapping_version: null, created_at: iso(5 * DAY) }
  DATA_SOURCES.set(p.id, [canonical, webhook, file, pg])
  MAPPINGS.set(webhook.id, [
    { version: 1, status: 'archived', created_at: iso(45 * DAY), activated_at: iso(45 * DAY), mapping: { event: { external_id: { from: 'trx_id' }, occurred_at: { from: 'created', transform: [{ fn: 'parse_datetime', format: '%d/%m/%Y %H:%M:%S', timezone: 'Asia/Jakarta' }] }, customer_external_id: { from: 'user.id' } } } },
    { version: 2, status: 'active', created_at: iso(20 * DAY), activated_at: iso(20 * DAY), mapping: {
      event_type: { const: 'transaction' },
      event: {
        external_id: { from: 'trx_id' },
        occurred_at: { from: 'created', transform: [{ fn: 'parse_datetime', format: '%d/%m/%Y %H:%M:%S', timezone: 'Asia/Jakarta' }] },
        customer_external_id: { from: 'user.id', transform: [{ fn: 'to_string' }] },
        amount: { from: 'total', transform: [{ fn: 'to_number' }] },
        currency: { const: 'IDR' },
        instrument_fingerprint: { from: 'card_number', transform: [{ fn: 'hash_pan' }] },
        card_bin: { from: 'card_number', transform: [{ fn: 'pan_bin' }] },
      },
      customer: { phone: { from: 'user.hp', transform: [{ fn: 'normalize_phone', default_country: 'ID' }] } },
      label: null,
      drop_fields: ['card_number', 'cvv'],
    } },
  ])
  JOBS.set(file.id, [
    { id: mockId('job'), data_source_id: file.id, mode: 'load_only', status: 'done', total_rows: 120_000, processed_rows: 120_000, accepted_rows: 119_874, rejected_rows: 126, error: null, created_at: iso(30 * DAY), started_at: iso(30 * DAY), finished_at: iso(30 * DAY - 25 * 60_000) },
    { id: mockId('job'), data_source_id: file.id, mode: 'score', status: 'running', total_rows: 5_000, processed_rows: 2_150, accepted_rows: 2_140, rejected_rows: 10, error: null, created_at: iso(10 * 60_000), started_at: iso(9 * 60_000), finished_at: null },
  ])
}

// ---------------------------------------------------------------- audit
export const AUDIT: AuditEntry[] = Array.from({ length: 60 }, (_, i) => {
  const actions = ['rule.create', 'rule.submit', 'rule.approve', 'ruleset.update', 'project.settings.update', 'model.approve', 'proposal.reject', 'mapping.activate', 'case.resolve', 'auth.login']
  const action = actions[i % actions.length]!
  return {
    id: 1000 - i, occurred_at: iso(i * 5 * HOUR), actor_type: action.startsWith('auth') ? 'user' : i % 9 === 0 ? 'service' : 'user',
    actor_id: i % 9 === 0 ? 'llm-service' : USERS[(i % 3) + 1]!.email, action, subject_type: action.split('.')[0]!,
    subject_id: `${action.split('.')[0]}-${i}`, before: i % 2 ? { status: 'draft' } : null, after: { status: i % 2 ? 'pending_approval' : 'active' },
    metadata: { project: PROJECTS[i % PROJECTS.length]!.slug }, request_id: `req-${(i * 7919).toString(16)}`,
  }
})
