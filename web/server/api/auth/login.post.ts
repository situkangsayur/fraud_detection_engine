import { z } from 'zod'
import type { LoginResponse, Me } from '#shared/types/api'
import { buildUpstreamHeaders } from '../../utils/proxy-headers'
import { upstreamFetch } from '../../utils/upstream'

const LoginBody = z.object({
  email: z.string().email().max(320),
  password: z.string().min(1).max(512),
})

export default defineEventHandler(async (event) => {
  const parsed = LoginBody.safeParse(await readBody(event).catch(() => null))
  if (!parsed.success) return sendProblem(event, 422, 'Validation failed', 'Email and password are required.')

  const headers = buildUpstreamHeaders(getRequestHeaders(event), { clientIp: getRequestIP(event, { xForwardedFor: true }) })
  const res = await upstreamFetch(event, '/api/v1/auth/login', {
    method: 'POST',
    headers: { ...headers, 'content-type': 'application/json', 'accept': 'application/json' },
    body: JSON.stringify(parsed.data),
  })
  if (!res.ok) {
    setResponseStatus(event, res.status)
    setResponseHeader(event, 'content-type', res.headers.get('content-type') ?? 'application/problem+json')
    return await res.text()
  }
  const pair = await res.json() as LoginResponse

  // Load memberships once so the UI can render immediately; the store refreshes /me later.
  const meRes = await upstreamFetch(event, '/api/v1/me', {
    method: 'GET',
    headers: { ...buildUpstreamHeaders({}, { accessToken: pair.access_token }), accept: 'application/json' },
  })
  const me = meRes.ok ? await meRes.json() as Me : null

  await replaceUserSession(event, {
    user: {
      id: pair.user.id,
      email: pair.user.email,
      full_name: pair.user.full_name,
      tenant_id: pair.user.tenant_id,
      tenant_role: pair.user.tenant_role,
      is_platform_admin: pair.user.is_platform_admin,
    },
    secure: { accessToken: pair.access_token, refreshToken: pair.refresh_token },
    loggedInAt: Date.now(),
  }, sessionOverrides())
  return { user: pair.user, me }
})
