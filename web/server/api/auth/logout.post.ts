import { buildUpstreamHeaders } from '../../utils/proxy-headers'
import { upstreamFetch } from '../../utils/upstream'
import { getAccessToken } from '../../utils/tokens'

export default defineEventHandler(async (event) => {
  const accessToken = await getAccessToken(event)
  if (accessToken) {
    // Best effort: revoke the refresh-token family server-side. The local session is cleared regardless.
    await upstreamFetch(event, '/api/v1/auth/logout', {
      method: 'POST',
      headers: buildUpstreamHeaders({}, { accessToken }),
    }).catch(() => undefined)
  }
  await clearUserSession(event, sessionOverrides())
  return { ok: true }
})
