import type { H3Event } from 'h3'
import type { LoginResponse } from '#shared/types/api'
import { buildUpstreamHeaders } from './proxy-headers'
import { upstreamFetch } from './upstream'
import { sessionOverrides } from './session'

// Single-flight refresh per refresh token: parallel requests that all see an expired access token share ONE refresh
// call. Without this, refresh-token rotation with reuse detection (core-api) would revoke the whole token family.
// Note: this map is per Node process — run multiple web replicas with sticky sessions (documented in README).
const inflight = new Map<string, Promise<LoginResponse | null>>()

export async function getAccessToken(event: H3Event): Promise<string | null> {
  const session = await getUserSession(event)
  return session.secure?.accessToken ?? null
}

async function doRefresh(refreshToken: string): Promise<LoginResponse | null> {
  const res = await upstreamFetch(null, '/api/v1/auth/refresh', {
    method: 'POST',
    headers: { ...buildUpstreamHeaders({}), 'content-type': 'application/json', 'accept': 'application/json' },
    body: JSON.stringify({ refresh_token: refreshToken }),
  })
  if (!res.ok) return null
  return await res.json() as LoginResponse
}

/** Refreshes the token pair and stores it in the sealed session. Returns the new access token or null (→ re-login). */
export async function refreshAccessToken(event: H3Event): Promise<string | null> {
  const session = await getUserSession(event)
  const refreshToken = session.secure?.refreshToken
  if (!refreshToken) return null
  let pending = inflight.get(refreshToken)
  if (!pending) {
    pending = doRefresh(refreshToken).finally(() => setTimeout(() => inflight.delete(refreshToken), 5_000))
    inflight.set(refreshToken, pending)
  }
  const pair = await pending
  if (!pair) {
    await clearUserSession(event, sessionOverrides())
    return null
  }
  await setUserSession(event, {
    secure: { accessToken: pair.access_token, refreshToken: pair.refresh_token },
  }, sessionOverrides())
  return pair.access_token
}
