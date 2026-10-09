<script setup lang="ts">
import type { DropdownMenuItem } from '@nuxt/ui'

defineProps<{ collapsed?: boolean }>()
const { t, locale, locales, setLocale } = useI18n()
const colorMode = useColorMode()
const session = useSessionStore()
const { clear } = useUserSession()

async function logout() {
  await $fetch('/api/auth/logout', { method: 'POST', headers: { 'x-requested-with': 'fraud-web' } }).catch(() => undefined)
  await clear()
  session.set(null)
  await navigateTo('/login')
}

const items = computed<DropdownMenuItem[][]>(() => [
  [{ label: session.user?.email ?? '', type: 'label' }],
  [
    {
      label: t('common.language'),
      icon: 'i-lucide-languages',
      children: (locales.value as { code: 'id' | 'en', name?: string }[]).map(l => ({
        label: l.name ?? l.code,
        type: 'checkbox' as const,
        checked: locale.value === l.code,
        onSelect: (e: Event) => { e.preventDefault(); setLocale(l.code) },
      })),
    },
    {
      label: colorMode.value === 'dark' ? t('common.lightMode') : t('common.darkMode'),
      icon: colorMode.value === 'dark' ? 'i-lucide-sun' : 'i-lucide-moon',
      onSelect: (e: Event) => { e.preventDefault(); colorMode.preference = colorMode.value === 'dark' ? 'light' : 'dark' },
    },
  ],
  [{ label: t('about.title'), icon: 'i-lucide-info', to: '/about' }],
  [{ label: t('actions.logout'), icon: 'i-lucide-log-out', color: 'error' as const, onSelect: logout }],
])
</script>

<template>
  <UDropdownMenu
    :items="items"
    :content="{ align: 'start', side: 'top' }"
    class="w-full"
  >
    <UButton
      color="neutral"
      variant="ghost"
      block
      :square="collapsed"
      class="justify-start"
      :aria-label="t('common.account')"
    >
      <UAvatar
        :alt="session.user?.full_name ?? '?'"
        size="xs"
      />
      <span
        v-if="!collapsed"
        class="truncate text-sm"
      >{{ session.user?.full_name }}</span>
    </UButton>
  </UDropdownMenu>
</template>
