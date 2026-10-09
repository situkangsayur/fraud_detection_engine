<script setup lang="ts">
import type { FraudProximity, GraphComponent, GraphNode, GraphSearchResult, GraphStats, LinkKind, Neighborhood } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const router = useRouter()
const { pid, base, apiBase, project } = useProject()
useHead({ title: () => t('nav.graph') })

const LINK_KINDS: LinkKind[] = ['email', 'phone', 'device', 'ip', 'card', 'bank_account', 'address', 'ref_transaction', 'api_client']
const customerId = ref<string | undefined>((route.query.customer as string) || undefined)
const opts = reactive({ depth: 2, include_similar: true, link_kinds: [...(project.value?.graph_config.link_kinds ?? ['email', 'phone', 'device', 'card', 'bank_account', 'address', 'ref_transaction'])] as LinkKind[] })

// search
const q = ref('')
const dq = useDebounced(q)
// Search needs ≥ 2 characters (gateway returns 422 otherwise); the current customer stays selectable.
const { data: hits } = await useAsyncData<{ label: string, value: string }[]>(`graph-search-${pid.value}`, async () => {
  if (dq.value.trim().length < 2) return customerId.value ? [{ label: shortId(customerId.value), value: customerId.value }] : []
  const r = await api.get<GraphSearchResult>(`${apiBase.value}/graph/search`, { query: { q: dq.value.trim() }, silent: true }).catch(() => null)
  return (r?.customers ?? []).map(c => ({ label: `${c.external_id}${c.risk_label === 'fraud' ? ' · fraud' : ''}`, value: c.id }))
}, { default: () => [], watch: [dq] })

const graph = ref<Neighborhood>({ nodes: [], edges: [] })
const proximity = ref<FraudProximity | null>(null)
const loading = ref(false)
async function load() {
  if (!customerId.value) return
  loading.value = true
  router.replace({ query: { ...route.query, customer: customerId.value } })
  try {
    const query = { depth: opts.depth, include_similar: opts.include_similar, link_kinds: opts.link_kinds.join(','), limit_nodes: 300 }
    const [n, p] = await Promise.all([
      api.get<Neighborhood>(`${apiBase.value}/graph/customers/${customerId.value}/neighborhood`, { query }),
      api.get<FraudProximity>(`${apiBase.value}/graph/customers/${customerId.value}/fraud-proximity`, { query: { max_depth: 3, include_similar: opts.include_similar, link_kinds: opts.link_kinds.join(',') }, silent: true }).catch(() => null),
    ])
    graph.value = n
    proximity.value = p
  }
  catch { /* toast */ }
  finally { loading.value = false }
}
watch([customerId, () => opts.depth, () => opts.include_similar, () => opts.link_kinds.join(',')], load)
onMounted(load)

// expand: merge a neighbour's neighbourhood into the current graph
async function expand(node: GraphNode) {
  if (node.type !== 'customer') return
  const more = await api.get<Neighborhood>(`${apiBase.value}/graph/customers/${node.id}/neighborhood`, { query: { depth: 1, include_similar: opts.include_similar, link_kinds: opts.link_kinds.join(',') } })
  const ids = new Set(graph.value.nodes.map(n => n.id))
  const eids = new Set(graph.value.edges.map(e => e.id))
  graph.value = {
    nodes: [...graph.value.nodes, ...more.nodes.filter(n => !ids.has(n.id)).map(n => ({ ...n, is_center: false }))],
    edges: [...graph.value.edges, ...more.edges.filter(e => !eids.has(e.id))],
  }
}
const selected = ref<GraphNode | null>(null)
const showPath = ref(true)

const { data: stats } = await useAsyncData(`graph-stats-${pid.value}`, () => api.get<GraphStats>(`${apiBase.value}/graph/stats`, { silent: true }).catch(() => null))
const onlyFraud = ref(true)
const { data: components } = await useAsyncData<GraphComponent[]>(`graph-components-${pid.value}`, () => api.get<GraphComponent[]>(`${apiBase.value}/graph/components`, { query: { min_size: 3, only_with_fraud: onlyFraud.value }, silent: true }).catch(() => []), { default: () => [] as GraphComponent[], watch: [onlyFraud] })
const canvas = ref<{ fit: () => void, exportPng: () => void }>()
</script>

<template>
  <PagePanel
    :title="t('nav.graph')"
    :description="t('graph.description')"
  >
    <template #toolbar>
      <div class="flex flex-wrap items-center gap-2 w-full">
        <USelectMenu
          v-model="customerId"
          v-model:search-term="q"
          :items="hits"
          value-key="value"
          :placeholder="t('graph.searchCustomer')"
          icon="i-lucide-search"
          class="w-80"
          ignore-filter
        />
        <USelectMenu
          v-model="opts.link_kinds"
          :items="LINK_KINDS"
          multiple
          class="w-72"
          :placeholder="t('graph.linkKinds')"
        />
        <USelect
          v-model="opts.depth"
          :items="[1, 2, 3].map(d => ({ label: `${t('graph.depth')} ${d}`, value: d }))"
          class="w-28"
        />
        <USwitch
          v-model="opts.include_similar"
          :label="t('graph.includeSimilar')"
          size="sm"
        />
        <div class="ml-auto flex gap-1">
          <UButton
            icon="i-lucide-maximize"
            color="neutral"
            variant="ghost"
            :aria-label="t('graph.fit')"
            @click="canvas?.fit()"
          />
          <UButton
            icon="i-lucide-image-down"
            color="neutral"
            variant="ghost"
            :aria-label="t('graph.exportPng')"
            @click="canvas?.exportPng()"
          />
        </div>
      </div>
    </template>

    <div class="grid xl:grid-cols-[1fr_20rem] gap-4">
      <UCard :ui="{ body: 'p-0 sm:p-0 h-[560px] relative' }">
        <UEmpty
          v-if="!customerId"
          icon="i-lucide-share-2"
          :title="t('graph.pickCustomer')"
          :description="t('graph.pickCustomerHelp')"
          class="h-full"
        />
        <ClientOnly v-else>
          <GraphCanvas
            ref="canvas"
            :nodes="graph.nodes"
            :edges="graph.edges"
            :highlight-path="showPath ? proximity?.path.map(n => n.id) : []"
            @select="n => (selected = n)"
            @expand="expand"
          />
        </ClientOnly>
        <UBadge
          v-if="graph.truncated"
          color="warning"
          variant="soft"
          size="sm"
          class="absolute top-2 left-2"
        >
          {{ t('graph.truncated') }}
        </UBadge>
        <div
          v-if="loading"
          class="absolute inset-0 flex items-center justify-center bg-default/40"
        >
          <UIcon
            name="i-lucide-loader-circle"
            class="size-8 animate-spin text-primary"
          />
        </div>
        <div class="absolute bottom-2 left-2 flex flex-wrap gap-1 text-[10px]">
          <UBadge
            color="error"
            variant="soft"
            size="xs"
          >
            ● {{ t('status.fraud') }}
          </UBadge>
          <UBadge
            color="primary"
            variant="soft"
            size="xs"
          >
            ● {{ t('common.customer') }}
          </UBadge>
          <UBadge
            color="neutral"
            variant="soft"
            size="xs"
          >
            ■ {{ t('graph.entity') }}
          </UBadge>
          <UBadge
            color="warning"
            variant="soft"
            size="xs"
          >
            ┄ {{ t('graph.similar') }}
          </UBadge>
          <span class="text-muted self-center ml-1">{{ t('graph.dblClickExpand') }}</span>
        </div>
      </UCard>

      <div class="space-y-4">
        <UCard v-if="proximity">
          <template #header>
            <div class="flex items-center justify-between">
              <h3 class="font-medium">
                {{ t('graph.fraudProximity') }}
              </h3>
              <USwitch
                v-model="showPath"
                size="xs"
                :label="t('graph.showPath')"
              />
            </div>
          </template>
          <p
            class="text-3xl font-semibold tabular-nums"
            :class="proximity.distance !== null && proximity.distance <= 2 ? 'text-error' : ''"
          >
            {{ proximity.distance ?? '∞' }}
          </p>
          <p class="text-xs text-muted">
            {{ t('graph.hopsToFraud') }}
          </p>
          <div class="flex gap-2 mt-2 text-xs">
            <UBadge
              v-for="(n, d) in proximity.fraud_within"
              :key="d"
              color="neutral"
              variant="outline"
              size="xs"
            >
              ≤{{ d }}: {{ n }}
            </UBadge>
          </div>
          <p
            v-if="proximity.path.length"
            class="text-xs mt-2 leading-5"
          >
            <template
              v-for="(n, i) in proximity.path"
              :key="n.id"
            >
              <span v-if="i > 0"> → </span>
              <span :class="n.risk_label === 'fraud' ? 'text-error font-medium' : n.type === 'entity' ? 'text-muted' : ''">{{ n.type === 'entity' ? `${n.kind}:${n.label}` : n.label }}</span>
            </template>
          </p>
          <UButton
            v-if="proximity.nearest_fraud_customer_id"
            size="xs"
            variant="link"
            class="px-0 mt-2"
            :label="t('graph.openNearestFraud')"
            :to="`${base}/customers/${proximity.nearest_fraud_customer_id}`"
          />
        </UCard>
        <UCard v-if="selected">
          <template #header>
            <h3 class="font-medium">
              {{ selected.type === 'customer' ? t('common.customer') : t(`graph.kinds.${selected.kind}`) }}
            </h3>
          </template>
          <p class="fp-mono text-sm break-all">
            {{ selected.label }}
          </p>
          <StatusBadge
            v-if="selected.risk_label"
            :value="selected.risk_label"
            class="mt-1"
          />
          <div
            v-if="selected.type === 'customer'"
            class="flex flex-wrap gap-2 mt-3"
          >
            <UButton
              size="xs"
              :label="t('graph.focus')"
              icon="i-lucide-crosshair"
              @click="customerId = selected.id"
            />
            <UButton
              size="xs"
              variant="outline"
              :label="t('graph.expand')"
              icon="i-lucide-expand"
              @click="expand(selected)"
            />
            <UButton
              size="xs"
              variant="outline"
              :label="t('common.details')"
              :to="`${base}/customers/${selected.id}`"
            />
          </div>
        </UCard>
        <UCard v-if="stats">
          <template #header>
            <h3 class="font-medium">
              {{ t('graph.stats') }}
            </h3>
          </template>
          <KeyValue :items="[{ label: t('graph.customers'), value: fmtCompact(stats.customers) }, { label: t('graph.fraudCustomers'), value: fmtCompact(stats.fraud_customers) }, { label: t('graph.entities'), value: fmtCompact(stats.entities) }, { label: t('graph.links'), value: fmtCompact(stats.links) }, { label: t('graph.similarityLinks'), value: fmtCompact(stats.similarity_links) }]" />
          <p class="text-xs text-muted mt-3 mb-1">
            {{ t('graph.supernodes') }}
          </p>
          <ul class="text-xs space-y-1">
            <li
              v-for="s in stats.supernodes"
              :key="s.display_value"
              class="flex justify-between"
            >
              <span><UBadge
                color="neutral"
                variant="soft"
                size="xs"
              >{{ s.kind }}</UBadge> <span class="fp-mono">{{ s.display_value }}</span></span>
              <span class="tabular-nums">{{ fmtNumber(s.degree) }}</span>
            </li>
          </ul>
        </UCard>
      </div>
    </div>

    <UCard class="mt-4">
      <template #header>
        <div class="flex items-center justify-between">
          <h3 class="font-medium">
            {{ t('graph.components') }}
          </h3>
          <USwitch
            v-model="onlyFraud"
            size="sm"
            :label="t('graph.onlyWithFraud')"
          />
        </div>
      </template>
      <UTable
        :data="components"
        :columns="[{ accessorKey: 'component_id', header: 'ID' }, { accessorKey: 'size', header: t('ml.size') }, { accessorKey: 'fraud_count', header: t('status.fraud') }, { accessorKey: 'fraud_rate', header: t('ml.fraudRate') }, { accessorKey: 'sample_customer_ids', header: t('ml.sampleCustomers') }]"
      >
        <template #fraud_rate-cell="{ row }">
          {{ fmtPct(row.original.fraud_rate) }}
        </template>
        <template #sample_customer_ids-cell="{ row }">
          <UButton
            v-for="c in row.original.sample_customer_ids.slice(0, 4)"
            :key="c"
            size="xs"
            variant="link"
            class="fp-mono px-1"
            :label="shortId(c)"
            @click="customerId = c"
          />
        </template>
      </UTable>
    </UCard>
  </PagePanel>
</template>
