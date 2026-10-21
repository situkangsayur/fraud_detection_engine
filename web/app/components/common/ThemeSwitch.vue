<script setup lang="ts">
// Theme picker. Default: three labelled buttons (System / Light / Dark). `compact`: one icon button for the page
// header that flips between light and dark (the full choice, including System, stays in the account menu).
import type { ThemePreference } from '~/composables/useAccount'

defineProps<{ compact?: boolean }>()
const { t } = useI18n()
const { theme, themeIcon, resolvedTheme } = useAccount()
const options: ThemePreference[] = ['system', 'light', 'dark']
const label = (v: ThemePreference) => t(`common.theme_${v}`)
</script>

<template>
  <UTooltip
    v-if="compact"
    :text="resolvedTheme === 'dark' ? t('common.lightMode') : t('common.darkMode')"
  >
    <UButton
      color="neutral"
      variant="ghost"
      :icon="resolvedTheme === 'dark' ? 'i-lucide-sun' : 'i-lucide-moon'"
      :aria-label="resolvedTheme === 'dark' ? t('common.lightMode') : t('common.darkMode')"
      @click="theme = resolvedTheme === 'dark' ? 'light' : 'dark'"
    />
  </UTooltip>
  <UFieldGroup
    v-else
    role="radiogroup"
    :aria-label="t('common.theme')"
  >
    <UButton
      v-for="v in options"
      :key="v"
      role="radio"
      :aria-checked="theme === v"
      :icon="themeIcon(v)"
      :label="label(v)"
      :color="theme === v ? 'primary' : 'neutral'"
      :variant="theme === v ? 'solid' : 'outline'"
      @click="theme = v"
    />
  </UFieldGroup>
</template>
