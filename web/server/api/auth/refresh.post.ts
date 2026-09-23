import { refreshAccessToken } from '../../utils/tokens'

export default defineEventHandler(async (event) => {
  const token = await refreshAccessToken(event)
  if (!token) return sendProblem(event, 401, 'Not signed in', 'Session expired, please sign in again.')
  return { ok: true }
})
