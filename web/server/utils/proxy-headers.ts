// Pure helpers for the BFF proxy (no Nuxt/h3 globals → unit-tested in tests/unit/proxy-headers.test.ts).

/** Request headers the browser may pass through to the gateway. Everything else (cookies, host, auth…) is dropped. */
export const FORWARDED_REQUEST_HEADERS = [
  'accept',
  'accept-language',
  'content-type',
  'last-event-id',
  'if-none-match',
  'if-modified-since',
  'user-agent',
] as const

/** Response headers passed back to the browser. `set-cookie` from upstream is never forwarded. */
export const FORWARDED_RESPONSE_HEADERS = [
  'content-type',
  'content-disposition',
  'cache-control',
  'etag',
  'last-modified',
  'location',
  'retry-after',
  'x-request-id',
  'x-total-count',
] as const

export const CSRF_HEADER = 'x-requested-with'
export const CSRF_VALUE = 'fraud-web'

type HeaderBag = Record<string, string | string[] | undefined>

function first(v: string | string[] | undefined): string | undefined {
  return Array.isArray(v) ? v[0] : v
}

function lowerKeys(h: HeaderBag): HeaderBag {
  const out: HeaderBag = {}
  for (const [k, v] of Object.entries(h)) out[k.toLowerCase()] = v
  return out
}

const REQUEST_ID_RE = /^[\w.:-]{8,128}$/

export function newRequestId(): string {
  return globalThis.crypto.randomUUID()
}

export interface UpstreamHeaderOptions {
  accessToken?: string | null
  requestId?: string
  clientIp?: string
}

export function buildUpstreamHeaders(incoming: HeaderBag, opts: UpstreamHeaderOptions = {}): Record<string, string> {
  const h = lowerKeys(incoming)
  const out: Record<string, string> = {}
  for (const name of FORWARDED_REQUEST_HEADERS) {
    const v = first(h[name])
    if (v) out[name] = v
  }
  const incomingId = first(h['x-request-id'])
  out['x-request-id'] = opts.requestId ?? (incomingId && REQUEST_ID_RE.test(incomingId) ? incomingId : newRequestId())
  if (opts.clientIp) out['x-forwarded-for'] = opts.clientIp
  if (opts.accessToken) out.authorization = `Bearer ${opts.accessToken}`
  return out
}

export function filterResponseHeaders(headers: Headers | Record<string, string>): Record<string, string> {
  const get = (name: string) => (headers instanceof Headers ? headers.get(name) : headers[name] ?? headers[name.toLowerCase()])
  const out: Record<string, string> = {}
  for (const name of FORWARDED_RESPONSE_HEADERS) {
    const v = get(name)
    if (v) out[name] = v
  }
  return out
}

export function isMutatingMethod(method: string): boolean {
  return !['GET', 'HEAD', 'OPTIONS'].includes(method.toUpperCase())
}

export interface CsrfInput {
  method: string
  headers: HeaderBag
  /** Expected origin of this web app, e.g. "https://fraud.example.com" (derived from the request host). */
  expectedOrigin?: string
}

/**
 * Defence in depth on top of the SameSite=strict session cookie:
 * mutating requests must carry the custom header (cannot be set cross-site without a CORS preflight, which we never
 * allow) and, when the browser sends Origin, it must match our own origin.
 */
export function checkCsrf({ method, headers, expectedOrigin }: CsrfInput): { ok: true } | { ok: false, reason: string } {
  if (!isMutatingMethod(method)) return { ok: true }
  const h = lowerKeys(headers)
  if (first(h[CSRF_HEADER]) !== CSRF_VALUE) return { ok: false, reason: 'missing anti-CSRF header' }
  const origin = first(h.origin)
  if (origin && expectedOrigin && originHost(origin) !== originHost(expectedOrigin)) return { ok: false, reason: 'cross-origin request' }
  return { ok: true }
}

/**
 * Host[:port] of an origin. The scheme is ignored on purpose: behind a TLS-terminating proxy the browser sends
 * `https://host` while the BFF itself is reached over plain http.
 */
function originHost(origin: string): string | null {
  try {
    return new URL(origin).host.toLowerCase()
  }
  catch {
    return null
  }
}

/** Seconds until the JWT expires (unverified decode — only used to refresh proactively), or null if not a JWT. */
export function jwtSecondsLeft(token: string, nowMs = Date.now()): number | null {
  const parts = token.split('.')
  if (parts.length !== 3 || !parts[1]) return null
  try {
    const json = JSON.parse(Buffer.from(parts[1].replace(/-/g, '+').replace(/_/g, '/'), 'base64').toString('utf8')) as { exp?: unknown }
    return typeof json.exp === 'number' ? Math.floor(json.exp - nowMs / 1000) : null
  }
  catch {
    return null
  }
}

export function shouldRefresh(token: string, nowMs = Date.now(), skewSeconds = 60): boolean {
  const left = jwtSecondsLeft(token, nowMs)
  return left !== null && left < skewSeconds
}

/**
 * Maps a BFF path (`/api/projects/1/rules?x=1`) to the gateway path (`/api/v1/projects/1/rules?x=1`).
 * Rejects traversal attempts and anything that is not under /api/.
 */
export function toUpstreamPath(bffPath: string): string | null {
  if (!bffPath.startsWith('/api/')) return null
  const [pathname = '', query] = bffPath.split(/\?(.*)/s, 2)
  let decoded: string
  try { decoded = decodeURIComponent(pathname) }
  catch { return null }
  if (decoded.split('/').some(seg => seg === '..' || seg === '.')) return null
  if (/[\\\0]/.test(decoded)) return null
  const rest = pathname.slice('/api/'.length)
  if (!rest || rest.startsWith('v1/') || rest.startsWith('_') || rest.startsWith('auth/')) return null
  return `/api/v1/${rest}${query ? `?${query}` : ''}`
}
