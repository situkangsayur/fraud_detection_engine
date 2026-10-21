<script setup lang="ts">
// Account menu: in the sidebar footer (default) and as an avatar button in every page header (`placement="header"`).
import type { DropdownMenuItem } from '@nuxt/ui'
import type { ThemePreference } from '~/composables/useAccount'

const props = defineProps<{ collapsed?: boolean, placement?: 'sidebar' | 'header' }>()
const { t, locale, locales, setLocale } = useI18n()
const session = useSessionStore()
const { logout, theme, themeIcon } = useAccount()

const roleLabel = computed(() => {
  const u = session.user
  if (!u) return ''
  if (u.is_platform_admin) return t('profile.platformAdmin')
  return t(`roles.${u.tenant_role}`)
})

const items = computed<DropdownMenuItem[][]>(() => [
  [{ label: session.user?.full_name ?? '', description: `${session.user?.email ?? ''} · ${roleLabel.value}`, type: 'label' }],
  [
    { label: t('profile.title'), icon: 'i-lucide-user-round', to: '/profile' },
    {
      label: t('common.theme'),
      icon: themeIcon(theme.value),
      children: (['system', 'light', 'dark'] as ThemePreference[]).map(v => ({
        label: t(`common.theme_${v}`),
        icon: themeIcon(v),
        type: 'checkbox' as const,
        checked: theme.value === v,
        onSelect: (e: Event) => { e.preventDefault(); theme.value = v },
      })),
    },
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
  ],
  [{ label: t('about.title'), icon: 'i-lucide-info', to: '/about' }],
  [{ label: t('actions.logout'), icon: 'i-lucide-log-out', color: 'error' as const, onSelect: logout }],
])
const inHeader = computed(() => props.placement === 'header')
</script>

<template>
  <UDropdownMenu
    :items="items"
    :content="inHeader ? { align: 'end', side: 'bottom' } : { align: 'start', side: 'top' }"
    :class="inHeader ? '' : 'w-full'"
  >
    <UButton
      color="neutral"
      variant="ghost"
      :block="!inHeader"
      :square="collapsed"
      :class="inHeader ? 'gap-1.5' : 'justify-start'"
      :aria-label="t('common.account')"
    >
      <UAvatar
        :alt="session.user?.full_name ?? '?'"
        size="xs"
      />
      <span
        v-if="!collapsed && !inHeader"
        class="truncate text-sm flex-1 text-left"
      >{{ session.user?.full_name }}</span>
      <UIcon
        v-if="!collapsed"
        :name="inHeader ? 'i-lucide-chevron-down' : 'i-lucide-chevrons-up-down'"
        class="size-4 text-muted shrink-0"
      />
    </UButton>
  </UDropdownMenu>
</template>
