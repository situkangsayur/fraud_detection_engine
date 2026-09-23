// BFF catch-all: /api/<anything> → gateway /api/v1/<anything>.
// Adds the bearer token from the sealed session, refreshes proactively (and once on 401), streams responses back
// (SSE for LLM chat, downloads) and never forwards cookies or upstream Set-Cookie headers.
import { buildUpstreamHeaders, checkCsrf, filterResponseHeaders, isMutatingMethod, shouldRefresh, toUpstreamPath } from '../utils/proxy-headers'
import { upstreamFetch } from '../utils/upstream'
import { refreshAccessToken } from '../utils/tokens'

/** Bodies up to this size are buffered so a request can be replayed after a token refresh. */
const REPLAYABLE_BODY_LIMIT = 10 * 1024 * 1024

export default defineEventHandler(async (event) => {
  const method = event.method.toUpperCase()
  const url = getRequestURL(event)

  const csrf = checkCsrf({ method, headers: getRequestHeaders(event), expectedOrigin: url.origin })
  if (!csrf.ok) return sendProblem(event, 403, 'Forbidden', csrf.reason)

  const upstreamPath = toUpstreamPath(event.path)
  if (!upstreamPath) return sendProblem(event, 404, 'Not found')

  const session = await getUserSession(event)
  let accessToken = session.secure?.accessToken
  if (!accessToken) return sendProblem(event, 401, 'Not signed in')
  if (shouldRefresh(accessToken)) accessToken = (await refreshAccessToken(event)) ?? undefined
  if (!accessToken) return sendProblem(event, 401, 'Not signed in', 'Session expired, please sign in again.')

  let body: BodyInit | null = null
  let replayable = true
  if (isMutatingMethod(method)) {
    const length = Number(getRequestHeader(event, 'content-length') ?? 0)
    if (length > REPLAYABLE_BODY_LIMIT) {
      body = getRequestWebStream(event) ?? null
      replayable = false
    }
    else {
      const raw = await readRawBody(event, false)
      body = raw ? new Uint8Array(raw) : null
    }
  }

  const incoming = getRequestHeaders(event)
  const streaming = (incoming.accept ?? '').includes('text/event-stream')
  const clientIp = getRequestIP(event, { xForwardedFor: true })
  const requestId = buildUpstreamHeaders(incoming)['x-request-id']
  const send = (token: string) => upstreamFetch(event, upstreamPath, {
    method,
    headers: buildUpstreamHeaders(incoming, { accessToken: token, clientIp, requestId }),
    body,
    streaming,
  })

  let res = await send(accessToken)
  if (res.status === 401 && replayable) {
    const fresh = await refreshAccessToken(event)
    if (fresh) res = await send(fresh)
  }

  setResponseStatus(event, res.status, res.statusText)
  setResponseHeaders(event, filterResponseHeaders(res.headers))
  if (!res.body || method === 'HEAD' || res.status === 204) return null
  if ((res.headers.get('content-type') ?? '').includes('text/event-stream')) {
    setResponseHeaders(event, { 'cache-control': 'no-cache, no-transform', 'x-accel-buffering': 'no', 'connection': 'keep-alive' })
  }
  return sendStream(event, res.body)
})
