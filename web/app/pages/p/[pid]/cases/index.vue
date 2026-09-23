<script setup lang="ts">
import type { CaseSummary, CaseStatus, Page } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const { pid, base, apiBase } = useProject()
const { nameOf } = useMembers()
useHead({ title: () => t('nav.cases') })

const STATUSES: CaseStatus[] = ['open', 'in_review', 'resolved_fraud', 'resolved_legit']
const status = ref(String(route.query.status ?? 'open'))
const priority = ref(ALL)
const page = ref(1)
const pageSize = 50
const { data, status: loadStatus } = await useAsyncData(`cases-${pid.value}`, () => api.get<Page<CaseSummary>>(`${apiBase.value}/cases`, { query: { status: status.value, priority: priority.value, page: page.value, page_size: pageSize } }), { watch: [status, priority, page] })

const columns = computed(() => [
  { accessorKey: 'priority', header: 'P' },
  { accessorKey: 'status', header: t('common.status') },
  { accessorKey: 'customer_external_id', header: t('common.customer') },
  { accessorKey: 'typologies', header: t('common.typologies') },
  { accessorKey: 'final_score', header: t('common.score') },
  { accessorKey: 'event_count', header: t('cases.events') },
  { accessorKey: 'assigned_to', header: t('cases.assignee') },
  { accessorKey: 'created_at', header: t('common.created') },
])
</script>

<template>
  <PagePanel :title="t('nav.cases')">
    <template #toolbar>
      <UTabs
        v-model="status"
        :items="[{ label: t('common.all'), value: '' }, ...STATUSES.map(s => ({ label: t(`status.${s}`), value: s }))]"
        variant="link"
        size="sm"
        :content="false"
      />
      <USelect
        v-model="priority"
        :items="[{ label: t('cases.anyPriority'), value: ALL }, ...[1, 2, 3, 4, 5].map(p => ({ label: `P${p}`, value: String(p) }))]"
        size="sm"
        class="w-36 ml-auto"
      />
    </template>
    <UTable
      :data="data?.items ?? []"
      :columns="columns"
      :loading="loadStatus === 'pending'"
      class="cursor-pointer"
      :empty="t('cases.empty')"
      @select="(_, row) => navigateTo(`${base}/cases/${row.original.id}`)"
    >
      <template #priority-cell="{ row }">
        <UBadge
          :color="row.original.priority <= 1 ? 'error' : row.original.priority <= 2 ? 'warning' : 'neutral'"
          variant="soft"
          size="xs"
        >
          P{{ row.original.priority }}
        </UBadge>
      </template>
      <template #status-cell="{ row }">
        <StatusBadge
          :value="row.original.status"
          size="xs"
        />
      </template>
      <template #customer_external_id-cell="{ row }">
        <span class="fp-mono text-xs">{{ row.original.customer_external_id }}</span>
        <StatusBadge
          v-if="row.original.risk_label === 'fraud'"
          value="fraud"
          size="xs"
          class="ml-1"
        />
      </template>
      <template #typologies-cell="{ row }">
        <div class="flex gap-1 flex-wrap">
          <UBadge
            v-for="ty in row.original.typologies"
            :key="ty"
            color="neutral"
            variant="outline"
            size="xs"
          >
            {{ t(`typologies.${ty}`) }}
          </UBadge>
        </div>
      </template>
      <template #final_score-cell="{ row }">
        <ScoreBar
          :score="row.original.final_score ?? null"
          compact
        />
      </template>
      <template #assigned_to-cell="{ row }">
        {{ nameOf(row.original.assigned_to) ?? '—' }}
      </template>
      <template #created_at-cell="{ row }">
        <span class="text-xs">{{ fmtRelative(row.original.created_at) }}</span>
      </template>
    </UTable>
    <TablePager
      v-model:page="page"
      :total="data?.total ?? 0"
      :page-size="pageSize"
    />
  </PagePanel>
</template>
