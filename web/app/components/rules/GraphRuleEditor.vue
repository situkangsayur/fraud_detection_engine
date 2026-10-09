<script setup lang="ts">
import type { GraphDefinitionT } from '#shared/rules/dsl'
import { GRAPH_METRICS } from '#shared/rules/dsl'
import type { OperandModel } from '#shared/rules/model'

const model = defineModel<GraphDefinitionT<OperandModel>>({ required: true })
defineProps<{ disabled?: boolean }>()
const { t } = useI18n()
const LINK_KINDS = ['email', 'phone', 'device', 'ip', 'card', 'bank_account', 'address', 'ref_transaction', 'api_client']
</script>

<template>
  <div class="grid lg:grid-cols-2 gap-4">
    <UFormField
      :label="t('rules.graph.metric')"
      :help="t(`rules.graph.metricHelp.${model.metric}`)"
      required
    >
      <USelect
        v-model="model.metric"
        :items="GRAPH_METRICS.map(m => ({ label: t(`rules.graph.metrics.${m}`), value: m }))"
        class="w-full"
        :disabled="disabled"
      />
    </UFormField>
    <UFormField :label="t('graph.linkKinds')">
      <USelectMenu
        v-model="model.link_kinds"
        :items="LINK_KINDS"
        multiple
        class="w-full"
        :disabled="disabled"
      />
    </UFormField>
    <div class="flex flex-wrap items-end gap-4">
      <USwitch
        v-model="model.include_similar"
        :label="t('graph.includeSimilar')"
        :disabled="disabled"
      />
      <UFormField :label="t('graph.maxDepth')">
        <UInputNumber
          v-model="model.max_depth"
          :min="1"
          :max="4"
          class="w-24"
          :disabled="disabled"
        />
      </UFormField>
    </div>
    <UFormField
      :label="t('rules.compare')"
      required
    >
      <div class="flex flex-wrap items-start gap-2">
        <span class="text-sm text-muted pt-1 fp-mono">graph.{{ model.metric }}</span>
        <USelect
          v-model="model.compare.op"
          :items="['eq', 'ne', 'gt', 'gte', 'lt', 'lte', 'between'].map(o => ({ label: t(`rules.ops.${o}`), value: o }))"
          size="sm"
          class="w-32"
          :disabled="disabled"
        />
        <OperandEditor
          v-model="model.compare.right"
          :allowed="['const', 'field', 'formula']"
          :disabled="disabled"
        />
      </div>
    </UFormField>
  </div>
</template>
