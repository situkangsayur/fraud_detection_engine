<script setup lang="ts">
// Backtest a rule/ruleset over history (precision/recall against labels, hits per day).
import type { EChartsOption } from 'echarts'
import type { BacktestResult } from '#shared/types/api'

const props = defineProps<{ endpoint: string, body?: () => Record<string, unknown> }>()
const { t } = useI18n()
const api = useApi()
const { base } = useProject()

const today = new Date().toISOString().slice(0, 10)
const form = reactive({ from: new Date(Date.now() - 30 * 86_400_000).toISOString().slice(0, 10), to: today, limit: 50_000 })
const result = ref<BacktestResult | null>(null)
const busy = ref(false)
async function run() {
  busy.value = true
  try {
    result.value = await api.post<BacktestResult>(props.endpoint, { ...form, from: dayStartIso(form.from), to: dayEndIso(form.to), ...(props.body?.() ?? {}) })
  }
  catch { /* toast */ }
  finally { busy.value = false }
}
const chart = computed<EChartsOption>(() => ({
  tooltip: { trigger: 'axis' },
  grid: { left: 48, right: 48, top: 24, bottom: 24 },
  xAxis: { type: 'category', data: result.value?.by_day.map(d => d.date.slice(5)) ?? [] },
  yAxis: [{ type: 'value', name: t('rules.backtest.matched') }, { type: 'value', name: t('rules.backtest.hitRate'), axisLabel: { formatter: (v: number) => `${(v * 100).toFixed(1)}%` } }],
  series: [
    { name: t('rules.backtest.matched'), type: 'bar', data: result.value?.by_day.map(d => d.matched) ?? [], itemStyle: { color: '#6366f1' } },
    { name: t('rules.backtest.hitRate'), type: 'line', yAxisIndex: 1, data: result.value?.by_day.map(d => (d.evaluated ? d.matched / d.evaluated : 0)) ?? [], itemStyle: { color: '#f59e0b' } },
  ],
}))
</script>

<template>
  <div class="space-y-4">
    <div class="flex flex-wrap items-end gap-3">
      <UFormField :label="t('common.from')">
        <UInput
          v-model="form.from"
          type="date"
        />
      </UFormField>
      <UFormField :label="t('common.to')">
        <UInput
          v-model="form.to"
          type="date"
        />
      </UFormField>
      <UFormField :label="t('rules.backtest.limit')">
        <UInputNumber
          v-model="form.limit"
          :min="100"
          :max="50000"
          :step="1000"
          class="w-36"
        />
      </UFormField>
      <UButton
        :label="t('actions.backtest')"
        icon="i-lucide-history"
        :loading="busy"
        @click="run"
      />
    </div>
    <template v-if="result">
      <div class="grid grid-cols-2 lg:grid-cols-6 gap-3">
        <StatCard
          :label="t('rules.backtest.evaluated')"
          :value="fmtCompact(result.evaluated)"
        />
        <StatCard
          :label="t('rules.backtest.matched')"
          :value="fmtNumber(result.matched)"
        />
        <StatCard
          :label="t('rules.backtest.hitRate')"
          :value="fmtPct(result.hit_rate, 2)"
        />
        <StatCard
          :label="t('status.trapped')"
          :value="fmtNumber(result.trapped)"
        />
        <StatCard
          :label="t('rules.backtest.precision')"
          :value="fmtPct(result.precision)"
          :hint="`${result.labeled_fraud_matched} fraud / ${result.labeled_legit_matched} legit`"
        />
        <StatCard
          :label="t('rules.backtest.recall')"
          :value="fmtPct(result.recall)"
        />
      </div>
      <UCard>
        <EChart
          :option="chart"
          height="260px"
        />
      </UCard>
      <div
        v-if="result.decision_distribution"
        class="flex gap-2 text-sm"
      >
        <span class="text-muted">{{ t('rules.backtest.decisionDistribution') }}:</span>
        <UBadge
          v-for="(n, d) in result.decision_distribution"
          :key="d"
          :color="statusColor(d)"
          variant="soft"
        >
          {{ t(`decision.${d}`) }} {{ fmtCompact(n) }}
        </UBadge>
      </div>
      <div
        v-if="result.sample_matches.length"
        class="text-sm"
      >
        <span class="text-muted">{{ t('rules.backtest.samples') }}:</span>
        <NuxtLink
          v-for="id in result.sample_matches"
          :key="id"
          :to="`${base}/events/${id}`"
          class="fp-mono text-xs text-primary ml-2"
        >
          {{ shortId(id) }}
        </NuxtLink>
      </div>
    </template>
  </div>
</template>
