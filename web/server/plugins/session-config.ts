// Maps the compose env name NUXT_SESSION_SECRET onto nuxt-auth-utils' NUXT_SESSION_PASSWORD (read lazily from
// process.env on the first session access, i.e. after this plugin runs) and fails fast when no secret is configured
// outside mock/dev mode. Runtime config is frozen in production, so we never mutate it.
import { randomBytes } from 'node:crypto'

export default defineNitroPlugin(() => {
  const config = useRuntimeConfig()
  const secret = String(process.env.NUXT_SESSION_PASSWORD || config.sessionSecret || process.env.NUXT_SESSION_SECRET || '')
  const mock = ['1', 'true'].includes(String(config.mockApi || process.env.MOCK_API || '').toLowerCase())

  if (secret.length >= 32) {
    process.env.NUXT_SESSION_PASSWORD = secret
  }
  else if (mock || import.meta.dev) {
    process.env.NUXT_SESSION_PASSWORD = randomBytes(32).toString('hex')
    console.warn('[web] NUXT_SESSION_SECRET not set (or < 32 chars): using an ephemeral secret — sessions reset on restart.')
  }
  else {
    throw new Error('[web] NUXT_SESSION_SECRET must be set to at least 32 characters.')
  }
})
