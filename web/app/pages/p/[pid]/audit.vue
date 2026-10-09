<script setup lang="ts">
const { t } = useI18n()
const { apiBase } = useProject()
useHead({ title: () => t('nav.audit') })
</script>

<template>
  <PagePanel
    :title="t('nav.audit')"
    :description="t('audit.description')"
  >
    <AuditTable :path="`${apiBase}/audit`" />
  </PagePanel>
</template>
