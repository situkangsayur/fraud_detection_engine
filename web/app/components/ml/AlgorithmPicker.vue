<script setup lang="ts">
// Algorithm (plugin) selector + its parameter form. Params are reset to the plugin defaults on algorithm change.
import type { AlgorithmKind, MlAlgorithm } from '#shared/types/api'
import { mergeWithDefaults, validateParams } from '#shared/schema-form'

const props = defineProps<{ kind: AlgorithmKind, algorithms: MlAlgorithm[], label?: string, disabled?: boolean }>()
const algorithm = defineModel<string>('algorithm', { required: true })
const params = defineModel<Record<string, unknown>>('params', { required: true })
const { t } = useI18n()

const options = computed(() => props.algorithms.filter(a => a.kind === props.kind).map(a => ({
  label: a.display_name,
  value: a.name,
  description: `${a.name} v${a.version}${a.source === 'plugin' ? ' · plugin' : ''}`,
  disabled: a.status !== 'available',
})))
const selected = computed(() => props.algorithms.find(a => a.name === algorithm.value))

watch(algorithm, (name, prev) => {
  const algo = props.algorithms.find(a => a.name === name)
  if (algo && prev !== undefined && name !== prev) params.value = mergeWithDefaults(algo.param_schema, {})
})
onMounted(() => {
  if (selected.value) params.value = mergeWithDefaults(selected.value.param_schema, params.value)
})
const valid = computed(() => !selected.value || validateParams(params.value, selected.value.param_schema).length === 0)
defineExpose({ valid })
</script>

<template>
  <div class="space-y-3">
    <UFormField
      :label="label ?? t('ml.algorithm')"
      required
    >
      <USelectMenu
        v-model="algorithm"
        :items="options"
        value-key="value"
        :disabled="disabled"
        class="w-full"
      />
    </UFormField>
    <template v-if="selected">
      <p
        v-if="selected.description"
        class="text-xs text-muted"
      >
        {{ selected.description }}
      </p>
      <SchemaForm
        v-model="params"
        :schema="selected.param_schema"
        :disabled="disabled"
      />
    </template>
  </div>
</template>
