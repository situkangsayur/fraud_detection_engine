<script setup lang="ts">
// About / license page — public (no login needed) so the source offer is always reachable (AGPL-3.0 §13).
definePageMeta({ layout: 'auth' })
const { t } = useI18n()
const config = useRuntimeConfig()
const { loggedIn } = useUserSession()
useHead({ title: () => t('about.title') })
</script>

<template>
  <UCard class="w-full max-w-xl">
    <template #header>
      <div class="flex items-center gap-2">
        <UIcon
          name="i-lucide-shield-check"
          class="size-7 text-primary"
        />
        <div>
          <h1 class="font-semibold text-lg">
            Fraud Detection Platform
          </h1>
          <p class="text-xs text-muted">
            {{ t('about.subtitle') }}
          </p>
        </div>
      </div>
    </template>
    <KeyValue
      :items="[
        { label: t('about.version'), value: String(config.public.appVersion), mono: true },
        { key: 'license', label: t('about.license'), value: 'AGPL-3.0-only' },
        { label: t('about.author'), value: 'Hendri Karisma' },
        { key: 'source', label: t('about.source'), value: String(config.public.sourceUrl) },
      ]"
    >
      <template #license>
        <a
          href="https://www.gnu.org/licenses/agpl-3.0.html"
          target="_blank"
          rel="noopener"
          class="text-primary"
        >GNU Affero General Public License v3.0 only (AGPL-3.0-only)</a>
      </template>
      <template #source="{ value }">
        <a
          :href="String(value)"
          target="_blank"
          rel="noopener"
          class="text-primary break-all"
        >{{ value }}</a>
      </template>
    </KeyValue>
    <UAlert
      class="mt-4"
      color="neutral"
      variant="subtle"
      icon="i-lucide-scale"
      :description="t('about.attributionNotice')"
    />
    <p class="text-sm text-muted mt-3">
      {{ t('about.networkNotice') }}
    </p>
    <template #footer>
      <div class="flex justify-between items-center">
        <UButton
          :to="loggedIn ? '/' : '/login'"
          variant="ghost"
          icon="i-lucide-arrow-left"
          :label="t('actions.back')"
        />
        <AppFooter />
      </div>
    </template>
  </UCard>
</template>
