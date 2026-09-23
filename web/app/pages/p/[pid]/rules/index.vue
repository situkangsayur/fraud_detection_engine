<script setup lang="ts">
import type { Page, Rule } from '#shared/types/api'
import { RULE_KINDS } from '#shared/rules/dsl'
import { RULE_EXAMPLES } from '#shared/rules/examples'
import { describeDefinition } from '#shared/rules/model'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const { pid, base, apiBase, can } = useProject()
useHead({ title: () => t('nav.rules') })

const filters = reactive({ q: '', kind: orAll(route.query.kind), status: orAll(route.query.status), typology: ALL })
const debouncedQ = useDebounced(toRef(filters, 'q'))
const page = ref(1)
const pageSize = 50
const query = computed(() => ({ ...filters, q: debouncedQ.value, page: page.value, page_size: pageSize }))
const { data, status } = await useAsyncData(`rules-${pid.value}`, () => api.get<Page<Rule>>(`${apiBase.value}/rules`, { query: query.value }), { watch: [query] })

const newItems = computed(() => [
  RULE_KINDS.map(k => ({ label: t(`kinds.${k}`), description: t(`kinds.help.${k}`), icon: 'i-lucide-plus', to: `${base.value}/rules/new?kind=${k}` })),
  [{ label: t('rules.fromTemplate'), type: 'label' as const }, ...RULE_EXAMPLES.map(e => ({ label: `${e.code} · ${e.name}`, icon: 'i-lucide-copy', to: `${base.value}/rules/new?template=${e.code}` }))],
])
const columns = computed(() => [
  { accessorKey: 'code', header: t('common.code') },
  { accessorKey: 'name', header: t('common.name') },
  { accessorKey: 'kind', header: t('common.kind') },
  { accessorKey: 'status', header: t('common.status') },
  { accessorKey: 'risk', header: t('rules.riskScore') },
  { accessorKey: 'stats', header: t('rules.stats7d') },
  { accessorKey: 'updated_at', header: t('common.updated') },
])
</script>

<template>
  <PagePanel :title="t('nav.rules')">
    <template #actions>
      <UDropdownMenu
        v-if="can('analyst')"
        :items="newItems"
        :content="{ align: 'end' }"
        :ui="{ content: 'w-80' }"
      >
        <UButton
          icon="i-lucide-plus"
          :label="t('actions.newRule')"
          trailing-icon="i-lucide-chevron-down"
        />
      </UDropdownMenu>
    </template>
    <template #toolbar>
      <div class="flex flex-wrap gap-2 w-full">
        <UInput
          v-model="filters.q"
          icon="i-lucide-search"
          :placeholder="t('common.search')"
          class="w-56"
        />
        <USelect
          v-model="filters.kind"
          :items="[{ label: t('common.allKinds'), value: ALL }, ...RULE_KINDS.map(k => ({ label: t(`kinds.${k}`), value: k }))]"
          class="w-40"
        />
        <USelect
          v-model="filters.status"
          :items="[{ label: t('common.allStatuses'), value: ALL }, ...['draft', 'pending_approval', 'active', 'shadow', 'retired'].map(s => ({ label: t(`status.${s}`), value: s }))]"
          class="w-44"
        />
        <USelect
          v-model="filters.typology"
          :items="[{ label: t('common.allTypologies'), value: ALL }, ...['carding', 'account_takeover', 'bank_account_takeover', 'system_breach', 'promo_abuse', 'refund_abuse', 'money_mule', 'other'].map(x => ({ label: t(`typologies.${x}`), value: x }))]"
          class="w-48"
        />
      </div>
    </template>
    <UTable
      :data="data?.items ?? []"
      :columns="columns"
      :loading="status === 'pending'"
      class="cursor-pointer"
      :empty="t('rules.empty')"
      @select="(_, row) => navigateTo(`${base}/rules/${row.original.id}`)"
    >
      <template #code-cell="{ row }">
        <span class="fp-mono text-xs font-medium">{{ row.original.code }}</span>
        <span class="text-xs text-muted"> v{{ row.original.current_version }}</span>
      </template>
      <template #name-cell="{ row }">
        <div class="max-w-md">
          <p class="truncate">
            {{ row.original.name }}
          </p>
          <p class="text-xs text-muted truncate fp-mono">
            {{ describeDefinition(row.original.envelope.definition) }}
          </p>
        </div>
      </template>
      <template #kind-cell="{ row }">
        <UBadge
          color="neutral"
          variant="soft"
          size="sm"
        >
          {{ t(`kinds.${row.original.kind}`) }}
        </UBadge>
      </template>
      <template #status-cell="{ row }">
        <StatusBadge
          :value="row.original.status"
          size="xs"
        />
        <UTooltip
          v-if="row.original.serving?.shadow_version && row.original.status !== 'shadow'"
          :text="t('rules.shadowVersionRunning', { v: row.original.serving.shadow_version })"
        >
          <UBadge
            color="info"
            variant="outline"
            size="xs"
            class="ml-1"
          >
            v{{ row.original.serving.shadow_version }} {{ t('status.shadow') }}
          </UBadge>
        </UTooltip>
      </template>
      <template #risk-cell="{ row }">
        <span class="tabular-nums">{{ row.original.envelope.risk_score }}</span>
        <UBadge
          v-if="row.original.envelope.action && row.original.envelope.action !== 'score'"
          color="warning"
          variant="soft"
          size="xs"
          class="ml-1"
        >
          {{ t(`rules.actions.${row.original.envelope.action}`) }}
        </UBadge>
      </template>
      <template #stats-cell="{ row }">
        <span
          v-if="row.original.stats_7d"
          class="text-xs tabular-nums"
        >
          {{ fmtNumber(row.original.stats_7d.matched) }} / {{ fmtCompact(row.original.stats_7d.evaluated) }}
          <span
            v-if="row.original.stats_7d.trapped"
            class="text-warning"
          > · {{ row.original.stats_7d.trapped }} {{ t('status.trapped') }}</span>
        </span>
      </template>
      <template #updated_at-cell="{ row }">
        <span class="text-xs">{{ fmtRelative(row.original.updated_at) }}</span>
      </template>
    </UTable>
    <TablePager
      v-model:page="page"
      :total="data?.total ?? 0"
      :page-size="pageSize"
    />
  </PagePanel>
</template>
