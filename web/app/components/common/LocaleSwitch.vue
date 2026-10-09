<script setup lang="ts">
// Compact ID/EN toggle for places without the user menu (e.g. the login page). Choice persists in the
// `fraud_locale` cookie (see i18n.detectBrowserLanguage in nuxt.config.ts).
const { locale, locales, setLocale } = useI18n()
const items = computed(() => (locales.value as { code: 'id' | 'en', name?: string }[]).map(l => ({ label: l.code.toUpperCase(), value: l.code, title: l.name })))
</script>

<template>
  <UFieldGroup size="xs">
    <UButton
      v-for="l in items"
      :key="l.value"
      :label="l.label"
      :title="l.title"
      :color="locale === l.value ? 'primary' : 'neutral'"
      :variant="locale === l.value ? 'solid' : 'outline'"
      @click="setLocale(l.value)"
    />
  </UFieldGroup>
</template>
