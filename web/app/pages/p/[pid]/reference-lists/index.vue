<script setup lang="ts">
const { t } = useI18n()
const { apiBase, can } = useProject()
useHead({ title: () => t('nav.referenceLists') })
</script>

<template>
  <PagePanel
    :title="t('nav.referenceLists')"
    :description="t('lists.description')"
  >
    <ReferenceListsView
      :base-path="`${apiBase}/reference-lists`"
      :can-edit="can('analyst')"
    />
  </PagePanel>
</template>
