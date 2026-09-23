<script setup lang="ts">
import type { EChartsOption } from 'echarts'
import type { AnomalyRow, Cluster, GraphCommunity, MlAlgorithm, MlModel, ModelItems, Page, ProjectionPoint } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const toast = useToast()
const { pid, base, apiBase, project, can } = useProject()
useHead({ title: () => t('nav.mlUnsupervised') })

const { data: algorithms } = await useAsyncData<MlAlgorithm[]>('ml-algorithms', () => api.get<MlAlgorithm[]>('/ml/algorithms'), { default: () => [] as MlAlgorithm[] })
const { data: models, refresh: refreshModels } = await useAsyncData<MlModel[]>(`models-uns-${pid.value}`, () => api.get<Page<MlModel>>(`${apiBase.value}/ml/models`, { query: { kind: 'unsupervised', page_size: 50 } }).then(p => p.items), { default: () => [] as MlModel[] })
const modelId = ref<string | undefined>((route.query.model_id as string) ?? models.value.find(m => m.status === 'active')?.id)
const q = computed(() => ({ model_id: modelId.value }))
const { data: clusters, refresh: refreshClusters } = await useAsyncData<Cluster[]>(`clusters-${pid.value}`, () => (modelId.value ? api.get<ModelItems<Cluster>>(`${apiBase.value}/ml/unsupervised/clusters`, { query: q.value, silent: true }).then(r => r.items).catch(() => []) : Promise.resolve([])), { default: () => [] as Cluster[], watch: [modelId] })
const { data: points } = await useAsyncData<ProjectionPoint[]>(`projection-${pid.value}`, () => (modelId.value ? api.get<ModelItems<ProjectionPoint>>(`${apiBase.value}/ml/unsupervised/projection`, { query: { ...q.value, limit: 2000 }, silent: true }).then(r => r.items).catch(() => []) : Promise.resolve([])), { default: () => [] as ProjectionPoint[], watch: [modelId] })
const minScore = ref(0.6)
const { data: anomalies } = await useAsyncData<AnomalyRow[]>(`anomalies-${pid.value}`, () => (modelId.value ? api.get<ModelItems<AnomalyRow>>(`${apiBase.value}/ml/unsupervised/anomalies`, { query: { ...q.value, limit: 50, min_score: minScore.value }, silent: true }).then(r => r.items).catch(() => []) : Promise.resolve([])), { default: () => [] as AnomalyRow[], watch: [modelId, minScore] })
const { data: communities, refresh: refreshCommunities } = await useAsyncData<GraphCommunity[]>(`communities-${pid.value}`, () => api.get<ModelItems<GraphCommunity>>(`${apiBase.value}/ml/graph-communities`, { query: { min_size: 3 }, silent: true }).then(r => r.items).catch(() => []), { default: () => [] as GraphCommunity[] })

const colorBy = ref<'cluster' | 'anomaly'>('cluster')
const PALETTE = ['#6366f1', '#10b981', '#f59e0b', '#ec4899', '#06b6d4', '#8b5cf6', '#84cc16', '#f97316', '#14b8a6', '#e11d48']
const scatter = computed<EChartsOption>(() => {
  const pts = points.value
  if (colorBy.value === 'anomaly') {
    return {
      tooltip: { formatter: (p: unknown) => { const d = (p as { data: number[] }).data; return `${t('ml.anomalyScore')}: ${d[2]?.toFixed(3)}` } },
      grid: { left: 32, right: 80, top: 16, bottom: 24 },
      xAxis: { type: 'value', scale: true }, yAxis: { type: 'value', scale: true },
      visualMap: { min: 0, max: 1, dimension: 2, right: 0, top: 'middle', calculable: true, inRange: { color: ['#10b981', '#f59e0b', '#f43f5e'] } },
      series: [{ type: 'scatter', symbolSize: 5, data: pts.map(p => [p.x, p.y, p.anomaly_score, p.event_id]) }],
    }
  }
  const ids = [...new Set(pts.map(p => p.cluster_id ?? -1))].sort((a, b) => a - b)
  return {
    tooltip: { formatter: (p: unknown) => { const x = p as { seriesName: string, data: unknown[] }; return `${x.seriesName}<br>${t('ml.anomalyScore')}: ${(x.data[2] as number).toFixed(3)}${x.data[4] ? `<br>${t('status.fraud')}` : ''}` } },
    legend: { type: 'scroll', top: 0 },
    grid: { left: 32, right: 16, top: 32, bottom: 24 },
    xAxis: { type: 'value', scale: true }, yAxis: { type: 'value', scale: true },
    series: ids.map((cid, i) => ({
      name: cid === -1 ? t('ml.noise') : `${t('ml.cluster')} ${cid}`,
      type: 'scatter' as const,
      symbolSize: (d: unknown[]) => (d[4] ? 9 : 5),
      itemStyle: { color: cid === -1 ? '#94a3b8' : PALETTE[i % PALETTE.length], opacity: 0.8 },
      data: pts.filter(p => (p.cluster_id ?? -1) === cid).map(p => [p.x, p.y, p.anomaly_score, p.event_id, p.label === 'fraud' ? 1 : 0]),
    })),
  }
})
function onPointClick(p: unknown) {
  const d = (p as { data?: unknown[] }).data
  const eventId = d?.[3]
  if (typeof eventId === 'string') navigateTo(`${base.value}/events/${eventId}`)
}

const editing = ref<Cluster | null>(null)
const labelForm = reactive({ label: '', notes: '' })
function editCluster(c: Cluster) {
  editing.value = c
  labelForm.label = c.label ?? ''
  labelForm.notes = c.notes ?? ''
}
async function saveLabel() {
  if (!editing.value) return
  await api.patch(`${apiBase.value}/ml/unsupervised/clusters/${editing.value.model_id}/${editing.value.cluster_id}`, labelForm)
  editing.value = null
  await refreshClusters()
}
async function recompute() {
  await api.post(`${apiBase.value}/ml/graph-communities/recompute`)
  toast.add({ title: t('ml.communitiesQueued'), color: 'success' })
  setTimeout(() => refreshCommunities(), 3000)
}
const tab = ref('clusters')
</script>

<template>
  <PagePanel
    :title="t('nav.mlUnsupervised')"
    :description="t('ml.unsupervisedDescription')"
  >
    <template #actions>
      <ClientOnly>
        <USelect
          v-model="modelId"
          :items="models.map(m => ({ label: `v${m.version} · ${m.algorithms.anomaly?.name} + ${m.algorithms.clustering?.name}`, value: m.id, description: t(`status.${m.status}`) }))"
          class="w-80"
          :placeholder="t('ml.selectModel')"
        />
        <template #fallback>
          <USkeleton class="h-8 w-40" />
        </template>
      </ClientOnly>
      <UButton
        v-if="modelId"
        :to="`${base}/ml/models/${modelId}`"
        icon="i-lucide-info"
        color="neutral"
        variant="ghost"
        :aria-label="t('actions.details')"
      />
      <TrainModal
        v-if="can('analyst')"
        kind="unsupervised"
        :project="project"
        :algorithms="algorithms"
        @started="() => refreshModels()"
      />
    </template>
    <UEmpty
      v-if="!models.length"
      icon="i-lucide-scatter-chart"
      :title="t('ml.noModels')"
      :description="t('ml.noUnsupervisedHelp')"
    />
    <template v-else>
      <UCard class="mb-4">
        <template #header>
          <div class="flex items-center gap-2">
            <h3 class="font-medium">
              {{ t('ml.projection') }}
            </h3>
            <span class="text-xs text-muted">{{ t('ml.projectionHelp') }}</span>
            <UTabs
              v-model="colorBy"
              :items="[{ label: t('ml.byCluster'), value: 'cluster' }, { label: t('ml.byAnomaly'), value: 'anomaly' }]"
              size="xs"
              :content="false"
              class="ml-auto"
            />
          </div>
        </template>
        <EChart
          :option="scatter"
          height="420px"
          @click="onPointClick"
        />
      </UCard>

      <UTabs
        v-model="tab"
        :items="[{ label: t('ml.clusters'), value: 'clusters' }, { label: t('ml.topAnomalies'), value: 'anomalies' }, { label: t('ml.graphCommunities'), value: 'communities' }]"
        variant="link"
        :content="false"
        class="mb-3"
      />

      <UTable
        v-if="tab === 'clusters'"
        :data="clusters"
        :columns="[{ accessorKey: 'cluster_id', header: '#' }, { accessorKey: 'label', header: t('common.label') }, { accessorKey: 'size', header: t('ml.size') }, { accessorKey: 'fraud_rate', header: t('ml.fraudRate') }, { accessorKey: 'top_features', header: t('ml.distinguishing') }, { accessorKey: 'profile', header: t('ml.profile') }, { id: 'actions', header: '' }]"
      >
        <template #cluster_id-cell="{ row }">
          <UBadge
            :color="row.original.cluster_id === -1 ? 'neutral' : 'primary'"
            variant="soft"
          >
            {{ row.original.cluster_id === -1 ? t('ml.noise') : row.original.cluster_id }}
          </UBadge>
        </template>
        <template #label-cell="{ row }">
          {{ row.original.label ?? '—' }}
        </template>
        <template #size-cell="{ row }">
          {{ fmtNumber(row.original.size) }}
        </template>
        <template #fraud_rate-cell="{ row }">
          <span :class="(row.original.fraud_rate ?? 0) > 0.2 ? 'text-error font-medium' : ''">{{ fmtPct(row.original.fraud_rate) }}</span>
          <span class="text-xs text-muted"> ({{ row.original.labeled_count }})</span>
        </template>
        <template #top_features-cell="{ row }">
          <div class="flex flex-wrap gap-1">
            <UBadge
              v-for="f in row.original.top_features"
              :key="f.feature"
              color="neutral"
              variant="outline"
              size="xs"
              class="fp-mono"
            >
              {{ f.feature }} {{ f.smd > 0 ? '↑' : '↓' }}{{ Math.abs(f.smd).toFixed(1) }}
            </UBadge>
          </div>
        </template>
        <template #profile-cell="{ row }">
          <span class="fp-mono text-xs">{{ row.original.top_features.slice(0, 3).map(f => `${f.feature}=${fmtNumber(f.cluster_mean ?? Number(row.original.profile[f.feature]), 2)}`).join(' · ') }}</span>
        </template>
        <template #actions-cell="{ row }">
          <UButton
            v-if="can('analyst')"
            size="xs"
            variant="ghost"
            icon="i-lucide-tag"
            :aria-label="t('ml.labelCluster')"
            @click="editCluster(row.original)"
          />
        </template>
      </UTable>

      <div v-else-if="tab === 'anomalies'">
        <div class="flex items-center gap-2 mb-2 text-sm">
          <span>{{ t('ml.minScore') }}</span>
          <USlider
            v-model="minScore"
            :min="0"
            :max="1"
            :step="0.05"
            class="w-48"
          />
          <span class="tabular-nums">{{ minScore.toFixed(2) }}</span>
        </div>
        <UTable
          :data="anomalies"
          :columns="[{ accessorKey: 'external_id', header: t('common.event') }, { accessorKey: 'anomaly_score', header: t('ml.anomalyScore') }, { accessorKey: 'cluster_id', header: t('ml.cluster') }, { accessorKey: 'amount', header: t('common.amount') }, { accessorKey: 'occurred_at', header: t('common.occurred') }]"
          class="cursor-pointer"
          @select="(_, row) => navigateTo(`${base}/events/${row.original.event_id}`)"
        >
          <template #external_id-cell="{ row }">
            <span class="fp-mono text-xs">{{ row.original.external_id ?? shortId(row.original.event_id) }}</span>
          </template>
          <template #anomaly_score-cell="{ row }">
            <ScoreBar
              :score="row.original.anomaly_score * 100"
              compact
            />
          </template>
          <template #amount-cell="{ row }">
            {{ fmtMoney(row.original.amount) }}
          </template>
          <template #occurred_at-cell="{ row }">
            <span class="text-xs">{{ fmtDate(row.original.occurred_at) }}</span>
          </template>
        </UTable>
      </div>

      <div v-else>
        <div class="flex justify-between items-center mb-2">
          <p class="text-sm text-muted">
            {{ t('ml.communitiesHelp') }}
          </p>
          <UButton
            v-if="can('analyst')"
            size="sm"
            icon="i-lucide-refresh-cw"
            :label="t('ml.recompute')"
            variant="outline"
            @click="recompute"
          />
        </div>
        <UTable
          :data="communities"
          :columns="[{ accessorKey: 'community_id', header: '#' }, { accessorKey: 'size', header: t('ml.size') }, { accessorKey: 'fraud_count', header: t('status.fraud') }, { accessorKey: 'fraud_rate', header: t('ml.fraudRate') }, { accessorKey: 'customer_ids', header: t('ml.sampleCustomers') }]"
        >
          <template #fraud_rate-cell="{ row }">
            <span :class="row.original.fraud_rate > 0.2 ? 'text-error font-medium' : ''">{{ fmtPct(row.original.fraud_rate) }}</span>
          </template>
          <template #customer_ids-cell="{ row }">
            <NuxtLink
              v-for="c in row.original.customer_ids.slice(0, 4)"
              :key="c"
              :to="`${base}/graph?customer=${c}`"
              class="fp-mono text-xs text-primary mr-2"
            >
              {{ shortId(c) }}
            </NuxtLink>
          </template>
        </UTable>
      </div>
    </template>

    <UModal
      :open="!!editing"
      :title="t('ml.labelCluster')"
      @update:open="(v: boolean) => { if (!v) editing = null }"
    >
      <template #body>
        <div class="space-y-3">
          <UFormField :label="t('common.label')">
            <UInput
              v-model="labelForm.label"
              class="w-full"
              placeholder="Promo farm"
            />
          </UFormField>
          <UFormField :label="t('common.notes')">
            <UTextarea
              v-model="labelForm.notes"
              :rows="3"
              class="w-full"
            />
          </UFormField>
        </div>
      </template>
      <template #footer>
        <UButton
          :label="t('actions.save')"
          @click="saveLabel"
        />
      </template>
    </UModal>
  </PagePanel>
</template>
