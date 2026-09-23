<script setup lang="ts">
// Supervised model evaluation charts: ROC, PR, loss curves, calibration, confusion matrix, feature importance.
import type { EChartsOption } from 'echarts'
import type { MlModel } from '#shared/types/api'

const props = defineProps<{ model: MlModel }>()
const { t } = useI18n()
const m = computed(() => props.model.metrics)

const curve = (points: { x: number, y: number }[] | undefined, xName: string, yName: string, diagonal: boolean): EChartsOption => ({
  tooltip: { trigger: 'axis' },
  grid: { left: 48, right: 16, top: 16, bottom: 40 },
  xAxis: { type: 'value', min: 0, max: 1, name: xName, nameLocation: 'middle', nameGap: 26 },
  yAxis: { type: 'value', min: 0, max: 1, name: yName },
  series: [
    { type: 'line', showSymbol: false, areaStyle: { opacity: 0.12 }, data: (points ?? []).map(p => [p.x, p.y]), itemStyle: { color: '#6366f1' } },
    ...(diagonal ? [{ type: 'line' as const, showSymbol: false, lineStyle: { type: 'dashed' as const, color: '#94a3b8' }, data: [[0, 0], [1, 1]] }] : []),
  ],
})
const roc = computed(() => curve(m.value.roc_curve, 'FPR', 'TPR', true))
/** Explicit PR curve when provided, otherwise derived from the per-threshold table (recall → precision). */
const prPoints = computed(() => m.value.pr_curve
  ?? [...(m.value.thresholds ?? [])].sort((a, b) => a.recall - b.recall).map(x => ({ x: x.recall, y: x.precision })))
const pr = computed(() => curve(prPoints.value, 'Recall', 'Precision', false))
const history = computed<EChartsOption>(() => {
  const h = props.model.training_history ?? {}
  const n = Math.max(h.train_loss?.length ?? 0, h.val_loss?.length ?? 0, h.val_pr_auc?.length ?? 0)
  return {
    tooltip: { trigger: 'axis' },
    legend: { top: 0 },
    grid: { left: 48, right: 48, top: 28, bottom: 24 },
    xAxis: { type: 'category', data: Array.from({ length: n }, (_, i) => i + 1) },
    yAxis: [{ type: 'value', name: 'loss' }, { type: 'value', name: 'PR-AUC', min: 0, max: 1 }],
    series: [
      { name: 'train_loss', type: 'line', data: h.train_loss ?? [], markLine: h.best_epoch ? { symbol: 'none', data: [{ xAxis: h.best_epoch - 1 }], label: { formatter: 'best' } } : undefined },
      { name: 'val_loss', type: 'line', data: h.val_loss ?? [] },
      { name: 'val_pr_auc', type: 'line', yAxisIndex: 1, data: h.val_pr_auc ?? [] },
    ],
  }
})
const hasHistory = computed(() => (props.model.training_history?.train_loss?.length ?? 0) > 0)
const calibration = computed<EChartsOption>(() => ({
  tooltip: { trigger: 'axis' },
  grid: { left: 48, right: 16, top: 16, bottom: 40 },
  xAxis: { type: 'value', min: 0, max: 1, name: t('ml.predicted'), nameLocation: 'middle', nameGap: 26 },
  yAxis: { type: 'value', min: 0, max: 1, name: t('ml.observed') },
  series: [
    { type: 'line', data: (m.value.calibration ?? []).filter(c => c.count > 0 && c.mean_predicted !== null && c.observed_rate !== null).map(c => [c.mean_predicted, c.observed_rate]), itemStyle: { color: '#10b981' } },
    { type: 'line', showSymbol: false, lineStyle: { type: 'dashed', color: '#94a3b8' }, data: [[0, 0], [1, 1]] },
  ],
}))
const importance = computed<EChartsOption>(() => {
  const fi = [...(m.value.feature_importance ?? [])].sort((a, b) => a.importance - b.importance)
  return {
    tooltip: {},
    grid: { left: 180, right: 24, top: 8, bottom: 24 },
    xAxis: { type: 'value' },
    yAxis: { type: 'category', data: fi.map(f => f.feature), axisLabel: { fontFamily: 'monospace', fontSize: 11 } },
    series: [{ type: 'bar', data: fi.map(f => f.importance), itemStyle: { color: '#6366f1' } }],
  }
})
const cm = computed(() => m.value.confusion_matrix)
</script>

<template>
  <div class="space-y-4">
    <div class="grid grid-cols-2 lg:grid-cols-5 gap-3">
      <StatCard
        label="ROC-AUC"
        :value="fmtNumber(m.roc_auc, 3)"
      />
      <StatCard
        label="PR-AUC"
        :value="fmtNumber(m.pr_auc, 3)"
      />
      <StatCard
        :label="t('ml.trainedRows')"
        :value="fmtCompact(model.trained_rows)"
      />
      <StatCard
        :label="t('ml.fraudLabels')"
        :value="fmtNumber(m.class_balance?.fraud)"
        :hint="`${fmtNumber(m.class_balance?.legit)} legit`"
      />
      <StatCard
        :label="t('ml.features')"
        :value="model.feature_names.length"
      />
    </div>
    <div class="grid lg:grid-cols-2 xl:grid-cols-3 gap-4">
      <UCard v-if="m.roc_curve?.length">
        <template #header>
          <h3 class="font-medium text-sm">
            ROC
          </h3>
        </template>
        <EChart
          :option="roc"
          height="240px"
        />
      </UCard>
      <UCard>
        <template #header>
          <h3 class="font-medium text-sm">
            Precision–Recall
          </h3>
        </template>
        <EChart
          :option="pr"
          height="240px"
        />
      </UCard>
      <UCard v-if="hasHistory">
        <template #header>
          <h3 class="font-medium text-sm">
            {{ t('ml.trainingHistory') }}
          </h3>
        </template>
        <EChart
          :option="history"
          height="240px"
        />
      </UCard>
      <UCard>
        <template #header>
          <h3 class="font-medium text-sm">
            {{ t('ml.confusionMatrix') }} <span
              v-if="cm?.threshold !== undefined"
              class="text-muted"
            >@ {{ cm.threshold }}</span>
          </h3>
        </template>
        <div
          v-if="cm"
          class="grid grid-cols-[auto_1fr_1fr] gap-1 text-center text-sm"
        >
          <span />
          <span class="text-xs text-muted">{{ t('ml.predFraud') }}</span>
          <span class="text-xs text-muted">{{ t('ml.predLegit') }}</span>
          <span class="text-xs text-muted self-center">{{ t('ml.actualFraud') }}</span>
          <div class="rounded bg-emerald-500/20 p-3 tabular-nums">
            TP {{ fmtNumber(cm.tp) }}
          </div>
          <div class="rounded bg-rose-500/15 p-3 tabular-nums">
            FN {{ fmtNumber(cm.fn) }}
          </div>
          <span class="text-xs text-muted self-center">{{ t('ml.actualLegit') }}</span>
          <div class="rounded bg-amber-500/15 p-3 tabular-nums">
            FP {{ fmtNumber(cm.fp) }}
          </div>
          <div class="rounded bg-elevated p-3 tabular-nums">
            TN {{ fmtNumber(cm.tn) }}
          </div>
        </div>
        <UTable
          v-if="m.thresholds?.length"
          :data="m.thresholds"
          :columns="[{ accessorKey: 'threshold', header: 'θ' }, { accessorKey: 'precision', header: 'P' }, { accessorKey: 'recall', header: 'R' }, { accessorKey: 'f1', header: 'F1' }]"
          class="mt-3 text-xs"
        >
          <template #precision-cell="{ row }">
            {{ row.original.precision.toFixed(3) }}
          </template>
          <template #recall-cell="{ row }">
            {{ row.original.recall.toFixed(3) }}
          </template>
          <template #f1-cell="{ row }">
            {{ row.original.f1.toFixed(3) }}
          </template>
        </UTable>
      </UCard>
      <UCard>
        <template #header>
          <h3 class="font-medium text-sm">
            {{ t('ml.calibration') }}
          </h3>
        </template>
        <EChart
          :option="calibration"
          height="240px"
        />
      </UCard>
      <UCard>
        <template #header>
          <h3 class="font-medium text-sm">
            {{ t('ml.featureImportance') }}
          </h3>
        </template>
        <EChart
          :option="importance"
          height="240px"
        />
      </UCard>
    </div>
  </div>
</template>
