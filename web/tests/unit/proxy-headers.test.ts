import { describe, expect, it } from 'vitest'
import { buildUpstreamHeaders, checkCsrf, filterResponseHeaders, jwtSecondsLeft, shouldRefresh, toUpstreamPath } from '../../server/utils/proxy-headers'

function jwt(payload: object) {
  const b64 = (o: object) => Buffer.from(JSON.stringify(o)).toString('base64url')
  return `${b64({ alg: 'HS256' })}.${b64(payload)}.sig`
}

describe('BFF proxy header logic', () => {
  it('forwards only allow-listed request headers and injects the bearer token', () => {
    const h = buildUpstreamHeaders({
      'Cookie': 'fraud_session=secret', 'Authorization': 'Bearer attacker', 'Host': 'evil', 'Content-Type': 'application/json',
      'Accept': 'text/event-stream', 'Last-Event-ID': '42', 'X-Forwarded-For': '1.2.3.4', 'X-Request-Id': 'abcdefgh-1234',
    }, { accessToken: 'tok', clientIp: '10.0.0.9' })
    expect(h).toEqual({
      'content-type': 'application/json', 'accept': 'text/event-stream', 'last-event-id': '42',
      'x-request-id': 'abcdefgh-1234', 'x-forwarded-for': '10.0.0.9', 'authorization': 'Bearer tok',
    })
  })

  it('replaces malformed request ids', () => {
    const h = buildUpstreamHeaders({ 'x-request-id': 'bad id with spaces\n' })
    expect(h['x-request-id']).toMatch(/^[0-9a-f-]{36}$/)
  })

  it('never forwards upstream Set-Cookie', () => {
    const out = filterResponseHeaders(new Headers({ 'content-type': 'text/event-stream', 'set-cookie': 'a=b', 'x-request-id': 'r1', 'server': 'traefik' }))
    expect(out).toEqual({ 'content-type': 'text/event-stream', 'x-request-id': 'r1' })
  })

  it('requires the anti-CSRF header and same origin on mutating requests', () => {
    expect(checkCsrf({ method: 'GET', headers: {} })).toEqual({ ok: true })
    expect(checkCsrf({ method: 'POST', headers: {} }).ok).toBe(false)
    expect(checkCsrf({ method: 'POST', headers: { 'x-requested-with': 'fraud-web' }, expectedOrigin: 'https://app' })).toEqual({ ok: true })
    expect(checkCsrf({ method: 'DELETE', headers: { 'x-requested-with': 'fraud-web', 'origin': 'https://evil' }, expectedOrigin: 'https://app' }).ok).toBe(false)
  })

  it('decodes JWT expiry for proactive refresh', () => {
    const now = Date.UTC(2026, 0, 1)
    const token = jwt({ exp: now / 1000 + 30 })
    expect(jwtSecondsLeft(token, now)).toBe(30)
    expect(shouldRefresh(token, now)).toBe(true)
    expect(shouldRefresh(jwt({ exp: now / 1000 + 3600 }), now)).toBe(false)
    expect(jwtSecondsLeft('opaque-token')).toBeNull()
    expect(shouldRefresh('opaque-token')).toBe(false)
  })

  it('maps BFF paths to gateway paths and rejects traversal / reserved prefixes', () => {
    expect(toUpstreamPath('/api/projects/p1/rules?kind=simple&q=a%20b')).toBe('/api/v1/projects/p1/rules?kind=simple&q=a%20b')
    expect(toUpstreamPath('/api/projects/p1/field-catalog/source.order%2Eid')).toBe('/api/v1/projects/p1/field-catalog/source.order%2Eid')
    expect(toUpstreamPath('/api/projects/../admin')).toBeNull()
    expect(toUpstreamPath('/api/projects/%2e%2e/admin')).toBeNull()
    expect(toUpstreamPath('/api/v1/projects')).toBeNull()
    expect(toUpstreamPath('/api/auth/login')).toBeNull()
    expect(toUpstreamPath('/api/_auth/session')).toBeNull()
    expect(toUpstreamPath('/other')).toBeNull()
    expect(toUpstreamPath('/api/')).toBeNull()
  })
})
