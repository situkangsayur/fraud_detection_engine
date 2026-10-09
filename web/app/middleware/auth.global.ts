import { ApiError } from '~/composables/useApi'

// Every page except /login requires a session. Membership data (/me) is loaded once per session.
export default defineNuxtRouteMiddleware(async (to) => {
  const { loggedIn } = useUserSession()
  if (to.path === '/about') return // public: license & source offer
  if (to.path === '/login') {
    if (loggedIn.value) return navigateTo('/')
    return
  }
  if (!loggedIn.value) return navigateTo({ path: '/login', query: to.fullPath !== '/' ? { next: to.fullPath } : undefined })

  const session = useSessionStore()
  try {
    await session.load()
  }
  catch (err) {
    // Only an expired/invalid session sends the user back to /login (after clearing it, otherwise /login would
    // bounce straight back here). Anything else (gateway down, 5xx) surfaces as an error page — never a redirect loop.
    if (err instanceof ApiError && err.status === 401) {
      await useUserSession().clear()
      return navigateTo({ path: '/login', query: { next: to.fullPath } })
    }
    throw createError({ statusCode: err instanceof ApiError ? err.status || 503 : 503, statusMessage: err instanceof Error ? err.message : 'Platform unavailable', fatal: true })
  }

  // Project routes: the user must be a member (tenant admins are implicitly project_admin). Unknown ids — e.g. a
  // bookmark or `?next=` from before a demo reset re-created the projects — go back to the project list.
  const pid = to.params.pid as string | undefined
  if (pid && !session.roleIn(pid)) return navigateTo('/projects')
  if (to.path.startsWith('/admin') && !session.isPlatformAdmin) return navigateTo('/')
  if (to.path.startsWith('/tenant') && !session.isTenantAdmin && !session.isPlatformAdmin && !to.path.startsWith('/tenant/regulations')) return navigateTo('/')
})
