<script setup lang="ts">
import type { Customer, EventSummary, Page } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const { base, apiBase } = useProject()
const id = computed(() => String(route.params.id))
const { data: c } = await useAsyncData(`customer-${id.value}`, () => api.get<Customer>(`${apiBase.value}/customers/${id.value}`))
const { data: events } = await useAsyncData(`customer-events-${id.value}`, () => api.get<Page<EventSummary>>(`${apiBase.value}/customers/${id.value}/events`, { query: { page_size: 20 } }))
useHead({ title: () => `${t('common.customer')} ${c.value?.external_id ?? ''}` })
</script>

<template>
  <PagePanel :title="`${t('common.customer')} ${c?.external_id ?? ''}`">
    <template #actions>
      <UButton
        :to="`${base}/graph?customer=${id}`"
        icon="i-lucide-share-2"
        :label="t('actions.openGraph')"
        color="neutral"
        variant="outline"
      />
      <UButton
        :to="`${base}/events?customer_id=${id}`"
        icon="i-lucide-activity"
        :label="t('nav.events')"
        color="neutral"
        variant="outline"
      />
    </template>
    <div
      v-if="c"
      class="grid lg:grid-cols-3 gap-4"
    >
      <UCard>
        <KeyValue
          :items="[
            { label: t('common.name'), value: c.full_name },
            { label: t('common.email'), value: c.email },
            { label: t('customers.phone'), value: c.phone },
            { key: 'risk', label: t('customers.riskLabel'), value: c.risk_label },
            { label: t('common.status'), value: c.status },
            { label: t('customers.registered'), value: fmtDate(c.registered_at) },
          ]"
        >
          <template #risk>
            <StatusBadge :value="c.risk_label" />
          </template>
        </KeyValue>
      </UCard>
      <div class="lg:col-span-2 grid grid-cols-3 gap-4 content-start">
        <StatCard
          :label="t('customers.events30d')"
          :value="fmtNumber(c.stats?.events_30d)"
          icon="i-lucide-activity"
        />
        <StatCard
          :label="t('customers.declines30d')"
          :value="fmtNumber(c.stats?.declines_30d)"
          icon="i-lucide-ban"
        />
        <StatCard
          :label="t('customers.avgScore30d')"
          :value="fmtNumber(c.stats?.avg_score_30d, 1)"
          icon="i-lucide-gauge"
        />
      </div>
      <UCard class="lg:col-span-3">
        <template #header>
          <h3 class="font-medium">
            {{ t('customers.recentEvents') }}
          </h3>
        </template>
        <UTable
          :data="events?.items ?? []"
          :columns="[{ accessorKey: 'occurred_at', header: t('common.occurred') }, { accessorKey: 'external_id', header: t('common.externalId') }, { accessorKey: 'event_type', header: t('common.type') }, { accessorKey: 'amount', header: t('common.amount') }, { accessorKey: 'decision', header: t('common.decision') }]"
        >
          <template #occurred_at-cell="{ row }">
            {{ fmtDate(row.original.occurred_at) }}
          </template>
          <template #external_id-cell="{ row }">
            <NuxtLink
              :to="`${base}/events/${row.original.id}`"
              class="fp-mono text-xs text-primary"
            >
              {{ row.original.external_id }}
            </NuxtLink>
          </template>
          <template #amount-cell="{ row }">
            {{ fmtMoney(row.original.amount) }}
          </template>
          <template #decision-cell="{ row }">
            <StatusBadge
              :value="row.original.decision"
              size="xs"
            />
          </template>
        </UTable>
      </UCard>
    </div>
  </PagePanel>
</template>
