<script setup lang="ts">
import type { MlAlgorithm, MlModel, Page } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const { pid, base, apiBase, project } = useProject()
useHead({ title: () => t('nav.mlSupervised') })
const { data: algorithms } = await useAsyncData<MlAlgorithm[]>('ml-algorithms', () => api.get<MlAlgorithm[]>('/ml/algorithms'), { default: () => [] as MlAlgorithm[] })
const { data, refresh } = await useAsyncData(`models-sup-${pid.value}`, () => api.get<Page<MlModel>>(`${apiBase.value}/ml/models`, { query: { kind: 'supervised', page_size: 100 } }))
const active = computed(() => data.value?.items.find(m => m.status === 'active'))

// Poll while something is training.
let timer: ReturnType<typeof setInterval> | undefined
watch(() => data.value?.items.some(m => m.status === 'training'), (training) => {
  clearInterval(timer)
  if (training && import.meta.client) timer = setInterval(async () => {
    for (const m of data.value?.items.filter(x => x.status === 'training') ?? []) await api.get(`${apiBase.value}/ml/models/${m.id}`, { silent: true }).catch(() => null)
    await refresh()
  }, 3000)
}, { immediate: true })
onBeforeUnmount(() => clearInterval(timer))
</script>

<template>
  <PagePanel
    :title="t('nav.mlSupervised')"
    :description="t('ml.supervisedDescription')"
  >
    <template #actions>
      <TrainModal
        kind="supervised"
        :project="project"
        :algorithms="algorithms"
        @started="() => refresh()"
      />
    </template>
    <UCard
      v-if="active"
      class="mb-4"
    >
      <div class="flex flex-wrap items-center gap-4">
        <UIcon
          name="i-lucide-badge-check"
          class="size-6 text-success"
        />
        <div>
          <p class="text-xs text-muted uppercase">
            {{ t('ml.activeModel') }}
          </p>
          <p class="font-medium">
            v{{ active.version }} · {{ active.algorithms.supervised?.name }}
          </p>
        </div>
        <StatCard
          label="PR-AUC"
          :value="fmtNumber(active.metrics.pr_auc, 3)"
        />
        <StatCard
          label="ROC-AUC"
          :value="fmtNumber(active.metrics.roc_auc, 3)"
        />
        <UButton
          :to="`${base}/ml/models/${active.id}`"
          :label="t('ml.viewMetrics')"
          variant="outline"
          class="ml-auto"
        />
      </div>
    </UCard>
    <UTable
      :data="data?.items ?? []"
      :columns="[{ accessorKey: 'version', header: 'v' }, { accessorKey: 'algorithm', header: t('ml.algorithm') }, { accessorKey: 'status', header: t('common.status') }, { accessorKey: 'pr_auc', header: 'PR-AUC' }, { accessorKey: 'roc_auc', header: 'ROC-AUC' }, { accessorKey: 'trained_rows', header: t('ml.trainedRows') }, { accessorKey: 'training_started_at', header: t('ml.trainedAt') }]"
      class="cursor-pointer"
      :empty="t('ml.noModels')"
      @select="(_, row) => navigateTo(`${base}/ml/models/${row.original.id}`)"
    >
      <template #algorithm-cell="{ row }">
        <span class="fp-mono text-xs">{{ row.original.algorithms.supervised?.name }}</span>
      </template>
      <template #status-cell="{ row }">
        <div class="flex items-center gap-2">
          <StatusBadge
            :value="row.original.status"
            size="xs"
          />
          <UProgress
            v-if="row.original.status === 'training'"
            :model-value="row.original.progress * 100"
            size="xs"
            class="w-24"
          />
        </div>
      </template>
      <template #pr_auc-cell="{ row }">
        {{ fmtNumber(row.original.metrics.pr_auc, 3) }}
      </template>
      <template #roc_auc-cell="{ row }">
        {{ fmtNumber(row.original.metrics.roc_auc, 3) }}
      </template>
      <template #trained_rows-cell="{ row }">
        {{ fmtCompact(row.original.trained_rows) }}
      </template>
      <template #training_started_at-cell="{ row }">
        <span class="text-xs">{{ fmtDate(row.original.training_started_at) }}</span>
      </template>
    </UTable>
  </PagePanel>
</template>
