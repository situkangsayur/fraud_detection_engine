<script setup lang="ts">
import type { FieldCatalogEntry, Page } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const toast = useToast()
const { pid, apiBase, can } = useProject()
useHead({ title: () => t('nav.fieldCatalog') })
const entity = ref('')
const q = ref('')
const dq = useDebounced(q)
const { data, refresh } = await useAsyncData(`catalog-page-${pid.value}`, () => api.get<Page<FieldCatalogEntry>>(`${apiBase.value}/field-catalog`, { query: { entity: entity.value, q: dq.value, page_size: 200 } }), { watch: [entity, dq] })
async function toggleVelocity(f: FieldCatalogEntry, on: boolean) {
  await api.patch(`${apiBase.value}/field-catalog/${encodeURIComponent(f.path)}`, { velocity_enabled: on })
  toast.add({ title: t('catalog.updated'), color: 'success' })
  await refresh()
}
</script>

<template>
  <PagePanel
    :title="t('nav.fieldCatalog')"
    :description="t('catalog.description')"
  >
    <template #toolbar>
      <UInput
        v-model="q"
        icon="i-lucide-search"
        :placeholder="t('common.search')"
        class="w-64"
      />
      <UTabs
        v-model="entity"
        :items="[{ label: t('common.all'), value: '' }, ...['event', 'source', 'customer', 'features', 'ml', 'graph'].map(e => ({ label: e, value: e }))]"
        size="sm"
        variant="link"
        :content="false"
        class="ml-2"
      />
    </template>
    <UTable
      :data="data?.items ?? []"
      :columns="[{ accessorKey: 'path', header: t('catalog.path') }, { accessorKey: 'data_type', header: t('common.type') }, { accessorKey: 'entity', header: t('catalog.entity') }, { accessorKey: 'velocity_enabled', header: t('catalog.velocity') }, { accessorKey: 'pii', header: 'PII' }, { accessorKey: 'description', header: t('common.description') }]"
    >
      <template #path-cell="{ row }">
        <span class="fp-mono text-xs">{{ row.original.path }}</span>
        <UBadge
          v-if="!row.original.builtin"
          color="info"
          variant="soft"
          size="xs"
          class="ml-1"
        >
          source
        </UBadge>
      </template>
      <template #velocity_enabled-cell="{ row }">
        <USwitch
          :model-value="row.original.velocity_enabled"
          size="xs"
          :disabled="!can('project_admin') || row.original.builtin"
          @update:model-value="(v: boolean) => toggleVelocity(row.original, v)"
        />
      </template>
      <template #pii-cell="{ row }">
        <UIcon
          v-if="row.original.pii"
          name="i-lucide-shield-alert"
          class="text-warning"
        />
      </template>
    </UTable>
  </PagePanel>
</template>
