import { describe, expect, it } from 'vitest'
import { errorsByField, parseProblem, problemMessage } from '#shared/utils/problem'

describe('parseProblem', () => {
  it('reads RFC 7807 problem+json from the Rust services', () => {
    const p = parseProblem(422, { type: 'about:blank', title: 'Validation failed', status: 422, detail: 'invalid rule', errors: [{ path: 'definition.list', message: 'unknown list' }] }, 'req-1')
    expect(p).toEqual({ status: 422, title: 'Validation failed', detail: 'invalid rule', type: 'about:blank', errors: [{ field: 'definition.list', message: 'unknown list' }], requestId: 'req-1' })
    expect(errorsByField(p)).toEqual({ 'definition.list': 'unknown list' })
  })

  it('reads FastAPI validation errors (detail array with loc)', () => {
    const p = parseProblem(422, { detail: [{ loc: ['body', 'params', 'lr'], msg: 'Input should be greater than 0', type: 'greater_than' }] })
    expect(p.title).toBe('Validation failed')
    expect(p.errors).toEqual([{ field: 'params.lr', message: 'Input should be greater than 0' }])
  })

  it('reads FastAPI string detail', () => {
    expect(parseProblem(404, { detail: 'no_active_model' }).detail).toBe('no_active_model')
  })

  it('unwraps h3/Nuxt error envelopes', () => {
    const p = parseProblem(500, { statusCode: 403, statusMessage: 'Forbidden', data: { title: 'Forbidden', detail: 'maker–checker' } })
    expect(p.status).toBe(403)
    expect(p.detail).toBe('maker–checker')
  })

  it('parses JSON text bodies and ignores HTML error pages', () => {
    expect(parseProblem(409, '{"title":"Conflict","detail":"exists"}').detail).toBe('exists')
    const html = parseProblem(502, '<html><body>Bad Gateway</body></html>')
    expect(html.title).toBe('Service unavailable')
    expect(html.detail).toBeUndefined()
  })

  it('builds a readable one-line message', () => {
    const p = parseProblem(422, { title: 'Validation failed', detail: 'invalid', errors: [{ field: 'a', message: 'x' }, { field: 'b', message: 'y' }] })
    expect(problemMessage(p)).toBe('invalid — a: x; b: y')
    expect(problemMessage(parseProblem(0, null))).toBe('Request failed')
  })
})
