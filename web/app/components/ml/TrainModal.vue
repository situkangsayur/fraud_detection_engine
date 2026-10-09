<script setup lang="ts">
// Start a training job with the project's configured algorithm(s) or an override (schema-driven params).
import type { MlAlgorithm, Project } from '#shared/types/api'

const props = defineProps<{ kind: 'supervised' | 'unsupervised', project: Project | null | undefined, algorithms: MlAlgorithm[] }>()
const emit = defineEmits<{ started: [modelId: string] }>()
const { t } = useI18n()
const api = useApi()
const toast = useToast()
const { apiBase } = useProject()

const open = ref(false)
const since = ref(90)
const sup = reactive({ algorithm: 'mlp_backprop', params: {} as Record<string, unknown> })
const uns = reactive({ anomaly_algorithm: 'isolation_forest', anomaly_params: {} as Record<string, unknown>, clustering_algorithm: 'hdbscan', clustering_params: {} as Record<string, unknown> })
watch(open, (o) => {
  if (!o || !props.project) return
  const c = props.project.ml_config
  Object.assign(sup, { algorithm: c.supervised.algorithm, params: { ...c.supervised.params } })
  Object.assign(uns, { ...c.unsupervised, anomaly_params: { ...c.unsupervised.anomaly_params }, clustering_params: { ...c.unsupervised.clustering_params } })
})
const pickers = ref<{ valid: boolean }[]>([])
const valid = computed(() => pickers.value.every(p => p?.valid !== false))
const busy = ref(false)
async function start() {
  busy.value = true
  try {
    const body = props.kind === 'supervised' ? { ...sup, since_days: since.value } : { ...uns, since_days: since.value }
    const res = await api.post<{ model_id: string }>(`${apiBase.value}/ml/${props.kind}/train`, body)
    toast.add({ title: t('ml.trainingStarted'), color: 'success' })
    open.value = false
    emit('started', res.model_id)
  }
  catch { /* toast */ }
  finally { busy.value = false }
}
</script>

<template>
  <UModal
    v-model:open="open"
    :title="kind === 'supervised' ? t('ml.trainSupervised') : t('ml.trainUnsupervised')"
    :ui="{ content: 'max-w-3xl' }"
  >
    <UButton
      icon="i-lucide-play"
      :label="t('actions.train')"
    />
    <template #body>
      <div class="space-y-5">
        <AlgorithmPicker
          v-if="kind === 'supervised'"
          :ref="(el) => { if (el) pickers[0] = el as never }"
          v-model:algorithm="sup.algorithm"
          v-model:params="sup.params"
          kind="supervised"
          :algorithms="algorithms"
        />
        <template v-else>
          <AlgorithmPicker
            :ref="(el) => { if (el) pickers[0] = el as never }"
            v-model:algorithm="uns.anomaly_algorithm"
            v-model:params="uns.anomaly_params"
            kind="anomaly"
            :label="t('ml.anomalyAlgorithm')"
            :algorithms="algorithms"
          />
          <USeparator />
          <AlgorithmPicker
            :ref="(el) => { if (el) pickers[1] = el as never }"
            v-model:algorithm="uns.clustering_algorithm"
            v-model:params="uns.clustering_params"
            kind="clustering"
            :label="t('ml.clusteringAlgorithm')"
            :algorithms="algorithms"
          />
        </template>
        <UFormField
          :label="t('ml.sinceDays')"
          :help="kind === 'supervised' ? t('ml.sinceDaysSupervisedHelp') : t('ml.sinceDaysUnsupervisedHelp')"
        >
          <UInputNumber
            v-model="since"
            :min="7"
            :max="730"
            class="w-32"
          />
        </UFormField>
      </div>
    </template>
    <template #footer>
      <div class="flex justify-end gap-2 w-full">
        <UButton
          :label="t('actions.cancel')"
          color="neutral"
          variant="ghost"
          @click="open = false"
        />
        <UButton
          :label="t('actions.train')"
          icon="i-lucide-play"
          :loading="busy"
          :disabled="!valid"
          @click="start"
        />
      </div>
    </template>
  </UModal>
</template>
