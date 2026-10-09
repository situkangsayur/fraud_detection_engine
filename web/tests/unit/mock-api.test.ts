// Contract smoke tests for the in-process mock gateway (keeps the demo mode honest to api-contract.md shapes).
import { describe, expect, it } from 'vitest'
import { mockFetch } from '../../server/mock'
import { EXAMPLE_SIMPLE } from '#shared/rules/examples'

async function call(method: string, path: string, body?: unknown, token?: string) {
  const res = await mockFetch(`/api/v1/${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...(token ? { authorization: `Bearer ${token}` } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const text = await res.text()
  return { status: res.status, type: res.headers.get('content-type'), json: text ? JSON.parse(text) : null }
}

describe('mock API', () => {
  it('logs in and returns memberships with roles', async () => {
    const login = await call('POST', 'auth/login', { email: 'analyst@demo.local', password: 'x' })
    expect(login.status).toBe(200)
    expect(login.json.refresh_token).toBeTruthy()
    const me = await call('GET', 'me', undefined, login.json.access_token)
    expect(me.json.projects.length).toBeGreaterThan(0)
    expect(me.json.projects[0].role).toBe('analyst')
    expect((await call('POST', 'auth/login', { email: 'analyst@demo.local', password: 'wrong' })).status).toBe(401)
  })

  it('returns memberships keyed by id (real /me shape)', async () => {
    const login = await call('POST', 'auth/login', { email: 'analyst@demo.local', password: 'x' })
    const me = await call('GET', 'me', undefined, login.json.access_token)
    expect(me.json.projects[0]).toMatchObject({ id: expect.any(String), slug: expect.any(String), status: 'active' })
  })

  it('returns event detail as {event, source, features, customer, decision, labels, case}', async () => {
    const pid = (await call('GET', 'projects')).json.items[0].id
    const ev = (await call('GET', `projects/${pid}/events?page_size=1`)).json.items[0]
    const d = (await call('GET', `projects/${pid}/events/${ev.id}`)).json
    expect(Object.keys(d).sort()).toEqual(['case', 'customer', 'decision', 'event', 'features', 'labels', 'source'])
    expect(d.event.id).toBe(ev.id)
  })

  it('returns plain arrays / wrappers like the real services', async () => {
    const pid = (await call('GET', 'projects')).json.items[0].id
    expect(Array.isArray((await call('GET', `projects/${pid}/reference-lists`)).json)).toBe(true)
    expect((await call('GET', `projects/${pid}/members`)).json.items.length).toBeGreaterThan(0)
    expect((await call('GET', `projects/${pid}/ml/unsupervised/clusters`)).json).toMatchObject({ model_id: expect.any(String), items: expect.any(Array) })
    expect((await call('GET', `projects/${pid}/analytics/drift`)).json.items.length).toBeGreaterThan(0)
    expect((await call('GET', `projects/${pid}/graph/search?q=C`)).status).toBe(422)
    expect((await call('GET', `projects/${pid}/graph/search?q=CUST`)).json.customers.length).toBeGreaterThan(0)
  })

  it('gives platform admins no project access (privacy by default)', async () => {
    const login = await call('POST', 'auth/login', { email: 'admin@fraud.local', password: 'x' })
    const me = await call('GET', 'me', undefined, login.json.access_token)
    expect(me.json.projects).toEqual([])
  })

  it('returns paginated lists and problem+json errors', async () => {
    const analyst = (await call('POST', 'auth/login', { email: 'analyst@demo.local', password: 'x' })).json.access_token
    const projects = await call('GET', 'projects', undefined, analyst)
    expect(projects.json).toMatchObject({ page: 1, page_size: 50 })
    const pid = projects.json.items[0].id
    const rules = await call('GET', `projects/${pid}/rules?kind=velocity`, undefined, analyst)
    expect(rules.json.items.every((r: { kind: string }) => r.kind === 'velocity')).toBe(true)
    const bad = await call('POST', `projects/${pid}/rules`, { ...EXAMPLE_SIMPLE, code: 'bad code' }, analyst)
    expect(bad.status).toBe(422)
    expect(bad.type).toContain('application/problem+json')
    expect(bad.json.errors[0]).toMatchObject({ field: 'code' })
  })

  it('enforces maker–checker on rule approval', async () => {
    const analyst = (await call('POST', 'auth/login', { email: 'analyst@demo.local', password: 'x' })).json.access_token
    const approver = (await call('POST', 'auth/login', { email: 'approver@demo.local', password: 'x' })).json.access_token
    const pid = (await call('GET', 'projects', undefined, analyst)).json.items[0].id
    const created = await call('POST', `projects/${pid}/rules`, { ...EXAMPLE_SIMPLE, code: 'RL-MC-TEST' }, analyst)
    expect(created.status).toBe(201)
    expect(created.json.status).toBe('draft')
    await call('POST', `projects/${pid}/rules/${created.json.id}/submit`, {}, analyst)
    expect((await call('POST', `projects/${pid}/rules/${created.json.id}/approve`, { target_status: 'shadow' }, analyst)).status).toBe(403)
    const ok = await call('POST', `projects/${pid}/rules/${created.json.id}/approve`, { target_status: 'shadow' }, approver)
    expect(ok.json.status).toBe('shadow')
  })

  it('streams LLM chat as SSE frames', async () => {
    const pid = (await call('GET', 'projects')).json.items[0].id
    const res = await mockFetch(`/api/v1/projects/${pid}/llm/chat/stream`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ message: 'hai' }) })
    expect(res.headers.get('content-type')).toContain('text/event-stream')
    const text = await res.text()
    expect(text).toContain('event: conversation')
    expect(text).toContain('"content"')
    expect(text).toContain('event: tool')
    expect(text).toContain('event: token')
    expect(text.trim().endsWith('}')).toBe(true)
    expect(text).toContain('event: done')
  })
})

describe('mock settings & decisions contract', () => {
  it('exposes engine_combination (default noisy_or), validates it, and has no dedupe flag', async () => {
    const pid = (await call('GET', 'projects')).json.items[0].id
    const s = await call('GET', `projects/${pid}/settings`)
    expect(s.json.engine_combination).toBe('noisy_or')
    expect(s.json.cases).toEqual({ auto_create_on: ['review', 'decline'] })
    expect((await call('PUT', `projects/${pid}/settings/engine_combination`, 'weighted_average')).json.engine_combination).toBe('weighted_average')
    expect((await call('PUT', `projects/${pid}/settings/engine_combination`, 'max')).status).toBe(422)
    await call('PUT', `projects/${pid}/settings/engine_combination`, 'noisy_or')
  })

  it('reason contributions sum to final_score', async () => {
    const pid = (await call('GET', 'projects')).json.items[0].id
    const events = (await call('GET', `projects/${pid}/events?decision=decline`)).json.items
    const detail = (await call('GET', `projects/${pid}/events/${events[0].id}`)).json.decision
    const sum = detail.reasons.reduce((a: number, r: { contribution: number }) => a + r.contribution, 0)
    expect(Math.abs(sum - detail.final_score)).toBeLessThan(0.5)
  })
})
