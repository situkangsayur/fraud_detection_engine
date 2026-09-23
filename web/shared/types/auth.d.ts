// nuxt-auth-utils session typing. `secure` is sealed server-side and never sent to the browser.
declare module '#auth-utils' {
  interface User {
    id: string
    email: string
    full_name: string
    tenant_id: string | null
    tenant_role: 'tenant_admin' | 'member'
    is_platform_admin: boolean
  }

  interface UserSession {
    loggedInAt?: number
  }

  interface SecureSessionData {
    accessToken: string
    refreshToken: string
  }
}

export {}
