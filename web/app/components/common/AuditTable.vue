<script setup lang="ts">
import type { AuditEntry, Page } from '#shared/types/api'

const props = defineProps<{ path: string }>()
const { t } = useI18n()
const api = useApi()
const filters = reactive({ action: '', actor: '' })
const df = useDebounced(computed(() => ({ ...filters })))
const page = ref(1)
const { data, status } = await useAsyncData(`audit-${props.path}`, () => api.get<Page<AuditEntry>>(props.path, { query: { ...df.value, page: page.value, page_size: 50 } }), { watch: [df, page] })
const selected = ref<AuditEntry | null>(null)
</script>

<template>
  <div>
    <div class="flex gap-2 mb-3">
      <UInput
        v-model="filters.action"
        icon="i-lucide-search"
        :placeholder="t('audit.action')"
        class="w-56"
      />
      <UInput
        v-model="filters.actor"
        icon="i-lucide-user"
        :placeholder="t('audit.actor')"
        class="w-56"
      />
    </div>
    <UTable
      :data="data?.items ?? []"
      :loading="status === 'pending'"
      :columns="[{ accessorKey: 'occurred_at', header: t('audit.when') }, { accessorKey: 'actor_id', header: t('audit.actor') }, { accessorKey: 'action', header: t('audit.action') }, { accessorKey: 'subject', header: t('audit.subject') }, { accessorKey: 'request_id', header: 'Request ID' }]"
      class="cursor-pointer"
      @select="(_, row) => (selected = row.original)"
    >
      <template #occurred_at-cell="{ row }">
        <span class="text-xs">{{ fmtDate(row.original.occurred_at) }}</span>
      </template>
      <template #actor_id-cell="{ row }">
        <UBadge
          :color="row.original.actor_type === 'user' ? 'neutral' : 'info'"
          variant="soft"
          size="xs"
        >
          {{ row.original.actor_type }}
        </UBadge>
        <span class="text-xs ml-1">{{ row.original.actor_id }}</span>
      </template>
      <template #action-cell="{ row }">
        <span class="fp-mono text-xs">{{ row.original.action }}</span>
      </template>
      <template #subject-cell="{ row }">
        <span class="text-xs">{{ row.original.subject_type }} <span class="fp-mono text-muted">{{ row.original.subject_id }}</span></span>
      </template>
      <template #request_id-cell="{ row }">
        <span class="fp-mono text-xs text-muted">{{ row.original.request_id }}</span>
      </template>
    </UTable>
    <TablePager
      v-model:page="page"
      :total="data?.total ?? 0"
      :page-size="50"
    />
    <USlideover
      :open="!!selected"
      :title="selected?.action"
      :ui="{ content: 'max-w-2xl' }"
      @update:open="(v: boolean) => { if (!v) selected = null }"
    >
      <template #body>
        <JsonDiff
          v-if="selected"
          :before="selected.before ?? undefined"
          :after="selected.after ?? undefined"
        />
        <p class="text-xs text-muted uppercase mt-4 mb-1">
          metadata
        </p>
        <JsonTree
          v-if="selected"
          :value="selected.metadata"
          :open="true"
        />
      </template>
    </USlideover>
  </div>
</template>
