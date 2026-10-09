<script setup lang="ts">
import type { MlModel } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const { base, apiBase } = useProject()
const id = computed(() => String(route.params.id))
const { data: model, refresh } = await useAsyncData(`model-${id.value}`, () => api.get<MlModel>(`${apiBase.value}/ml/models/${id.value}`))
useHead({ title: () => `${t('ml.model')} v${model.value?.version ?? ''}` })
let timer: ReturnType<typeof setInterval> | undefined
watch(() => model.value?.status, (s) => {
  clearInterval(timer)
  if (s === 'training' && import.meta.client) timer = setInterval(() => refresh(), 3000)
}, { immediate: true })
onBeforeUnmount(() => clearInterval(timer))
</script>

<template>
  <PagePanel :title="model ? `${t(`ml.kinds.${model.kind}`)} · v${model.version}` : t('ml.model')">
    <template #actions>
      <StatusBadge
        v-if="model"
        :value="model.status"
      />
      <ApprovalActions
        v-if="model"
        :endpoint="`${apiBase}/ml/models/${id}`"
        :status="model.status"
        :allow-shadow="false"
        @changed="refresh"
      />
    </template>
    <template v-if="model">
      <UCard class="mb-4">
        <div class="grid lg:grid-cols-2 gap-4">
          <KeyValue
            :items="[
              { label: t('ml.algorithm'), value: Object.values(model.algorithms).map(a => `${a.name}@${a.version}`).join(' + '), mono: true },
              { label: t('ml.featureSet'), value: `v${model.feature_set_version} (${model.feature_names.length})` },
              { label: t('ml.trainedAt'), value: fmtDate(model.training_started_at) },
              { label: t('ml.finishedAt'), value: fmtDate(model.training_finished_at) },
              { label: t('ml.activatedAt'), value: fmtDate(model.activated_at) },
            ]"
          />
          <div>
            <p class="text-xs text-muted uppercase mb-1">
              {{ t('ml.params') }}
            </p>
            <JsonTree
              :value="model.params"
              :open="true"
            />
          </div>
        </div>
        <UProgress
          v-if="model.status === 'training'"
          :model-value="model.progress * 100"
          class="mt-4"
        />
        <UAlert
          v-if="model.error"
          color="error"
          variant="subtle"
          :description="model.error"
          class="mt-4"
        />
      </UCard>
      <ModelMetrics
        v-if="model.kind === 'supervised' && model.status !== 'training'"
        :model="model"
      />
      <UCard v-else-if="model.kind === 'unsupervised'">
        <JsonTree
          :value="model.metrics"
          name="metrics"
          :open="true"
        />
        <UButton
          :to="`${base}/ml/unsupervised?model_id=${model.id}`"
          :label="t('ml.viewClusters')"
          variant="link"
          class="mt-2"
        />
      </UCard>
    </template>
  </PagePanel>
</template>
