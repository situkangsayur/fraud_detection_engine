<script setup lang="ts">
import type { Decision, EventSummary, Page } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const router = useRouter()
const { pid, base, apiBase } = useProject()
useHead({ title: () => t('nav.events') })

const EVENT_TYPES = ['transaction', 'login', 'account_change', 'promo_redemption', 'payout', 'registration', 'refund']
const filters = reactive({
  q: String(route.query.q ?? ''),
  event_type: orAll(route.query.event_type),
  decision: orAll(route.query.decision) as Decision | typeof ALL,
  min_score: route.query.min_score ? Number(route.query.min_score) : undefined as number | undefined,
  customer_id: String(route.query.customer_id ?? ''),
})
const page = ref(Number(route.query.page ?? 1))
const pageSize = 50
const debouncedQ = useDebounced(toRef(filters, 'q'), 300)
const query = computed(() => ({ ...filters, q: debouncedQ.value, page: page.value, page_size: pageSize }))
watch(query, q => router.replace({ query: Object.fromEntries(Object.entries(q).filter(([k, v]) => v !== '' && v !== ALL && v !== undefined && k !== 'page_size')) }))
watch(() => [filters.event_type, filters.decision, filters.min_score, debouncedQ.value], () => { page.value = 1 })

const { data, status, refresh } = await useAsyncData(`events-${pid.value}`, () => api.get<Page<EventSummary>>(`${apiBase.value}/events`, { query: query.value }), { watch: [query] })

const columns = computed(() => [
  { accessorKey: 'occurred_at', header: t('common.occurred') },
  { accessorKey: 'external_id', header: t('common.externalId') },
  { accessorKey: 'event_type', header: t('common.type') },
  { accessorKey: 'customer_external_id', header: t('common.customer') },
  { accessorKey: 'amount', header: t('common.amount') },
  { accessorKey: 'channel', header: t('common.channel') },
  { accessorKey: 'decision', header: t('common.decision') },
  { accessorKey: 'final_score', header: t('common.score') },
])
function open(_: Event, row: { original: EventSummary }) {
  navigateTo(`${base.value}/events/${row.original.id}`)
}
</script>

<template>
  <PagePanel :title="t('nav.events')">
    <template #actions>
      <UButton
        icon="i-lucide-refresh-cw"
        color="neutral"
        variant="ghost"
        :aria-label="t('actions.refresh')"
        :loading="status === 'pending'"
        @click="() => refresh()"
      />
    </template>
    <template #toolbar>
      <div class="flex flex-wrap gap-2 w-full">
        <UInput
          v-model="filters.q"
          icon="i-lucide-search"
          :placeholder="t('events.searchPlaceholder')"
          class="w-64"
        />
        <USelect
          v-model="filters.event_type"
          :items="[{ label: t('common.allTypes'), value: ALL }, ...EVENT_TYPES.map(e => ({ label: e, value: e }))]"
          class="w-48"
        />
        <USelect
          v-model="filters.decision"
          :items="[{ label: t('common.allDecisions'), value: ALL }, ...(['approve', 'review', 'decline'] as const).map(d => ({ label: t(`decision.${d}`), value: d }))]"
          class="w-44"
        />
        <UInputNumber
          v-model="filters.min_score"
          :min="0"
          :max="100"
          :placeholder="t('events.minScore')"
          class="w-36"
        />
        <UBadge
          v-if="filters.customer_id"
          color="neutral"
          variant="soft"
          class="gap-1"
        >
          {{ t('common.customer') }}: {{ shortId(filters.customer_id) }}
          <UButton
            size="xs"
            variant="link"
            color="neutral"
            icon="i-lucide-x"
            :aria-label="t('actions.clear')"
            @click="filters.customer_id = ''"
          />
        </UBadge>
      </div>
    </template>

    <UTable
      :data="data?.items ?? []"
      :columns="columns"
      :loading="status === 'pending'"
      class="cursor-pointer"
      :empty="t('common.empty')"
      @select="open"
    >
      <template #occurred_at-cell="{ row }">
        <span
          class="text-xs whitespace-nowrap"
          :title="row.original.occurred_at"
        >{{ fmtDate(row.original.occurred_at) }}</span>
      </template>
      <template #external_id-cell="{ row }">
        <span class="fp-mono text-xs">{{ row.original.external_id }}</span>
      </template>
      <template #customer_external_id-cell="{ row }">
        <NuxtLink
          :to="`${base}/customers/${row.original.customer_id}`"
          class="fp-mono text-xs hover:text-primary"
          @click.stop
        >
          {{ row.original.customer_external_id }}
        </NuxtLink>
      </template>
      <template #amount-cell="{ row }">
        <span class="tabular-nums">{{ fmtMoney(row.original.amount, row.original.currency ?? 'IDR') }}</span>
      </template>
      <template #decision-cell="{ row }">
        <StatusBadge
          :value="row.original.decision"
          size="xs"
        />
      </template>
      <template #final_score-cell="{ row }">
        <ScoreBar
          :score="row.original.final_score"
          compact
        />
      </template>
    </UTable>
    <TablePager
      v-model:page="page"
      :total="data?.total ?? 0"
      :page-size="pageSize"
    />
  </PagePanel>
</template>
