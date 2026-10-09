// In-process mock of the gateway API (MOCK_API=1). Lets the UI be demoed, developed and smoke-tested without any
// backend. Stateful per process: creating/approving rules, resolving cases etc. changes what later GETs return.
// It intentionally follows docs/technical/api-contract.md shapes; it does NOT emulate authorisation rules exhaustively.
import type {
  BacktestResult, Case, DataSource, IngestError, InspectResult, MappingPreviewRow, MappingSpec, Page, Project, Rule,
  RuleStatus, Ruleset,
} from '#shared/types/api'
import type { RuleEnvelope } from '#shared/rules/dsl'
import { describeDefinition } from '#shared/rules/model'
import {
  ALGORITHMS, AUDIT, CLUSTER_LABELS, DATA_SOURCES, DEFAULT_SETTINGS, JOBS, MAPPINGS, MEMBERS, MODELS, PROJECTS,
  PROJECT_REGULATIONS, PROPOSALS, REF_ENTRIES, REF_LISTS, REGULATIONS, REPORTS, RULESETS, RULES, RULE_VERSIONS, SETTINGS,
  TENANT, TENANT_2, USERS, clustersFor, type MockEvent, communitiesFor, dataFor, iso, mockId, projectionFor, ruleFromEnvelope,
} from './data'
import { evaluateFormula, FormulaError } from './formula'

interface Ctx {
  method: string
  path: string
  params: string[]
  query: URLSearchParams
  body: unknown
  headers: Record<string, string>
  rawBody: Uint8Array | null
}

type Handler = (ctx: Ctx) => unknown | Promise<unknown>

class MockHttpError extends Error {
  constructor(public status: number, public title: string, public detail?: string, public errors: { field: string, message: string }[] = [], public extra: Record<string, unknown> = {}) {
    super(title)
  }
}

const routes: { method: string, re: RegExp, handler: Handler }[] = []
function on(method: string, pattern: string, handler: Handler) {
  // pattern: "projects/:pid/rules/:id" → regex with capture groups
  const re = new RegExp(`^${pattern.replace(/:[a-z_]+/g, '([^/]+)')}$`)
  routes.push({ method, re, handler })
}

function paginate<T>(items: T[], q: URLSearchParams): Page<T> {
  const page = Math.max(1, Number(q.get('page') ?? 1))
  const pageSize = Math.min(200, Math.max(1, Number(q.get('page_size') ?? 50)))
  return { items: items.slice((page - 1) * pageSize, page * pageSize), total: items.length, page, page_size: pageSize }
}

/** Canonical events-table row for a mock event (GET /events/{id} → event, case detail → event). */
function canonicalEvent(pid: string, e: MockEvent) {
  return { id: e.id, project_id: pid, external_id: e.external_id, event_type: e.event_type, occurred_at: e.occurred_at, customer_id: e.customer_id, data_source_id: e.data_source_id, ...e.event }
}

function project(pid: string): Project {
  const p = PROJECTS.find(x => x.id === pid)
  if (!p) throw new MockHttpError(404, 'Not found', 'project not found')
  return p
}

function obj(body: unknown): Record<string, unknown> {
  return typeof body === 'object' && body !== null ? body as Record<string, unknown> : {}
}

function currentUser(ctx: Ctx) {
  const token = (ctx.headers.authorization ?? '').replace(/^Bearer\s+/i, '')
  const payload = token.split('.')[1]
  let email = USERS[1]!.email
  if (payload) {
    try { email = JSON.parse(Buffer.from(payload, 'base64').toString('utf8')).email ?? email }
    catch { /* default user */ }
  }
  return USERS.find(u => u.email === email) ?? USERS[1]!
}

function fakeJwt(email: string, ttlSeconds = 3600) {
  const b64 = (o: unknown) => Buffer.from(JSON.stringify(o)).toString('base64url')
  return `${b64({ alg: 'none', typ: 'JWT' })}.${b64({ sub: email, email, exp: Math.floor(Date.now() / 1000) + ttlSeconds })}.mock`
}

function roleFor(email: string): 'project_admin' | 'approver' | 'analyst' | 'viewer' {
  if (email.startsWith('analyst')) return 'analyst'
  if (email.startsWith('approver')) return 'approver'
  if (email.startsWith('viewer')) return 'viewer'
  return 'project_admin'
}

// ---------------------------------------------------------------- auth & identity
on('POST', 'auth/login', (ctx) => {
  const { email, password } = obj(ctx.body) as { email?: string, password?: string }
  const user = USERS.find(u => u.email === email)
  if (!user || !password || password === 'wrong') throw new MockHttpError(401, 'Invalid credentials', 'Email or password is incorrect.')
  return { access_token: fakeJwt(user.email), token_type: 'Bearer', expires_in: 3600, refresh_token: `refresh-${user.email}-${Date.now()}`, user }
})
on('POST', 'auth/refresh', (ctx) => {
  const rt = String(obj(ctx.body).refresh_token ?? '')
  const email = rt.split('-').slice(1, -1).join('-') || USERS[1]!.email
  const user = USERS.find(u => u.email === email) ?? USERS[1]!
  return { access_token: fakeJwt(user.email), token_type: 'Bearer', expires_in: 3600, refresh_token: `refresh-${user.email}-${Date.now()}`, user }
})
on('POST', 'auth/logout', () => ({ ok: true }))
on('GET', 'me', (ctx) => {
  const user = currentUser(ctx)
  const projects = user.is_platform_admin ? [] : PROJECTS.map(p => ({ id: p.id, slug: p.slug, name: p.name, stage: p.stage, status: p.status, role: user.tenant_role === 'tenant_admin' ? 'project_admin' as const : roleFor(user.email) }))
  return { user, tenant: user.tenant_id ? TENANT : null, projects }
})

// ---------------------------------------------------------------- tenants
const TENANTS = [TENANT, TENANT_2]
on('GET', 'tenants', ctx => paginate(TENANTS, ctx.query))
on('POST', 'tenants', (ctx) => {
  const b = obj(ctx.body)
  const t = { id: mockId('tnt'), slug: String(b.slug), name: String(b.name), status: 'active' as const, created_at: new Date().toISOString() }
  TENANTS.push(t)
  return t
})
on('GET', 'tenants/:tid', ctx => TENANTS.find(t => t.id === ctx.params[0]) ?? (() => { throw new MockHttpError(404, 'Not found') })())
on('PATCH', 'tenants/:tid', (ctx) => {
  const t = TENANTS.find(x => x.id === ctx.params[0])
  if (!t) throw new MockHttpError(404, 'Not found')
  Object.assign(t, obj(ctx.body))
  return t
})
on('GET', 'tenants/:tid/users', ctx => paginate(USERS.filter(u => u.tenant_id === ctx.params[0]), ctx.query))
on('POST', 'tenants/:tid/users', (ctx) => {
  const b = obj(ctx.body)
  const u = { id: mockId('usr'), email: String(b.email), full_name: String(b.full_name), tenant_id: ctx.params[0]!, tenant_role: (b.tenant_role as 'member') ?? 'member', is_platform_admin: false, is_active: true }
  USERS.push(u)
  return u
})
on('PATCH', 'tenants/:tid/users/:uid', (ctx) => {
  const u = USERS.find(x => x.id === ctx.params[1])
  if (!u) throw new MockHttpError(404, 'Not found')
  Object.assign(u, obj(ctx.body))
  return u
})
on('GET', 'tenants/:tid/audit', ctx => paginate(AUDIT, ctx.query))

// regulations (tenant library)
on('GET', 'tenants/:tid/regulations', ctx => paginate(REGULATIONS, ctx.query))
on('POST', 'tenants/:tid/regulations', (ctx) => {
  const form = parseMultipartFields(ctx)
  const r = {
    id: mockId('reg'), code: form.code ?? 'NEW-DOC', title: form.title ?? 'Uploaded document', doc_type: (form.doc_type ?? 'regulation') as 'regulation',
    issuer: form.issuer ?? 'internal', version: 1, effective_date: form.effective_date || null, supersedes_id: form.supersedes_id || null,
    file_name: form.__filename ?? 'document.pdf', status: 'processing' as const, chunk_count: 0, summary: null, error: null, created_at: new Date().toISOString(),
  }
  REGULATIONS.unshift(r)
  setTimeout(() => { r.status = 'indexed' as never; r.chunk_count = 42 }, 4000)
  return { regulation_id: r.id, status: 'processing' }
})
on('GET', 'tenants/:tid/regulations/:id', ctx => REGULATIONS.find(r => r.id === ctx.params[1]) ?? (() => { throw new MockHttpError(404, 'Not found') })())
on('DELETE', 'tenants/:tid/regulations/:id', (ctx) => {
  const i = REGULATIONS.findIndex(r => r.id === ctx.params[1])
  if (i >= 0) REGULATIONS.splice(i, 1)
  return null
})

// tenant-wide reference lists
on('GET', 'tenants/:tid/reference-lists', () => REF_LISTS.get('tenant') ?? [])

// ---------------------------------------------------------------- projects
on('GET', 'projects', (ctx) => {
  const user = currentUser(ctx)
  return paginate(user.is_platform_admin ? [] : PROJECTS.filter(p => p.status === 'active' || ctx.query.get('include_archived')), ctx.query)
})
on('POST', 'projects', (ctx) => {
  const b = obj(ctx.body) as Partial<Project> & { template?: string }
  if (!b.slug || !/^[a-z0-9][a-z0-9-]{1,62}$/.test(b.slug)) throw new MockHttpError(422, 'Validation failed', undefined, [{ field: 'slug', message: 'must match ^[a-z0-9][a-z0-9-]{1,62}$' }])
  if (PROJECTS.some(p => p.slug === b.slug)) throw new MockHttpError(409, 'Conflict', 'slug already used in this tenant')
  const base = PROJECTS[0]!
  const p: Project = {
    ...structuredClone(base), id: mockId('prj'), slug: b.slug, name: b.name ?? b.slug, description: b.description ?? null, stage: b.stage ?? 'custom',
    business_context: b.business_context ?? null, ml_config: b.ml_config ?? base.ml_config, llm_config: b.llm_config ?? base.llm_config,
    graph_config: b.graph_config ?? base.graph_config, created_at: new Date().toISOString(),
  }
  PROJECTS.push(p)
  SETTINGS[p.id] = DEFAULT_SETTINGS()
  MEMBERS[p.id] = [...(MEMBERS[base.id] ?? [])]
  RULES.set(p.id, b.template && b.template !== 'none' ? (RULES.get(base.id) ?? []).slice(0, 4).map(r => ruleFromEnvelope(r.envelope, 'active')) : [])
  RULESETS.set(p.id, [])
  REF_LISTS.set(p.id, [])
  DATA_SOURCES.set(p.id, [])
  MODELS.set(p.id, [])
  REPORTS.set(p.id, [])
  PROPOSALS.set(p.id, [])
  return p
})
on('GET', 'projects/:pid', (ctx) => {
  const p = project(ctx.params[0]!)
  const d = dataFor(p.id)
  return { ...p, summary: { events_30d: d.events.length * 120, open_cases: d.cases.filter(c => c.status === 'open').length, active_rules: (RULES.get(p.id) ?? []).filter(r => r.status === 'active').length, active_models: (MODELS.get(p.id) ?? []).filter(m => m.status === 'active').length } }
})
on('PATCH', 'projects/:pid', (ctx) => {
  const p = project(ctx.params[0]!)
  Object.assign(p, obj(ctx.body))
  return p
})
on('POST', 'projects/:pid/archive', (ctx) => {
  const p = project(ctx.params[0]!)
  p.status = 'archived'
  return p
})
on('GET', 'projects/:pid/members', ctx => ({ items: (MEMBERS[ctx.params[0]!] ?? []).map(m => ({ ...m, is_active: true })) }))
on('PUT', 'projects/:pid/members', (ctx) => {
  const b = obj(ctx.body) as { user_id: string, role: 'viewer' }
  const list = MEMBERS[ctx.params[0]!] ?? (MEMBERS[ctx.params[0]!] = [])
  const u = USERS.find(x => x.id === b.user_id)
  if (!u) throw new MockHttpError(404, 'Not found', 'user not found')
  const existing = list.find(m => m.user_id === b.user_id)
  if (existing) existing.role = b.role
  else list.push({ user_id: u.id, email: u.email, full_name: u.full_name, role: b.role })
  return list
})
on('DELETE', 'projects/:pid/members/:uid', (ctx) => {
  MEMBERS[ctx.params[0]!] = (MEMBERS[ctx.params[0]!] ?? []).filter(m => m.user_id !== ctx.params[1])
  return null
})
on('GET', 'projects/:pid/settings', ctx => SETTINGS[ctx.params[0]!] ?? DEFAULT_SETTINGS())
on('PUT', 'projects/:pid/settings/:key', (ctx) => {
  const s = SETTINGS[ctx.params[0]!] ?? DEFAULT_SETTINGS()
  const key = ctx.params[1] as keyof typeof s
  if (key === 'engine_combination' && !['noisy_or', 'weighted_average'].includes(ctx.body as string))
    throw new MockHttpError(422, 'Validation failed', 'engine_combination must be "noisy_or" or "weighted_average"')
  if (key === 'engine_weights') {
    const w = obj(ctx.body) as Record<string, number>
    if (Object.values(w).some(v => typeof v !== 'number' || v < 0)) throw new MockHttpError(422, 'Validation failed', 'weights must be non-negative numbers')
  }
  if (key === 'decision_thresholds') {
    const t = obj(ctx.body) as { review: number, decline: number }
    if (!(t.review < t.decline)) throw new MockHttpError(422, 'Validation failed', undefined, [{ field: 'review', message: 'review threshold must be below decline threshold' }])
  }
  ;(s as unknown as Record<string, unknown>)[key] = ctx.body
  SETTINGS[ctx.params[0]!] = s
  return s
})

// ---------------------------------------------------------------- analytics & audit
on('GET', 'projects/:pid/analytics/overview', (ctx) => {
  const d = dataFor(ctx.params[0]!)
  const decided = d.events.filter(e => e.decision)
  const days = Array.from({ length: 14 }, (_, i) => iso((13 - i) * 86_400_000).slice(0, 10))
  return {
    totals: { events: d.events.length * 120, approve: decided.filter(e => e.decision === 'approve').length * 120, review: decided.filter(e => e.decision === 'review').length * 120, decline: decided.filter(e => e.decision === 'decline').length * 120 },
    by_event_type: ['transaction', 'login', 'promo_redemption', 'account_change', 'payout'].map(t => ({ event_type: t, count: d.events.filter(e => e.event_type === t).length * 120 })),
    by_label_fraud_type: [{ fraud_type: 'carding', count: 41 }, { fraud_type: 'account_takeover', count: 28 }, { fraud_type: 'promo_abuse', count: 63 }, { fraud_type: 'bank_account_takeover', count: 9 }, { fraud_type: 'refund_abuse', count: 12 }],
    daily: days.map((date, i) => ({ date, events: 3200 + Math.round(Math.sin(i / 2) * 400) + i * 30, review: 90 + (i % 5) * 12, decline: 25 + (i % 3) * 8, avg_score: 18 + (i % 4) * 2.5 })),
    score_histogram: Array.from({ length: 10 }, (_, i) => ({ bucket: i * 10, count: Math.round(9000 * Math.exp(-i / 1.6)) + (i > 7 ? 60 : 0) })),
    from: days[0], to: days[days.length - 1],
    engine_avg: { rules: 21.4, supervised: 17.9, unsupervised: 24.2, graph: 6.1 },
    open_cases: d.cases.filter(c => c.status === 'open').length,
    degraded_rate: 0.004,
  }
})
on('GET', 'projects/:pid/analytics/drift', () => ({
  recent_window: { from: iso(7 * 86_400_000), to: iso(0) },
  baseline_window: { from: iso(37 * 86_400_000), to: iso(7 * 86_400_000) },
  items: [
    { feature: 'amount', psi: 0.04, recent_mean: 812_000, baseline_mean: 790_500, status: 'stable' },
    { feature: 'discount_ratio', psi: 0.27, recent_mean: 0.19, baseline_mean: 0.08, status: 'significant' },
    { feature: 'cust_cnt_24h', psi: 0.12, recent_mean: 4.2, baseline_mean: 3.6, status: 'moderate' },
    { feature: 'is_new_device', psi: 0.06, recent_mean: 0.14, baseline_mean: 0.12, status: 'stable' },
    { feature: 'graph_component_size', psi: 0.18, recent_mean: 2.9, baseline_mean: 2.1, status: 'moderate' },
  ].map(r => ({ ...r, recent_n: 3200, baseline_n: 14_800 })),
}))
on('GET', 'projects/:pid/analytics/typologies', () => [
  { fraud_type: 'promo_abuse', week: '2026-W37', count: 31, amount: 15_400_000 },
  { fraud_type: 'carding', week: '2026-W37', count: 18, amount: 74_900_000 },
])
on('GET', 'projects/:pid/audit', (ctx) => {
  const p = project(ctx.params[0]!)
  const q = ctx.query
  const rows = AUDIT.filter(a => (!q.get('action') || a.action.includes(q.get('action')!)) && (!q.get('actor') || (a.actor_id ?? '').includes(q.get('actor')!)))
    .map(a => ({ ...a, metadata: { ...a.metadata, project: p.slug } }))
  return paginate(rows, q)
})

// ---------------------------------------------------------------- events, customers, cases, labels
on('GET', 'projects/:pid/events', (ctx) => {
  const q = ctx.query
  const d = dataFor(ctx.params[0]!)
  const needle = (q.get('q') ?? '').toLowerCase()
  const items = d.events.filter(e =>
    (!q.get('event_type') || e.event_type === q.get('event_type'))
    && (!q.get('decision') || e.decision === q.get('decision'))
    && (!q.get('customer_id') || e.customer_id === q.get('customer_id'))
    && (!q.get('min_score') || (e.final_score ?? 0) >= Number(q.get('min_score')))
    && (!needle || e.external_id.toLowerCase().includes(needle) || e.customer_external_id.toLowerCase().includes(needle)))
  return paginate(items.map(({ event: _e, payload: _p, features: _f, decision_detail: _d, labels: _l, case_id: _c, ...summary }) => summary), q)
})
on('GET', 'projects/:pid/events/:id', (ctx) => {
  const d = dataFor(ctx.params[0]!)
  const e = d.events.find(x => x.id === ctx.params[1])
  if (!e) throw new MockHttpError(404, 'Not found', 'event not found')
  const c = d.cases.find(x => x.id === e.case_id)
  return {
    event: canonicalEvent(ctx.params[0]!, e),
    source: e.payload,
    features: e.features,
    customer: d.customers.find(x => x.id === e.customer_id) ?? null,
    decision: e.decision_detail,
    labels: e.labels,
    case: c ? { id: c.id, status: c.status, priority: c.priority } : null,
  }
})
on('POST', 'projects/:pid/events/:id/rescore', (ctx) => {
  const e = dataFor(ctx.params[0]!).events.find(x => x.id === ctx.params[1])
  if (!e?.decision_detail) throw new MockHttpError(404, 'Not found')
  e.decision_detail = { ...e.decision_detail, latency_ms: 37, created_at: new Date().toISOString() }
  return e.decision_detail
})
on('GET', 'projects/:pid/decisions/:eid', (ctx) => {
  const e = dataFor(ctx.params[0]!).events.find(x => x.id === ctx.params[1])
  if (!e?.decision_detail) throw new MockHttpError(404, 'Not found')
  return e.decision_detail
})
on('GET', 'projects/:pid/customers', (ctx) => {
  const needle = (ctx.query.get('q') ?? '').toLowerCase()
  return paginate(dataFor(ctx.params[0]!).customers.filter(c => !needle || c.external_id.toLowerCase().includes(needle) || (c.full_name ?? '').toLowerCase().includes(needle)), ctx.query)
})
on('GET', 'projects/:pid/customers/:id', (ctx) => {
  const c = dataFor(ctx.params[0]!).customers.find(x => x.id === ctx.params[1])
  if (!c) throw new MockHttpError(404, 'Not found')
  return c
})
on('GET', 'projects/:pid/customers/:id/events', ctx => paginate(dataFor(ctx.params[0]!).events.filter(e => e.customer_id === ctx.params[1]), ctx.query))
on('GET', 'projects/:pid/cases', (ctx) => {
  const q = ctx.query
  const d = dataFor(ctx.params[0]!)
  const items = d.cases.filter(c => (!q.get('status') || c.status === q.get('status')) && (!q.get('priority') || String(c.priority) === q.get('priority')) && (!q.get('typology') || c.typologies.includes(q.get('typology') as never)))
  return paginate(items.map(c => ({
    id: c.id, status: c.status, priority: c.priority, typologies: c.typologies, assigned_to: c.assigned_to, customer_id: c.customer_id,
    customer_external_id: c.customer_external_id, event_id: c.event_id, event_count: c.event_ids.length, decision: c.decision,
    final_score: c.final_score, risk_label: d.customers.find(x => x.id === c.customer_id)?.risk_label ?? 'unknown',
    created_at: c.created_at, updated_at: c.updated_at, resolved_at: c.resolved_at,
  })), q)
})
on('GET', 'projects/:pid/cases/:id', (ctx) => {
  const d = dataFor(ctx.params[0]!)
  const c = d.cases.find(x => x.id === ctx.params[1])
  if (!c) throw new MockHttpError(404, 'Not found')
  const event = d.events.find(e => e.id === c.event_id) ?? null
  const { customer_external_id: _x, final_score: _f, decision: _d, ...record } = c
  return {
    case: record,
    customer: d.customers.find(x => x.id === c.customer_id) ?? null,
    event: event ? canonicalEvent(ctx.params[0]!, event) : null,
    decision: event?.decision_detail ?? null,
    graph: event?.decision_detail?.graph ?? null,
  }
})
on('PATCH', 'projects/:pid/cases/:id', (ctx) => {
  const c = dataFor(ctx.params[0]!).cases.find(x => x.id === ctx.params[1])
  if (!c) throw new MockHttpError(404, 'Not found')
  const b = obj(ctx.body) as Partial<Case> & { note?: string }
  if (b.note) c.notes.push({ at: new Date().toISOString(), by: currentUser(ctx).full_name, text: b.note })
  if (b.status) c.status = b.status
  if (b.priority) c.priority = b.priority
  if (b.assigned_to !== undefined) c.assigned_to = b.assigned_to
  c.updated_at = new Date().toISOString()
  return c
})
on('POST', 'projects/:pid/cases/:id/resolve', (ctx) => {
  const c = dataFor(ctx.params[0]!).cases.find(x => x.id === ctx.params[1])
  if (!c) throw new MockHttpError(404, 'Not found')
  const b = obj(ctx.body) as { label: 'fraud' | 'legit', fraud_type?: string }
  c.status = b.label === 'fraud' ? 'resolved_fraud' : 'resolved_legit'
  c.resolved_at = new Date().toISOString()
  return c
})
on('POST', 'projects/:pid/labels', ctx => ({ id: mockId('lbl'), ...obj(ctx.body), created_at: new Date().toISOString() }))
on('GET', 'projects/:pid/labels', ctx => paginate(dataFor(ctx.params[0]!).events.flatMap(e => e.labels), ctx.query))
on('POST', 'projects/:pid/score/simulate', (ctx) => {
  const d = dataFor(ctx.params[0]!)
  const sample = d.events.find(e => e.decision_detail)!.decision_detail!
  return { ...sample, event_id: 'simulated', external_id: String(obj(obj(ctx.body).event).external_id ?? 'SIM-1'), persisted: false }
})

// ---------------------------------------------------------------- rules
function rules(pid: string): Rule[] {
  project(pid)
  return RULES.get(pid) ?? []
}
function rule(pid: string, id: string): Rule {
  const r = rules(pid).find(x => x.id === id)
  if (!r) throw new MockHttpError(404, 'Not found', 'rule not found')
  return r
}
function validateEnvelope(env: RuleEnvelope) {
  const errors: { path: string, message: string }[] = []
  if (!/^[A-Z0-9-]{3,40}$/.test(env.code ?? '')) errors.push({ path: 'code', message: 'must match ^[A-Z0-9-]{3,40}$' })
  if (!env.name) errors.push({ path: 'name', message: 'required' })
  if (env.definition?.kind !== env.kind) errors.push({ path: 'definition.kind', message: 'must equal envelope kind' })
  if (env.definition?.kind === 'reference' && !['card_blacklist', 'merchant_limits', 'company_blacklist_devices'].includes(env.definition.list))
    errors.push({ path: 'definition.list', message: `unknown reference list '${env.definition.list}'` })
  const json = JSON.stringify(env.definition ?? {})
  const fields = [...json.matchAll(/"type":"field","path":"([^"]+)"/g)].map(m => m[1]!)
  for (const f of fields) if (!/^(event|source|customer|features|ml|graph)\./.test(f)) errors.push({ path: 'definition', message: `unknown field path '${f}'` })
  return { valid: errors.length === 0, errors, referenced_fields: [...new Set(fields)], referenced_lists: env.definition?.kind === 'reference' ? [env.definition.list] : [] }
}
function backtest(seed: number): BacktestResult {
  const evaluated = 48_210
  const matched = 200 + (seed % 400)
  const fraud = Math.round(matched * 0.6)
  return {
    evaluated, matched, trapped: seed % 7, hit_rate: matched / evaluated, labeled_fraud_matched: fraud, labeled_legit_matched: Math.round(matched * 0.2),
    precision: 0.75, recall: 0.42, sample_matches: dataFor(PROJECTS[0]!.id).events.slice(0, 5).map(e => e.id),
    by_day: Array.from({ length: 30 }, (_, i) => ({ date: iso((30 - i) * 86_400_000).slice(0, 10), matched: Math.round(matched / 30 + Math.sin(i) * 4), evaluated: Math.round(evaluated / 30) })),
    score_histogram: Array.from({ length: 10 }, (_, i) => ({ bucket: `${i * 10}-${i * 10 + 10}`, count: Math.round(5000 * Math.exp(-i / 1.8)) })),
    decision_distribution: { approve: 46_900, review: 1_050, decline: 260 },
  }
}
function transition(pid: string, id: string, status: RuleStatus, ctx: Ctx) {
  const r = rule(pid, id)
  const b = obj(ctx.body) as { target_status?: RuleStatus }
  const target = status === 'active' ? (b.target_status ?? 'active') : status
  if (status === 'active' && r.status !== 'pending_approval') throw new MockHttpError(409, 'Conflict', 'only pending_approval rules can be approved')
  if (status === 'active' && r.submitted_by === currentUser(ctx).id) throw new MockHttpError(403, 'Forbidden', 'maker–checker: the approver must differ from the submitter')
  if (status === 'pending_approval') r.submitted_by = currentUser(ctx).id
  r.status = target
  r.updated_at = new Date().toISOString()
  return r
}

on('GET', 'projects/:pid/rules', (ctx) => {
  const q = ctx.query
  const needle = (q.get('q') ?? '').toLowerCase()
  return paginate(rules(ctx.params[0]!).filter(r =>
    (!q.get('kind') || r.kind === q.get('kind')) && (!q.get('status') || r.status === q.get('status'))
    && (!q.get('typology') || r.typologies.includes(q.get('typology') as never))
    && (!needle || r.code.toLowerCase().includes(needle) || r.name.toLowerCase().includes(needle))), q)
})
on('POST', 'projects/:pid/rules/validate', ctx => validateEnvelope(ctx.body as RuleEnvelope))
on('POST', 'projects/:pid/rules/test', (ctx) => {
  const env = obj(ctx.body).rule as RuleEnvelope | undefined
  if (!env) throw new MockHttpError(422, 'Validation failed', 'rule is required')
  const v = validateEnvelope(env)
  if (!v.valid) throw new MockHttpError(422, 'Validation failed', undefined, v.errors.map(e => ({ field: e.path, message: e.message })))
  const trapped = env.kind === 'velocity' && env.definition.kind === 'velocity' && !!env.definition.statistic
  return trapped
    ? { outcome: 'trapped', contribution: env.on_trapped === 'score' ? env.trapped_score ?? 0 : 0, trapped_reason: 'insufficient_history (3 < min_samples 5)', trace: { samples: 3 } }
    : { outcome: 'match', contribution: env.risk_score, trapped_reason: null, trace: { summary: describeDefinition(env.definition), left: 6_250_000, right: 5_000_000 } }
})
on('POST', 'projects/:pid/rules/backtest', () => backtest(123))
on('GET', 'projects/:pid/rules/performance', ctx => ({ since_days: 7, items: rules(ctx.params[0]!).map(r => ({
  rule_id: r.id, code: r.code, status: r.status, evaluated: r.stats_7d?.evaluated ?? 0, matched: r.stats_7d?.matched ?? 0, trapped: r.stats_7d?.trapped ?? 0,
  hit_rate: (r.stats_7d?.matched ?? 0) / Math.max(1, r.stats_7d?.evaluated ?? 1), precision: 0.4 + (r.code.length % 5) / 10, last_hit_at: r.updated_at,
})) }))
on('POST', 'projects/:pid/rules', (ctx) => {
  const env = ctx.body as RuleEnvelope
  const v = validateEnvelope(env)
  if (!v.valid) throw new MockHttpError(422, 'Validation failed', undefined, v.errors.map(e => ({ field: e.path, message: e.message })))
  if (rules(ctx.params[0]!).some(r => r.code === env.code)) throw new MockHttpError(409, 'Conflict', `rule code ${env.code} already exists`)
  const r = ruleFromEnvelope(env, 'draft')
  r.stats_7d = { evaluated: 0, matched: 0, trapped: 0 }
  rules(ctx.params[0]!).unshift(r)
  return r
})
on('GET', 'projects/:pid/rules/:id', (ctx) => {
  const r = rule(ctx.params[0]!, ctx.params[1]!)
  return { ...r, versions: RULE_VERSIONS.get(r.id) ?? [] }
})
on('PUT', 'projects/:pid/rules/:id', (ctx) => {
  const r = rule(ctx.params[0]!, ctx.params[1]!)
  if (r.status === 'retired') throw new MockHttpError(409, 'Conflict', 'retired rules cannot be edited')
  const env = ctx.body as RuleEnvelope & { change_note?: string }
  const v = validateEnvelope(env)
  if (!v.valid) throw new MockHttpError(422, 'Validation failed', undefined, v.errors.map(e => ({ field: e.path, message: e.message })))
  const { change_note, ...clean } = env
  r.current_version += 1
  r.envelope = clean
  r.name = clean.name
  r.description = clean.description ?? null
  r.status = 'draft'
  r.updated_at = new Date().toISOString()
  RULE_VERSIONS.get(r.id)?.push({ version: r.current_version, envelope: clean, change_note: change_note ?? null, created_by: currentUser(ctx).id, created_at: r.updated_at })
  return { ...r, versions: RULE_VERSIONS.get(r.id) ?? [] }
})
on('GET', 'projects/:pid/rules/:id/versions/:v', (ctx) => {
  const r = rule(ctx.params[0]!, ctx.params[1]!)
  const v = RULE_VERSIONS.get(r.id)?.find(x => x.version === Number(ctx.params[2]))
  if (!v) throw new MockHttpError(404, 'Not found')
  return v
})
on('POST', 'projects/:pid/rules/:id/backtest', ctx => backtest(ctx.params[1]!.length * 37))
on('POST', 'projects/:pid/rules/:id/submit', ctx => transition(ctx.params[0]!, ctx.params[1]!, 'pending_approval', ctx))
on('POST', 'projects/:pid/rules/:id/approve', ctx => transition(ctx.params[0]!, ctx.params[1]!, 'active', ctx))
on('POST', 'projects/:pid/rules/:id/reject', ctx => transition(ctx.params[0]!, ctx.params[1]!, 'draft', ctx))
on('POST', 'projects/:pid/rules/:id/retire', ctx => transition(ctx.params[0]!, ctx.params[1]!, 'retired', ctx))

// rulesets
function rulesets(pid: string): Ruleset[] { project(pid); return RULESETS.get(pid) ?? [] }
function ruleset(pid: string, id: string): Ruleset {
  const r = rulesets(pid).find(x => x.id === id)
  if (!r) throw new MockHttpError(404, 'Not found')
  return r
}
on('GET', 'projects/:pid/rulesets', ctx => paginate(rulesets(ctx.params[0]!), ctx.query))
on('POST', 'projects/:pid/rulesets', (ctx) => {
  const b = obj(ctx.body) as Partial<Ruleset>
  const rs: Ruleset = { id: mockId('rst'), code: b.code ?? 'RS-NEW', name: b.name ?? 'New ruleset', description: b.description ?? null, event_types: b.event_types ?? [], typologies: b.typologies ?? [], aggregation: b.aggregation ?? 'probabilistic_or', max_score: b.max_score ?? 100, version: 1, status: 'draft', rules: [], created_at: new Date().toISOString(), updated_at: new Date().toISOString() }
  rulesets(ctx.params[0]!).push(rs)
  return rs
})
on('GET', 'projects/:pid/rulesets/:id', ctx => ruleset(ctx.params[0]!, ctx.params[1]!))
on('PUT', 'projects/:pid/rulesets/:id', (ctx) => {
  const rs = ruleset(ctx.params[0]!, ctx.params[1]!)
  Object.assign(rs, obj(ctx.body), { status: 'draft', version: rs.version + 1, updated_at: new Date().toISOString() })
  return rs
})
on('PUT', 'projects/:pid/rulesets/:id/rules', (ctx) => {
  const rs = ruleset(ctx.params[0]!, ctx.params[1]!)
  const all = rules(ctx.params[0]!)
  rs.rules = (ctx.body as { rule_id: string, weight: number, pinned_version: number | null }[]).map((m) => {
    const r = all.find(x => x.id === m.rule_id)
    return { ...m, rule_code: r?.code, rule_name: r?.name }
  })
  rs.status = 'draft'
  return rs
})
for (const [action, status] of [['submit', 'pending_approval'], ['approve', 'active'], ['reject', 'draft'], ['retire', 'retired']] as const) {
  on('POST', `projects/:pid/rulesets/:id/${action}`, (ctx) => {
    const rs = ruleset(ctx.params[0]!, ctx.params[1]!)
    rs.status = action === 'approve' ? ((obj(ctx.body).target_status as RuleStatus) ?? status) : status
    return rs
  })
}
on('POST', 'projects/:pid/rulesets/:id/backtest', () => backtest(777))

// reference lists
function listsFor(pid: string) { project(pid); return REF_LISTS.get(pid) ?? [] }
function findList(id: string) {
  for (const lists of REF_LISTS.values()) {
    const l = lists.find(x => x.id === id)
    if (l) return l
  }
  throw new MockHttpError(404, 'Not found', 'reference list not found')
}
on('GET', 'projects/:pid/reference-lists', ctx => [...listsFor(ctx.params[0]!), ...(REF_LISTS.get('tenant') ?? [])])
on('POST', 'projects/:pid/reference-lists', (ctx) => {
  const b = obj(ctx.body) as { name?: string }
  if (!b.name || !/^[a-z0-9][a-z0-9_]{1,62}$/.test(b.name)) throw new MockHttpError(422, 'Validation failed', undefined, [{ field: 'name', message: 'must match ^[a-z0-9][a-z0-9_]{1,62}$' }])
  const l = { id: mockId('lst'), description: null, list_type: 'blacklist' as const, key_kind: 'generic', columns: [], ...obj(ctx.body), scope: 'project' as const, entry_count: 0, created_at: new Date().toISOString() } as never
  listsFor(ctx.params[0]!).push(l)
  REF_ENTRIES.set((l as { id: string }).id, [])
  return l
})
on('GET', 'projects/:pid/reference-lists/:id', ctx => findList(ctx.params[1]!))
on('PATCH', 'projects/:pid/reference-lists/:id', ctx => Object.assign(findList(ctx.params[1]!), obj(ctx.body)))
on('DELETE', 'projects/:pid/reference-lists/:id', (ctx) => {
  const l = findList(ctx.params[1]!)
  if (rules(ctx.params[0]!).some(r => r.status === 'active' && r.envelope.definition.kind === 'reference' && r.envelope.definition.list === l.name))
    throw new MockHttpError(409, 'Conflict', `list '${l.name}' is referenced by an active rule`)
  REF_LISTS.set(ctx.params[0]!, listsFor(ctx.params[0]!).filter(x => x.id !== l.id))
  return null
})
on('GET', 'projects/:pid/reference-lists/:id/entries', (ctx) => {
  const needle = (ctx.query.get('q') ?? '').toLowerCase()
  return paginate((REF_ENTRIES.get(ctx.params[1]!) ?? []).filter(e => !needle || e.key.toLowerCase().includes(needle)), ctx.query)
})
on('POST', 'projects/:pid/reference-lists/:id/entries', (ctx) => {
  const list = findList(ctx.params[1]!)
  const entries = REF_ENTRIES.get(list.id) ?? []
  for (const e of (obj(ctx.body).entries as { key: string, attributes?: Record<string, unknown>, reason?: string, valid_until?: string }[] ?? [])) {
    const existing = entries.find(x => x.key === e.key)
    if (existing) Object.assign(existing, e)
    else entries.unshift({ id: Date.now() + entries.length, key: e.key, attributes: e.attributes ?? {}, valid_from: new Date().toISOString(), valid_until: e.valid_until ?? null, reason: e.reason ?? null, created_at: new Date().toISOString() })
  }
  REF_ENTRIES.set(list.id, entries)
  list.entry_count = entries.length
  return { upserted: entries.length }
})
on('POST', 'projects/:pid/reference-lists/:id/import', (ctx) => {
  const list = findList(ctx.params[1]!)
  const text = ctx.rawBody ? Buffer.from(ctx.rawBody).toString('utf8') : ''
  const csv = text.split(/\r?\n/).filter(l => l && !l.startsWith('--') && !/^content-/i.test(l) && l.includes(','))
  const entries = REF_ENTRIES.get(list.id) ?? []
  let n = 0
  for (const line of csv.slice(1)) {
    const [key, ...rest] = line.split(',')
    if (!key) continue
    entries.unshift({ id: Date.now() + n, key: key.trim(), attributes: { value: rest.join(',') }, valid_from: new Date().toISOString(), valid_until: null, reason: 'csv import', created_at: new Date().toISOString() })
    n += 1
  }
  REF_ENTRIES.set(list.id, entries)
  list.entry_count = entries.length
  return { imported: n }
})
on('DELETE', 'projects/:pid/reference-lists/:id/entries/:eid', (ctx) => {
  REF_ENTRIES.set(ctx.params[1]!, (REF_ENTRIES.get(ctx.params[1]!) ?? []).filter(e => String(e.id) !== ctx.params[2]))
  return null
})
on('POST', 'projects/:pid/formulas/evaluate', (ctx) => {
  const b = obj(ctx.body) as { expr?: string, variables?: Record<string, unknown> }
  try {
    return { value: evaluateFormula(String(b.expr ?? ''), b.variables ?? {}), trapped: false, params: Object.keys(b.variables ?? {}) }
  }
  catch (err) {
    if (err instanceof FormulaError) {
      if (/null|division|non-finite|not numeric/.test(err.message)) return { trapped: true, reason: err.message.replace(/ /g, '_'), params: [] }
      throw new MockHttpError(422, 'Invalid formula', err.message, [], { message: err.message, position: err.position })
    }
    throw err
  }
})

// proposals
on('GET', 'projects/:pid/proposals', (ctx) => {
  const q = ctx.query
  return paginate((PROPOSALS.get(ctx.params[0]!) ?? []).filter(p => (!q.get('status') || p.status === q.get('status')) && (!q.get('source') || p.source === q.get('source'))), q)
})
on('GET', 'projects/:pid/proposals/:id', (ctx) => {
  const p = (PROPOSALS.get(ctx.params[0]!) ?? []).find(x => x.id === ctx.params[1])
  if (!p) throw new MockHttpError(404, 'Not found')
  return p
})
on('POST', 'projects/:pid/proposals/:id/approve', (ctx) => {
  const p = (PROPOSALS.get(ctx.params[0]!) ?? []).find(x => x.id === ctx.params[1])
  if (!p) throw new MockHttpError(404, 'Not found')
  if (p.status !== 'pending') throw new MockHttpError(409, 'Conflict', 'proposal is not pending')
  if (p.definition && p.proposal_type === 'new_rule') {
    const r = ruleFromEnvelope(p.definition, 'shadow')
    rules(ctx.params[0]!).unshift(r)
    p.applied_rule_id = r.id
  }
  p.status = 'applied'
  p.reviewed_by = currentUser(ctx).id
  p.reviewed_at = new Date().toISOString()
  return p
})
on('POST', 'projects/:pid/proposals/:id/reject', (ctx) => {
  const p = (PROPOSALS.get(ctx.params[0]!) ?? []).find(x => x.id === ctx.params[1])
  if (!p) throw new MockHttpError(404, 'Not found')
  p.status = 'rejected'
  p.review_comment = String(obj(ctx.body).comment ?? '')
  p.reviewed_at = new Date().toISOString()
  return p
})

// ---------------------------------------------------------------- graph
function graphNeighborhood(pid: string, cid: string, depth: number, kinds: string[]) {
  const d = dataFor(pid)
  const center = d.customers.find(c => c.id === cid) ?? d.customers[0]!
  const nodes = [{ id: center.id, type: 'customer' as const, label: center.external_id, risk_label: center.risk_label, is_center: true }]
  const edges: { id: string, source: string, target: string, kind: string, similarity?: number }[] = []
  const entityKinds = (kinds.length ? kinds : ['device', 'phone', 'card', 'address', 'email']) as ('device' | 'phone' | 'card' | 'address' | 'email')[]
  let idx = d.customers.indexOf(center)
  entityKinds.slice(0, 4).forEach((kind, k) => {
    const eid = `ent-${kind}-${center.id.slice(0, 6)}-${k}`
    const label = kind === 'card' ? '411111******1111' : kind === 'phone' ? '+62812****8890' : kind === 'device' ? `dev-${k * 7}` : kind === 'email' ? 'b***@mail.test' : 'Jl. Mawar 12, Jakarta'
    nodes.push({ id: eid, type: 'entity', label, kind } as never)
    edges.push({ id: `e-${center.id}-${eid}`, source: center.id, target: eid, kind })
    const sharing = 1 + ((k + idx) % 3)
    for (let s = 0; s < sharing; s++) {
      idx = (idx + 7) % d.customers.length
      const other = d.customers[idx]!
      if (other.id === center.id || nodes.some(n => n.id === other.id)) continue
      nodes.push({ id: other.id, type: 'customer', label: other.external_id, risk_label: other.risk_label } as never)
      edges.push({ id: `e-${other.id}-${eid}`, source: other.id, target: eid, kind })
      if (depth >= 2 && s === 0) {
        const eid2 = `ent-device-${other.id.slice(0, 6)}-2`
        nodes.push({ id: eid2, type: 'entity', label: `dev-${idx}`, kind: 'device' } as never)
        edges.push({ id: `e-${other.id}-${eid2}`, source: other.id, target: eid2, kind: 'device' })
        const third = d.customers[(idx + 11) % d.customers.length]!
        if (!nodes.some(n => n.id === third.id)) {
          nodes.push({ id: third.id, type: 'customer', label: third.external_id, risk_label: k === 1 ? 'fraud' : third.risk_label } as never)
          edges.push({ id: `e-${third.id}-${eid2}`, source: third.id, target: eid2, kind: 'device' })
        }
      }
    }
  })
  if (kinds.includes('address') || !kinds.length) {
    const sim = `ent-address-sim-${center.id.slice(0, 6)}`
    nodes.push({ id: sim, type: 'entity', label: 'Jl Mawar No.12 Jkt', kind: 'address' } as never)
    const addr = nodes.find(n => n.id.startsWith('ent-address-'))
    if (addr && addr.id !== sim) edges.push({ id: `sim-${addr.id}`, source: addr.id, target: sim, kind: 'similar', similarity: 0.91 })
  }
  return { nodes, edges }
}
on('GET', 'projects/:pid/graph/customers/:cid/neighborhood', ctx => graphNeighborhood(ctx.params[0]!, ctx.params[1]!, Number(ctx.query.get('depth') ?? 2), (ctx.query.get('link_kinds') ?? '').split(',').filter(Boolean)))
on('GET', 'projects/:pid/graph/customers/:cid/fraud-proximity', (ctx) => {
  const g = graphNeighborhood(ctx.params[0]!, ctx.params[1]!, 2, [])
  const fraud = g.nodes.find(n => n.type === 'customer' && !n.is_center && n.risk_label === 'fraud')
  if (!fraud) return { distance: null, path: [], nearest_fraud_customer_id: null, fraud_within: { 1: 0, 2: 0, 3: 0 }, truncated: false }
  const viaEdge = g.edges.find(e => e.source === fraud.id)
  const mid = g.edges.find(e => e.target === viaEdge?.target && e.source !== fraud.id)
  const firstHop = g.edges.find(e => e.source === ctx.params[1] && g.edges.some(x => x.source === mid?.source && x.target === e.target))
  const ids = [ctx.params[1]!, firstHop?.target, mid?.source, viaEdge?.target, fraud.id].filter(Boolean) as string[]
  const path = ids.map(id => g.nodes.find(n => n.id === id)).filter(Boolean).map(n => ({ id: n!.id, type: n!.type, label: n!.label, ...('kind' in n! ? { kind: (n as { kind?: string }).kind } : {}), ...(n!.risk_label ? { risk_label: n!.risk_label } : {}) }))
  return { distance: 2, path, nearest_fraud_customer_id: fraud.id, fraud_within: { 1: 0, 2: 1, 3: 2 }, truncated: false }
})
on('GET', 'projects/:pid/graph/components', (ctx) => {
  const only = ctx.query.get('only_with_fraud') === 'true'
  const customers = dataFor(ctx.params[0]!).customers
  return Array.from({ length: 15 }, (_, i) => ({ component_id: `cmp-${i}`, size: 3 + ((i * 7) % 40), fraud_count: i % 3 === 0 ? 1 + (i % 4) : 0, fraud_rate: i % 3 === 0 ? (1 + (i % 4)) / (3 + ((i * 7) % 40)) : 0, sample_customer_ids: customers.slice(i * 2, i * 2 + 4).map(c => c.id) }))
    .filter(c => !only || c.fraud_count > 0).sort((a, b) => b.size - a.size)
})
on('GET', 'projects/:pid/graph/stats', () => ({
  customers: 48_211, fraud_customers: 212, supernode_degree_cap: 50, entities: 131_902, links: 402_117, similarity_links: 8_342,
  supernodes: [{ kind: 'ip', display_value: '36.72.0.***', degree: 1_830 }, { kind: 'device', display_value: 'dev-emulator-*', degree: 212 }, { kind: 'address', display_value: 'Jl. Gudang Ekspedisi ***', degree: 97 }],
}))
on('GET', 'projects/:pid/graph/search', (ctx) => {
  const needle = (ctx.query.get('q') ?? '').trim().toLowerCase()
  if (needle.length < 2) throw new MockHttpError(422, 'Unprocessable Entity', undefined, [{ field: 'q', message: 'must contain at least 2 characters' }])
  const customers = dataFor(ctx.params[0]!).customers.filter(c => c.external_id.toLowerCase().includes(needle) || (c.full_name ?? '').toLowerCase().includes(needle)).slice(0, 20)
  return { customers: customers.map(c => ({ id: c.id, external_id: c.external_id, risk_label: c.risk_label })), entities: [] }
})

// ---------------------------------------------------------------- ML
on('GET', 'ml/algorithms', () => ALGORITHMS)
on('POST', 'ml/algorithms/reload', () => ({ loaded: ALGORITHMS.filter(a => a.status === 'available').length, invalid: ALGORITHMS.filter(a => a.status === 'invalid').map(a => ({ module: a.name, error: a.error })) }))
on('GET', 'projects/:pid/ml/models', (ctx) => {
  const kind = ctx.query.get('kind')
  return paginate((MODELS.get(ctx.params[0]!) ?? []).filter(m => !kind || m.kind === kind).sort((a, b) => b.version - a.version), ctx.query)
})
on('GET', 'projects/:pid/ml/models/:id', (ctx) => {
  const m = (MODELS.get(ctx.params[0]!) ?? []).find(x => x.id === ctx.params[1])
  if (!m) throw new MockHttpError(404, 'Not found')
  if (m.status === 'training') m.progress = Math.min(1, m.progress + 0.2)
  if (m.progress >= 1 && m.status === 'training') { m.status = 'ready'; m.training_finished_at = new Date().toISOString() }
  return m
})
for (const kind of ['supervised', 'unsupervised'] as const) {
  on('POST', `projects/:pid/ml/${kind}/train`, (ctx) => {
    const list = MODELS.get(ctx.params[0]!) ?? []
    const template = list.find(m => m.kind === kind)
    const b = obj(ctx.body)
    const version = Math.max(0, ...list.filter(m => m.kind === kind).map(m => m.version)) + 1
    const m = {
      ...(template ? structuredClone(template) : { feature_set_version: 1, feature_names: [], metrics: {}, training_history: {}, trained_rows: null, error: null, activated_at: null, training_finished_at: null }),
      id: mockId('mdl'), kind, version, status: 'training' as const, progress: 0, activated_at: null, training_started_at: new Date().toISOString(), training_finished_at: null,
      algorithms: kind === 'supervised' ? { supervised: { name: String(b.algorithm ?? 'mlp_backprop'), version: '1.0.0' } } : { anomaly: { name: String(b.anomaly_algorithm ?? 'isolation_forest'), version: '1.0.0' }, clustering: { name: String(b.clustering_algorithm ?? 'hdbscan'), version: '1.0.0' } },
      params: (b.params ?? { ...(b.anomaly_params as object ?? {}), ...(b.clustering_params as object ?? {}) }) as Record<string, unknown>,
    }
    list.unshift(m as never)
    MODELS.set(ctx.params[0]!, list)
    return { model_id: m.id, status: 'training' }
  })
}
for (const [action, status] of [['submit', 'pending_approval'], ['approve', 'active'], ['reject', 'ready']] as const) {
  on('POST', `projects/:pid/ml/models/:id/${action}`, (ctx) => {
    const list = MODELS.get(ctx.params[0]!) ?? []
    const m = list.find(x => x.id === ctx.params[1])
    if (!m) throw new MockHttpError(404, 'Not found')
    if (action === 'approve') {
      if (m.status !== 'pending_approval') throw new MockHttpError(409, 'Conflict', 'model must be pending_approval')
      list.filter(x => x.kind === m.kind && x.status === 'active').forEach((x) => { x.status = 'archived' })
      m.activated_at = new Date().toISOString()
    }
    m.status = status
    return m
  })
}
function activeUnsupervised(pid: string, modelId: string | null) {
  const m = (MODELS.get(pid) ?? []).find(x => x.kind === 'unsupervised' && (modelId ? x.id === modelId : x.status === 'active'))
  if (!m) throw new MockHttpError(404, 'Not found', 'no active unsupervised model')
  return m
}
on('GET', 'projects/:pid/ml/unsupervised/clusters', (ctx) => {
  const m = activeUnsupervised(ctx.params[0]!, ctx.query.get('model_id'))
  return { model_id: m.id, items: clustersFor(m.id).map(c => ({ ...c, ...(CLUSTER_LABELS.get(`${m.id}:${c.cluster_id}`) ?? {}) })) }
})
on('PATCH', 'projects/:pid/ml/unsupervised/clusters/:mid/:cid', (ctx) => {
  const b = obj(ctx.body) as { label?: string, notes?: string }
  CLUSTER_LABELS.set(`${ctx.params[1]}:${ctx.params[2]}`, { label: b.label ?? null, notes: b.notes ?? null })
  return { ok: true }
})
on('GET', 'projects/:pid/ml/unsupervised/projection', ctx => ({ model_id: activeUnsupervised(ctx.params[0]!, ctx.query.get('model_id')).id, items: projectionFor(ctx.params[0]!, Number(ctx.query.get('limit') ?? 2000)) }))
on('GET', 'projects/:pid/ml/unsupervised/anomalies', (ctx) => {
  const events = dataFor(ctx.params[0]!).events
  const model = activeUnsupervised(ctx.params[0]!, ctx.query.get('model_id'))
  const min = Number(ctx.query.get('min_score') ?? 0)
  return { model_id: model.id, items: projectionFor(ctx.params[0]!, 900).filter(p => p.anomaly_score >= min).sort((a, b) => b.anomaly_score - a.anomaly_score)
    .slice(0, Number(ctx.query.get('limit') ?? 50)).map((p) => {
      const e = events.find(x => x.id === p.event_id)
      return { event_id: p.event_id, external_id: e?.external_id, event_type: e?.event_type, customer_id: e?.customer_id, anomaly_score: p.anomaly_score, cluster_id: p.cluster_id, occurred_at: e?.occurred_at, amount: e?.amount, label: p.label ?? null }
    }) }
})
on('GET', 'projects/:pid/ml/graph-communities', ctx => ({ items: communitiesFor(ctx.params[0]!).filter(c => c.size >= Number(ctx.query.get('min_size') ?? 0)) }))
on('POST', 'projects/:pid/ml/graph-communities/recompute', () => ({ status: 'queued' }))

// ---------------------------------------------------------------- LLM
function attached(pid: string) {
  const ids = PROJECT_REGULATIONS.get(pid) ?? []
  return { items: REGULATIONS.filter(r => ids.includes(r.id)), regulation_ids: ids }
}
on('GET', 'projects/:pid/llm/regulations', ctx => attached(ctx.params[0]!))
on('PUT', 'projects/:pid/llm/regulations', (ctx) => {
  const ids = (obj(ctx.body).regulation_ids ?? ctx.body) as string[]
  PROJECT_REGULATIONS.set(ctx.params[0]!, Array.isArray(ids) ? ids : [])
  return attached(ctx.params[0]!)
})
on('POST', 'projects/:pid/llm/regulations/search', (ctx) => {
  const query = String(obj(ctx.body).query ?? '')
  const reg = REGULATIONS[0]!
  return { items: [
    { regulation_id: reg.id, chunk_id: `${reg.id}:12`, code: reg.code, version: reg.version, section: 'Pasal 15A', excerpt: 'LJK wajib menerapkan verifikasi tambahan untuk perubahan data kredensial yang diikuti transaksi dalam 24 jam.', score: 0.83 },
    { regulation_id: reg.id, chunk_id: `${reg.id}:4`, code: reg.code, version: reg.version, section: 'Pasal 8 ayat (2)', excerpt: `…pemantauan transaksi secara real-time… (query: ${query})`, score: 0.71 },
  ] }
})
const CONVERSATIONS = new Map<string, { id: string, title: string, created_at: string, messages: { role: string, content: string, tool_calls?: unknown[], citations?: unknown[] }[] }>()
function answerFor(message: string, pid: string) {
  const reg = REGULATIONS[0]!
  const rulesCount = rules(pid).filter(r => r.status === 'active').length
  return {
    answer: `Berdasarkan data 7 hari terakhir, terdapat ${rulesCount} rule aktif. Pola paling menonjol adalah **promo abuse** (cluster "Promo farm", fraud rate 41%). `
      + `Terkait pertanyaan "${message.slice(0, 80)}", POJK-12-2024 Pasal 15A mewajibkan verifikasi tambahan untuk perubahan kredensial yang diikuti transaksi dalam 24 jam — saat ini belum ada rule aktif yang menutup kondisi tersebut, sehingga saya membuat **proposal** untuk ditinjau approver.`,
    tool_calls: [
      { name: 'get_analytics_overview', args: { since_days: 7 }, result_summary: '22.4k events, 1.3k review, 310 decline' },
      { name: 'search_regulations', args: { query: 'perubahan kredensial' }, result_summary: '2 chunks from POJK-12-2024' },
      { name: 'get_rules_performance', args: { since_days: 30 }, result_summary: `${rulesCount} active rules` },
    ],
    citations: [{ regulation_id: reg.id, code: reg.code, section: 'Pasal 15A', excerpt: 'verifikasi tambahan untuk perubahan data kredensial yang diikuti transaksi dalam 24 jam' }],
  }
}
on('POST', 'projects/:pid/llm/chat', (ctx) => {
  const b = obj(ctx.body) as { conversation_id?: string, message?: string }
  const conv = (b.conversation_id && CONVERSATIONS.get(b.conversation_id)) || { id: mockId('cnv'), title: String(b.message ?? '').slice(0, 60), created_at: new Date().toISOString(), messages: [] }
  CONVERSATIONS.set(conv.id, conv)
  const a = answerFor(String(b.message ?? ''), ctx.params[0]!)
  conv.messages.push({ role: 'user', content: String(b.message ?? '') }, { role: 'assistant', content: a.answer, tool_calls: a.tool_calls, citations: a.citations })
  return { conversation_id: conv.id, ...a }
})
on('GET', 'projects/:pid/llm/conversations', ctx => paginate([...CONVERSATIONS.values()].map(({ messages: _m, ...c }) => c).reverse(), ctx.query))
on('GET', 'projects/:pid/llm/conversations/:id', (ctx) => {
  const c = CONVERSATIONS.get(ctx.params[1]!)
  if (!c) throw new MockHttpError(404, 'Not found')
  return c
})
on('POST', 'projects/:pid/llm/analysis/:type', (ctx) => {
  const type = ctx.params[1]!.replace(/-/g, '_') as 'rule_relevance'
  const list = REPORTS.get(ctx.params[0]!) ?? []
  const r = { id: mockId('rpt'), report_type: type, title: `${type.replace(/_/g, ' ')} — ${new Date().toISOString().slice(0, 10)}`, status: 'running' as const, params: obj(ctx.body), content_md: null as string | null, structured: null, model: 'qwen2.5:7b-instruct', error: null, created_at: new Date().toISOString(), finished_at: null as string | null }
  list.unshift(r)
  REPORTS.set(ctx.params[0]!, list)
  setTimeout(() => {
    (r as { status: string }).status = 'done'
    r.content_md = `## Hasil analisis\n\nAnalisis **${type}** selesai (mock). Rule \`RL-CARD-003\` masih relevan (precision 0.68). Rule \`RL-MER-001\` jarang terpicu dalam 30 hari (3 hit) — pertimbangkan untuk ditinjau.`
    r.finished_at = new Date().toISOString()
  }, 3000)
  return { report_id: r.id }
})
on('GET', 'projects/:pid/llm/reports', ctx => paginate(REPORTS.get(ctx.params[0]!) ?? [], ctx.query))
on('GET', 'projects/:pid/llm/reports/:id', (ctx) => {
  const r = (REPORTS.get(ctx.params[0]!) ?? []).find(x => x.id === ctx.params[1])
  if (!r) throw new MockHttpError(404, 'Not found')
  return r
})

// ---------------------------------------------------------------- data sources
function sources(pid: string): DataSource[] { project(pid); return DATA_SOURCES.get(pid) ?? [] }
function source(pid: string, id: string): DataSource {
  const s = sources(pid).find(x => x.id === id)
  if (!s) throw new MockHttpError(404, 'Not found', 'data source not found')
  return s
}
on('GET', 'projects/:pid/data-sources', ctx => paginate(sources(ctx.params[0]!), ctx.query))
on('POST', 'projects/:pid/data-sources', (ctx) => {
  const b = obj(ctx.body) as Partial<DataSource>
  if (!b.slug || !/^[a-z0-9][a-z0-9_-]{1,62}$/.test(b.slug)) throw new MockHttpError(422, 'Validation failed', undefined, [{ field: 'slug', message: 'must match ^[a-z0-9][a-z0-9_-]{1,62}$' }])
  const s: DataSource = { id: mockId('dsr'), slug: b.slug, name: b.name ?? b.slug, description: b.description ?? null, kind: b.kind ?? 'webhook', default_event_type: b.default_event_type ?? null, mode: b.mode ?? 'score', connection: b.connection ?? {}, inferred_schema: null, api_key_prefix: b.kind === 'webhook' ? 'fpk_N3wK3y01' : null, is_active: true, active_mapping_version: null, created_at: new Date().toISOString() }
  sources(ctx.params[0]!).push(s)
  MAPPINGS.set(s.id, [])
  return { ...s, api_key: s.kind === 'webhook' ? 'fpk_N3wK3y01.s3cr3t-shown-only-once-7c1f0a9b' : undefined }
})
on('GET', 'projects/:pid/data-sources/:id', ctx => source(ctx.params[0]!, ctx.params[1]!))
on('PATCH', 'projects/:pid/data-sources/:id', ctx => Object.assign(source(ctx.params[0]!, ctx.params[1]!), obj(ctx.body)))
on('DELETE', 'projects/:pid/data-sources/:id', (ctx) => {
  DATA_SOURCES.set(ctx.params[0]!, sources(ctx.params[0]!).filter(s => s.id !== ctx.params[1]))
  return null
})
on('POST', 'projects/:pid/data-sources/:id/rotate-key', ctx => ({ ...source(ctx.params[0]!, ctx.params[1]!), api_key: `fpk_R0t4t3d9.new-secret-${Date.now().toString(36)}` }))
on('GET', 'projects/:pid/data-sources/:id/mappings', ctx => ({ items: MAPPINGS.get(source(ctx.params[0]!, ctx.params[1]!).id) ?? [] }))
on('POST', 'projects/:pid/data-sources/:id/mappings', (ctx) => {
  const s = source(ctx.params[0]!, ctx.params[1]!)
  const list = MAPPINGS.get(s.id) ?? []
  const mapping = (obj(ctx.body).mapping ?? ctx.body) as MappingSpec
  for (const req of ['external_id', 'occurred_at', 'customer_external_id']) {
    if (!mapping.event?.[req]) throw new MockHttpError(422, 'Validation failed', undefined, [{ field: `event.${req}`, message: 'required canonical field is not mapped' }])
  }
  const v = { version: Math.max(0, ...list.map(x => x.version)) + 1, mapping, status: 'draft' as const, created_at: new Date().toISOString(), activated_at: null }
  list.push(v)
  MAPPINGS.set(s.id, list)
  return v
})
on('POST', 'projects/:pid/data-sources/:id/mappings/preview', (ctx) => {
  const b = obj(ctx.body) as { mapping?: MappingSpec, records?: Record<string, unknown>[] }
  const get = (rec: Record<string, unknown>, path: string) => path.split('.').reduce<unknown>((o, k) => (o && typeof o === 'object' ? (o as Record<string, unknown>)[k] : undefined), rec)
  return { items: (b.records ?? []).slice(0, 50).map((rec): MappingPreviewRow => {
    const event: Record<string, unknown> = {}
    const errors: { field: string, message: string }[] = []
    for (const [target, fm] of Object.entries(b.mapping?.event ?? {})) {
      let v: unknown = fm.const !== undefined ? fm.const : typeof fm.from === 'string' ? get(rec, fm.from) : Array.isArray(fm.from) ? fm.from.map(f => get(rec, f)).filter(x => x != null).join(', ') : undefined
      if (v == null && fm.default !== undefined) v = fm.default
      for (const step of fm.transform ?? []) {
        if (step.fn === 'to_number') { const n = Number(v); if (v != null && Number.isNaN(n)) errors.push({ field: `event.${target}`, message: `cannot convert '${String(v)}' to number` }); else v = n }
        else if (step.fn === 'hash_pan' || step.fn === 'hash_account') v = v == null ? v : `hmac:${String(v).replace(/\D/g, '').slice(-4).padStart(12, '•')}`
        else if (step.fn === 'pan_bin') v = v == null ? v : String(v).replace(/\D/g, '').slice(0, Number(step.length ?? 6))
        else if (step.fn === 'pan_last4') v = v == null ? v : String(v).replace(/\D/g, '').slice(-4)
        else if (step.fn === 'scale') v = Number(v) * Number(step.factor ?? 1)
        else if (step.fn === 'lowercase') v = v == null ? v : String(v).toLowerCase()
        else if (step.fn === 'uppercase') v = v == null ? v : String(v).toUpperCase()
        else if (step.fn === 'to_string') v = v == null ? v : String(v)
        else if (step.fn === 'parse_datetime') { if (v != null) v = `${String(v)} → UTC` }
      }
      event[target] = v
    }
    for (const req of ['external_id', 'occurred_at', 'customer_external_id']) if (event[req] == null) errors.push({ field: `event.${req}`, message: 'required value is missing' })
    return errors.length ? { ok: false, errors } : { ok: true, event, customer: { external_id: event.customer_external_id }, label: null }
  }) }
})
on('POST', 'projects/:pid/data-sources/:id/mappings/:v/activate', (ctx) => {
  const s = source(ctx.params[0]!, ctx.params[1]!)
  const list = MAPPINGS.get(s.id) ?? []
  const v = list.find(x => x.version === Number(ctx.params[2]))
  if (!v) throw new MockHttpError(404, 'Not found')
  list.forEach((x) => { if (x.status === 'active') x.status = 'archived' })
  v.status = 'active'
  v.activated_at = new Date().toISOString()
  s.active_mapping_version = v.version
  return v
})
on('GET', 'projects/:pid/data-sources/:id/errors', (ctx): Page<IngestError> => paginate([
  { id: 1, job_id: null, record: { trx_id: 'T-99', total: 'abc' }, reason: 'amount: cannot convert \'abc\' to number', created_at: iso(3_600_000) },
  { id: 2, job_id: null, record: { total: 1000 }, reason: 'external_id: required value missing', created_at: iso(7_200_000) },
], ctx.query))
on('POST', 'projects/:pid/data-sources/:id/inspect', (ctx): InspectResult => {
  source(ctx.params[0]!, ctx.params[1]!)
  const preview = Array.from({ length: 8 }, (_, i) => ({
    trx_id: `T-${2000 + i}`, created: `2${i}/09/2026 1${i}:0${i}:00`, total: 150_000 + i * 99_000, jenis: i % 3 ? 'PURCHASE' : 'VOUCHER',
    user: { id: `U-${10 + (i % 4)}`, nama: `Pelanggan ${i}`, no_hp: `0812345678${i}`, email: `user${i}@mail.test` },
    no_kartu: '4111111111111111', kode_voucher: i % 3 ? null : 'HEMAT50', is_fraud: i === 5 ? 1 : 0,
  }))
  return {
    upload_id: `upl_${Date.now().toString(36)}`,
    unmapped_fields: [], notes: ['label column detected: is_fraud'], llm_used: false,
    schema: { fields: [
      { path: 'trx_id', inferred_type: 'string', null_ratio: 0, distinct_ratio: 1, sample_values: ['T-2000', 'T-2001'] },
      { path: 'created', inferred_type: 'datetime', datetime_format: '%d/%m/%Y %H:%M:%S', null_ratio: 0, distinct_ratio: 1, sample_values: ['20/09/2026 10:00:00'] },
      { path: 'total', inferred_type: 'integer', null_ratio: 0, distinct_ratio: 0.95, sample_values: [150000, 249000] },
      { path: 'jenis', inferred_type: 'string', null_ratio: 0, distinct_ratio: 0.02, sample_values: ['PURCHASE', 'VOUCHER'] },
      { path: 'user.id', inferred_type: 'string', null_ratio: 0, distinct_ratio: 0.3, sample_values: ['U-10'] },
      { path: 'user.nama', inferred_type: 'string', null_ratio: 0, distinct_ratio: 0.3, sample_values: ['Pel***'], pii: 'name' },
      { path: 'user.no_hp', inferred_type: 'string', null_ratio: 0.01, distinct_ratio: 0.3, sample_values: ['0812****780'], pii: 'phone' },
      { path: 'user.email', inferred_type: 'string', null_ratio: 0, distinct_ratio: 0.3, sample_values: ['u***@mail.test'], pii: 'email' },
      { path: 'no_kartu', inferred_type: 'string', null_ratio: 0.2, distinct_ratio: 0.6, sample_values: ['4111********1111'], pii: 'pan' },
      { path: 'kode_voucher', inferred_type: 'string', null_ratio: 0.7, distinct_ratio: 0.05, sample_values: ['HEMAT50'] },
      { path: 'is_fraud', inferred_type: 'integer', null_ratio: 0, distinct_ratio: 0.0001, sample_values: [0, 1] },
    ] },
    suggested_mapping: {
      event_type: { from: 'jenis', value_map: { PURCHASE: 'transaction', VOUCHER: 'promo_redemption' }, default: 'transaction' },
      event: {
        external_id: { from: 'trx_id' },
        occurred_at: { from: 'created', transform: [{ fn: 'parse_datetime', format: '%d/%m/%Y %H:%M:%S', timezone: 'Asia/Jakarta' }] },
        customer_external_id: { from: 'user.id', transform: [{ fn: 'to_string' }] },
        amount: { from: 'total', transform: [{ fn: 'to_number' }] },
        instrument_fingerprint: { from: 'no_kartu', transform: [{ fn: 'hash_pan' }] },
        card_bin: { from: 'no_kartu', transform: [{ fn: 'pan_bin' }] },
        card_last4: { from: 'no_kartu', transform: [{ fn: 'pan_last4' }] },
        promo_code: { from: 'kode_voucher' },
      },
      customer: {
        full_name: { from: 'user.nama' },
        email: { from: 'user.email', transform: [{ fn: 'normalize_email' }] },
        phone: { from: 'user.no_hp', transform: [{ fn: 'normalize_phone', default_country: 'ID' }] },
      },
      label: { from: 'is_fraud', fraud_values: [1, '1', 'true'] },
      drop_fields: ['no_kartu'],
    },
    confidence: { 'event.external_id': 0.97, 'event.occurred_at': 0.93, 'event.customer_external_id': 0.88, 'event.amount': 0.9, 'event.instrument_fingerprint': 0.99, 'event.promo_code': 0.64, 'customer.phone': 0.86, 'customer.full_name': 0.58, 'label': 0.82, 'event_type': 0.55 },
    preview,
  }
})
on('GET', 'projects/:pid/data-sources/:id/jobs', ctx => paginate(JOBS.get(ctx.params[1]!) ?? [], ctx.query))
on('POST', 'projects/:pid/data-sources/:id/jobs', (ctx) => {
  const s = source(ctx.params[0]!, ctx.params[1]!)
  const b = obj(ctx.body) as { mode?: 'score' | 'load_only' }
  const job = { id: mockId('job'), data_source_id: s.id, mode: b.mode ?? 'score', status: 'running' as const, total_rows: 8_000, processed_rows: 0, accepted_rows: 0, rejected_rows: 0, error: null, created_at: new Date().toISOString(), started_at: new Date().toISOString(), finished_at: null }
  JOBS.set(s.id, [job, ...(JOBS.get(s.id) ?? [])])
  return { job_id: job.id }
})
on('GET', 'projects/:pid/ingest-jobs/:jid', (ctx) => {
  for (const jobs of JOBS.values()) {
    const j = jobs.find(x => x.id === ctx.params[1])
    if (j) {
      if (j.status === 'running' && j.total_rows) {
        j.processed_rows = Math.min(j.total_rows, j.processed_rows + 1_600)
        j.accepted_rows = j.processed_rows - Math.floor(j.processed_rows / 400)
        j.rejected_rows = Math.floor(j.processed_rows / 400)
        if (j.processed_rows >= j.total_rows) { (j as { status: string }).status = 'done'; j.finished_at = new Date().toISOString() }
      }
      return j
    }
  }
  throw new MockHttpError(404, 'Not found')
})
on('POST', 'projects/:pid/ingest-jobs/:jid/cancel', (ctx) => {
  for (const jobs of JOBS.values()) {
    const j = jobs.find(x => x.id === ctx.params[1])
    if (j) { (j as { status: string }).status = 'cancelled'; return j }
  }
  throw new MockHttpError(404, 'Not found')
})
const FIELD_OVERRIDES = new Map<string, { velocity_enabled?: boolean }>()
on('GET', 'projects/:pid/field-catalog', (ctx) => {
  const ev = ['event_type', 'external_id', 'occurred_at', 'channel', 'status', 'amount', 'currency', 'merchant_id', 'merchant_category', 'payment_method', 'instrument_fingerprint', 'card_bin', 'card_last4', 'issuer_country', 'recipient_fingerprint', 'device_id', 'ip_address', 'user_agent', 'geo_country', 'geo_city', 'promo_code', 'discount_amount', 'cashback_amount', 'ref_transaction_id', 'shipping_address', 'billing_address', 'account_change_type', 'login_success', 'api_client_id', 'customer_id']
  const numeric = new Set(['amount', 'discount_amount', 'cashback_amount'])
  const feats = ['amount', 'log_amount', 'hour_of_day', 'day_of_week', 'is_night', 'account_age_days', 'cust_cnt_1h', 'cust_cnt_24h', 'cust_txn_sum_24h', 'cust_txn_avg_30d', 'amount_zscore_30d', 'cust_distinct_devices_24h', 'cust_distinct_ips_24h', 'cust_distinct_instruments_7d', 'instrument_distinct_customers_30d', 'device_distinct_customers_30d', 'ip_distinct_customers_24h', 'is_new_device', 'is_new_instrument', 'is_new_recipient', 'geo_country_mismatch', 'bin_country_mismatch', 'shipping_billing_mismatch', 'secs_since_last_login', 'secs_since_account_change', 'recent_credential_change_24h', 'failed_logins_24h', 'promo_cnt_cust_30d', 'promo_cnt_device_30d', 'promo_distinct_customers_same_code_device_7d', 'discount_ratio', 'api_client_cnt_5m', 'api_client_distinct_customers_5m', 'graph_distance_to_fraud', 'graph_fraud_neighbors_2', 'graph_component_size', 'graph_shared_entity_count']
  const rows = [
    ...ev.map(f => ({ path: `event.${f}`, entity: 'event', data_type: numeric.has(f) ? 'number' : f === 'occurred_at' ? 'datetime' : f === 'login_success' ? 'bool' : 'string', description: null, data_source_id: null, velocity_enabled: true, velocity_column: f, pii: false, builtin: true })),
    ...['external_id', 'kyc_level', 'segment', 'status', 'registered_at', 'risk_label', 'account_age_days', 'full_name', 'email', 'phone'].map(f => ({ path: `customer.${f}`, entity: 'customer', data_type: f === 'kyc_level' || f === 'account_age_days' ? 'number' : 'string', description: null, data_source_id: null, velocity_enabled: false, pii: ['full_name', 'email', 'phone'].includes(f), builtin: true })),
    ...feats.map(f => ({ path: `features.${f}`, entity: 'features', data_type: 'number', description: null, data_source_id: null, velocity_enabled: false, pii: false, builtin: true })),
    ...['fraud_probability', 'anomaly_score', 'cluster_id', 'cluster_fraud_rate'].map(f => ({ path: `ml.${f}`, entity: 'ml', data_type: 'number', description: null, data_source_id: null, velocity_enabled: false, pii: false, builtin: true })),
    ...['distance_to_fraud', 'fraud_neighbors_1', 'fraud_neighbors_2', 'component_size', 'shared_entity_count', 'degree', 'community_fraud_rate'].map(f => ({ path: `graph.${f}`, entity: 'graph', data_type: 'number', description: null, data_source_id: null, velocity_enabled: false, pii: false, builtin: true })),
    ...['order.id', 'order.items[0].sku', 'order.items[0].qty', 'order.voucher', 'user.tier'].map(f => ({ path: `source.${f}`, entity: 'source', data_type: f.endsWith('qty') ? 'integer' : 'string', description: null, data_source_id: 'shop-orders', velocity_enabled: f === 'order.voucher', pii: false, builtin: false })),
  ].map(r => ({ ...r, ...(FIELD_OVERRIDES.get(r.path) ?? {}) }))
  const q = ctx.query
  const needle = (q.get('q') ?? '').toLowerCase()
  const filtered = rows.filter(r => (!q.get('entity') || r.entity === q.get('entity')) && (!needle || r.path.toLowerCase().includes(needle)) && (!q.get('velocity_enabled') || String(r.velocity_enabled) === q.get('velocity_enabled')))
  return { items: filtered, total: filtered.length }
})
on('PATCH', 'projects/:pid/field-catalog/:path', (ctx) => {
  const path = decodeURIComponent(ctx.params[1]!)
  FIELD_OVERRIDES.set(path, { ...(FIELD_OVERRIDES.get(path) ?? {}), ...(obj(ctx.body) as { velocity_enabled?: boolean }) })
  return { path, ...FIELD_OVERRIDES.get(path) }
})

// ---------------------------------------------------------------- multipart helper (mock only; tiny & tolerant)
function parseMultipartFields(ctx: Ctx): Record<string, string> {
  const out: Record<string, string> = {}
  if (!ctx.rawBody) return out
  const text = Buffer.from(ctx.rawBody).toString('latin1')
  for (const part of text.split(/--[-\w]+/)) {
    const name = /name="([^"]+)"/.exec(part)?.[1]
    if (!name) continue
    const filename = /filename="([^"]*)"/.exec(part)?.[1]
    if (filename !== undefined) { out.__filename = filename; continue }
    out[name] = part.split(/\r?\n\r?\n/).slice(1).join('\n\n').replace(/\r?\n$/, '').trim()
  }
  return out
}

// ---------------------------------------------------------------- SSE chat stream
function sseChat(ctx: Ctx, pid: string): Response {
  const b = obj(ctx.body) as { message?: string, conversation_id?: string }
  const conv = (b.conversation_id && CONVERSATIONS.get(b.conversation_id)) || { id: mockId('cnv'), title: String(b.message ?? '').slice(0, 60), created_at: new Date().toISOString(), messages: [] as { role: string, content: string }[] }
  CONVERSATIONS.set(conv.id, conv as never)
  const a = answerFor(String(b.message ?? ''), pid)
  conv.messages.push({ role: 'user', content: String(b.message ?? '') }, { role: 'assistant', content: a.answer })
  const enc = new TextEncoder()
  const words = a.answer.split(/(\s+)/)
  const stream = new ReadableStream<Uint8Array>({
    async start(controller) {
      const send = (event: string, data: unknown) => controller.enqueue(enc.encode(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`))
      send('conversation', { conversation_id: conv.id })
      for (const tc of a.tool_calls) { send('tool', { ...tc, ok: true }); await new Promise(r => setTimeout(r, 120)) }
      for (const w of words) { send('token', { content: w }); await new Promise(r => setTimeout(r, 12)) }
      for (const c of a.citations) send('citation', c)
      send('done', { conversation_id: conv.id, answer: a.answer, tool_calls: a.tool_calls, citations: a.citations, proposals: [] })
      controller.close()
    },
  })
  return new Response(stream, { status: 200, headers: { 'content-type': 'text/event-stream; charset=utf-8', 'cache-control': 'no-cache' } })
}

// ---------------------------------------------------------------- entry point
export async function mockFetch(path: string, init: { method: string, headers: Record<string, string>, body?: BodyInit | null }): Promise<Response> {
  const url = new URL(path, 'http://mock.local')
  const rel = url.pathname.replace(/^\/api\/v1\//, '').replace(/\/+$/, '')
  const method = init.method.toUpperCase()
  let rawBody: Uint8Array | null = null
  if (init.body instanceof Uint8Array) rawBody = init.body
  else if (typeof init.body === 'string') rawBody = new TextEncoder().encode(init.body)
  else if (init.body instanceof ReadableStream) rawBody = new Uint8Array(await new Response(init.body).arrayBuffer())
  const contentType = init.headers['content-type'] ?? ''
  let body: unknown = null
  if (rawBody && contentType.includes('json')) {
    try { body = JSON.parse(new TextDecoder().decode(rawBody)) }
    catch { return json(400, { title: 'Bad request', status: 400, detail: 'invalid JSON body' }, true) }
  }
  const headers = Object.fromEntries(Object.entries(init.headers).map(([k, v]) => [k.toLowerCase(), v]))

  // tiny latency so loading states are visible in demos
  await new Promise(r => setTimeout(r, 60))

  const chatStream = /^projects\/([^/]+)\/llm\/chat\/stream$/.exec(rel)
  if (chatStream && method === 'POST') return sseChat({ method, path: rel, params: [chatStream[1]!], query: url.searchParams, body, headers, rawBody }, chatStream[1]!)

  for (const r of routes) {
    if (r.method !== method) continue
    const m = r.re.exec(rel)
    if (!m) continue
    try {
      const result = await r.handler({ method, path: rel, params: m.slice(1).map(decodeURIComponent), query: url.searchParams, body, headers, rawBody })
      if (result === null || result === undefined) return new Response(null, { status: 204 })
      return json(method === 'POST' && /(^|\/)(rules|rulesets|projects|reference-lists|data-sources|tenants|users|mappings)$/.test(rel) ? 201 : 200, result)
    }
    catch (err) {
      if (err instanceof MockHttpError) return json(err.status, { type: 'about:blank', title: err.title, status: err.status, detail: err.detail, ...(err.errors.length ? { errors: err.errors } : {}), ...err.extra }, true)
      console.error('[mock] handler error', rel, err)
      return json(500, { title: 'Mock error', status: 500, detail: String(err) }, true)
    }
  }
  return json(404, { type: 'about:blank', title: 'Not found', status: 404, detail: `mock has no route for ${method} /api/v1/${rel}` }, true)
}

function json(status: number, data: unknown, problem = false): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'content-type': problem ? 'application/problem+json' : 'application/json', 'x-request-id': `mock-${Math.random().toString(36).slice(2, 10)}` },
  })
}
