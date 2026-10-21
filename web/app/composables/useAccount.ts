/** Account actions shared by the sidebar menu, the header menu and the profile page. */
export type ThemePreference = 'system' | 'light' | 'dark'

export function useAccount() {
  const session = useSessionStore()
  const { clear } = useUserSession()
  const colorMode = useColorMode()

  async function logout() {
    await $fetch('/api/auth/logout', { method: 'POST', headers: { 'x-requested-with': 'fraud-web' } }).catch(() => undefined)
    await clear()
    session.set(null)
    await navigateTo('/login')
  }

  const theme = computed<ThemePreference>({
    get: () => (['light', 'dark'].includes(colorMode.preference) ? colorMode.preference : 'system') as ThemePreference,
    set: (v) => { colorMode.preference = v },
  })
  const themeIcon = (v: ThemePreference) => ({ system: 'i-lucide-monitor', light: 'i-lucide-sun', dark: 'i-lucide-moon' })[v]

  return { logout, theme, themeIcon, resolvedTheme: computed(() => colorMode.value) }
}
