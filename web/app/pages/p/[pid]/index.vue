<script setup lang="ts">
import type { EChartsOption } from 'echarts'
import type { AnalyticsOverview, DriftResponse, DriftRow, ProjectSettings } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const { pid, base, project, apiBase } = useProject()
useHead({ title: () => `${t('nav.dashboard')} · ${project.value?.name ?? ''}` })

const range = ref<'7' | '14' | '30'>('14')
const query = computed(() => ({ from: new Date(Date.now() - Number(range.value) * 86_400_000).toISOString().slice(0, 10) }))
const { data: overview, status } = await useAsyncData(`overview-${pid.value}`, () => api.get<AnalyticsOverview>(`${apiBase.value}/analytics/overview`, { query: query.value }), { watch: [range] })
const { data: drift } = await useAsyncData<DriftRow[]>(`drift-${pid.value}`, () => api.get<DriftResponse>(`${apiBase.value}/analytics/drift`, { silent: true }).then(r => r.items).catch(() => []), { default: () => [] })
const { data: settings } = await useAsyncData(`settings-${pid.value}`, () => api.get<ProjectSettings>(`${apiBase.value}/settings`, { silent: true }).catch(() => null))

const decisionRate = (n: number) => (overview.value?.totals.events ? n / overview.value.totals.events : 0)

const timeseries = computed<EChartsOption>(() => ({
  tooltip: { trigger: 'axis' },
  legend: { top: 0 },
  grid: { left: 48, right: 48, top: 32, bottom: 24 },
  xAxis: { type: 'category', data: overview.value?.daily.map(d => d.date.slice(5)) ?? [] },
  yAxis: [{ type: 'value', name: t('dashboard.events') }, { type: 'value', name: t('dashboard.avgScore'), max: 100 }],
  series: [
    { name: t('dashboard.events'), type: 'bar', data: overview.value?.daily.map(d => d.events) ?? [], itemStyle: { color: '#94a3b8' } },
    { name: t('decision.review'), type: 'line', data: overview.value?.daily.map(d => d.review) ?? [], itemStyle: { color: '#f59e0b' } },
    { name: t('decision.decline'), type: 'line', data: overview.value?.daily.map(d => d.decline) ?? [], itemStyle: { color: '#f43f5e' } },
    { name: t('dashboard.avgScore'), type: 'line', yAxisIndex: 1, smooth: true, data: overview.value?.daily.map(d => d.avg_score) ?? [], itemStyle: { color: '#6366f1' } },
  ],
}))

const histogram = computed<EChartsOption>(() => ({
  tooltip: {},
  grid: { left: 48, right: 16, top: 16, bottom: 24 },
  xAxis: { type: 'category', data: overview.value?.score_histogram.map(b => b.bucket) ?? [] },
  yAxis: { type: 'log', minorSplitLine: { show: false } },
  series: [{
    type: 'bar',
    data: (overview.value?.score_histogram ?? []).map((b, i) => ({
      value: Math.max(1, b.count),
      itemStyle: { color: i * 10 >= (settings.value?.decision_thresholds.decline ?? 80) ? '#f43f5e' : i * 10 >= (settings.value?.decision_thresholds.review ?? 50) ? '#f59e0b' : '#10b981' },
    })),
  }],
}))

const engines = computed<EChartsOption>(() => ({
  tooltip: {},
  grid: { left: 90, right: 24, top: 8, bottom: 24 },
  xAxis: { type: 'value', max: 100 },
  yAxis: { type: 'category', data: ['rules', 'supervised', 'unsupervised', 'graph'].map(e => t(`engines.${e}`)) },
  series: [{ type: 'bar', data: (['rules', 'supervised', 'unsupervised', 'graph'] as const).map(e => overview.value?.engine_avg[e] ?? 0), itemStyle: { color: '#6366f1' }, label: { show: true, position: 'right' } }],
}))

const typologies = computed<EChartsOption>(() => ({
  tooltip: { trigger: 'item' },
  legend: { orient: 'vertical', right: 0, top: 'middle' },
  series: [{ type: 'pie', radius: ['45%', '70%'], center: ['35%', '50%'], data: overview.value?.by_label_fraud_type.map(f => ({ name: t(`typologies.${f.fraud_type}`), value: f.count })) ?? [] }],
}))
</script>

<template>
  <PagePanel
    :title="project?.name ?? t('nav.dashboard')"
    :description="project?.business_context ?? undefined"
  >
    <template #actions>
      <USelect
        v-model="range"
        :items="[{ label: t('dashboard.lastDays', { n: 7 }), value: '7' }, { label: t('dashboard.lastDays', { n: 14 }), value: '14' }, { label: t('dashboard.lastDays', { n: 30 }), value: '30' }]"
        size="sm"
        class="w-36"
      />
    </template>

    <div
      v-if="status === 'pending' && !overview"
      class="grid grid-cols-2 lg:grid-cols-5 gap-4"
    >
      <USkeleton
        v-for="i in 5"
        :key="i"
        class="h-20"
      />
    </div>
    <template v-else-if="overview">
      <div class="grid grid-cols-2 lg:grid-cols-5 gap-4">
        <StatCard
          :label="t('dashboard.events')"
          :value="fmtCompact(overview.totals.events)"
          icon="i-lucide-activity"
        />
        <StatCard
          :label="t('decision.approve')"
          :value="fmtPct(decisionRate(overview.totals.approve))"
          :hint="fmtCompact(overview.totals.approve)"
          icon="i-lucide-circle-check"
          color="!bg-emerald-500/10 !text-emerald-600"
        />
        <StatCard
          :label="t('decision.review')"
          :value="fmtPct(decisionRate(overview.totals.review))"
          :hint="fmtCompact(overview.totals.review)"
          icon="i-lucide-eye"
          color="!bg-amber-500/10 !text-amber-600"
        />
        <StatCard
          :label="t('decision.decline')"
          :value="fmtPct(decisionRate(overview.totals.decline))"
          :hint="fmtCompact(overview.totals.decline)"
          icon="i-lucide-ban"
          color="!bg-rose-500/10 !text-rose-600"
        />
        <StatCard
          :label="t('dashboard.openCases')"
          :value="fmtNumber(overview.open_cases)"
          :hint="`${t('dashboard.degradedRate')}: ${fmtPct(overview.degraded_rate, 2)}`"
          icon="i-lucide-briefcase"
        />
      </div>

      <div class="grid xl:grid-cols-3 gap-4 mt-4">
        <UCard class="xl:col-span-2">
          <template #header>
            <h3 class="font-medium">
              {{ t('dashboard.decisionsOverTime') }}
            </h3>
          </template>
          <EChart
            :option="timeseries"
            height="300px"
          />
        </UCard>
        <UCard>
          <template #header>
            <h3 class="font-medium">
              {{ t('dashboard.fraudByTypology') }}
            </h3>
          </template>
          <EChart
            :option="typologies"
            height="300px"
          />
        </UCard>
        <UCard>
          <template #header>
            <h3 class="font-medium">
              {{ t('dashboard.scoreDistribution') }}
            </h3>
          </template>
          <EChart
            :option="histogram"
            height="240px"
          />
        </UCard>
        <UCard>
          <template #header>
            <h3 class="font-medium">
              {{ t('dashboard.engineAverages') }}
            </h3>
          </template>
          <EChart
            :option="engines"
            height="240px"
          />
        </UCard>
        <UCard>
          <template #header>
            <div class="flex items-center justify-between">
              <h3 class="font-medium">
                {{ t('dashboard.featureDrift') }}
              </h3>
              <UTooltip :text="t('dashboard.psiHelp')">
                <UIcon
                  name="i-lucide-info"
                  class="text-muted"
                />
              </UTooltip>
            </div>
          </template>
          <UTable
            :data="drift"
            :columns="[{ accessorKey: 'feature', header: t('common.feature') }, { accessorKey: 'psi', header: 'PSI' }, { accessorKey: 'status', header: t('common.status') }]"
            class="text-sm"
          >
            <template #feature-cell="{ row }">
              <span class="fp-mono text-xs">{{ row.original.feature }}</span>
            </template>
            <template #psi-cell="{ row }">
              <span class="tabular-nums">{{ row.original.psi.toFixed(3) }}</span>
            </template>
            <template #status-cell="{ row }">
              <StatusBadge
                :value="row.original.status"
                size="xs"
              />
            </template>
          </UTable>
        </UCard>
      </div>

      <div class="flex flex-wrap gap-2 mt-4">
        <UButton
          :to="`${base}/events?decision=decline`"
          icon="i-lucide-ban"
          color="neutral"
          variant="outline"
          :label="t('dashboard.viewDeclines')"
        />
        <UButton
          :to="`${base}/cases?status=open`"
          icon="i-lucide-briefcase"
          color="neutral"
          variant="outline"
          :label="t('dashboard.viewOpenCases')"
        />
        <UButton
          :to="`${base}/llm/reports`"
          icon="i-lucide-sparkles"
          color="neutral"
          variant="outline"
          :label="t('dashboard.askLlm')"
        />
      </div>
    </template>
  </PagePanel>
</template>
