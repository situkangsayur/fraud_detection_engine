<script setup lang="ts">
import type { NuxtError } from '#app'

const props = defineProps<{ error: NuxtError }>()
const { t } = useI18n()
useHead({ title: () => String(props.error.statusCode ?? 'Error') })
</script>

<template>
  <UApp>
    <div class="min-h-screen grid place-items-center p-6">
      <UEmpty
        :icon="error.statusCode === 404 ? 'i-lucide-search-x' : 'i-lucide-server-crash'"
        :title="error.statusCode === 404 ? t('errors.notFound') : t('errors.generic')"
        :description="error.statusMessage || error.message"
        :actions="[{ label: t('actions.back'), icon: 'i-lucide-arrow-left', onClick: () => clearError({ redirect: '/' }) }]"
      />
      <AppFooter />
    </div>
  </UApp>
</template>
