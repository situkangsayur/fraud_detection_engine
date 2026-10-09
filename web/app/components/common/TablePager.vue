<script setup lang="ts">
const page = defineModel<number>('page', { required: true })
defineProps<{ total: number, pageSize: number }>()
const { t } = useI18n()
</script>

<template>
  <div class="flex items-center justify-between gap-2 pt-3 text-sm text-muted">
    <span>{{ t('common.totalItems', { n: fmtNumber(total) }) }}</span>
    <UPagination
      v-if="total > pageSize"
      v-model:page="page"
      :total="total"
      :items-per-page="pageSize"
      size="sm"
    />
  </div>
</template>
