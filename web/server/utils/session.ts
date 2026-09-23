/**
 * Per-call session overrides. The cookie is always httpOnly + SameSite=strict (nuxt.config); `Secure` is enabled with
 * NUXT_SESSION_SECURE=true (compose name) or nuxt-auth-utils' native NUXT_SESSION_COOKIE_SECURE=true.
 */
export function sessionOverrides(): { cookie?: { secure: boolean } } {
  const flag = String(useRuntimeConfig().sessionSecure || process.env.NUXT_SESSION_SECURE || '').toLowerCase()
  return flag === 'true' || flag === '1' ? { cookie: { secure: true } } : {}
}
