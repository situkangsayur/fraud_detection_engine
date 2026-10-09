<script setup lang="ts">
import type { ReferenceDefinitionT } from '#shared/rules/dsl'
import type { ConditionModel, OperandModel } from '#shared/rules/model'
import { uid } from '#shared/rules/model'

const model = defineModel<ReferenceDefinitionT<OperandModel, ConditionModel>>({ required: true })
defineProps<{ disabled?: boolean }>()
const { t } = useI18n()
const { base } = useProject()
const { data: lists } = useReferenceLists()
const selected = computed(() => lists.value.find(l => l.name === model.value.list))

watch(() => model.value.mode, (mode) => {
  if (mode === 'attribute' && !model.value.attribute_condition) {
    model.value.attribute_condition = {
      uid: uid('c'),
      type: 'leaf',
      left: { uid: uid('o'), type: 'field', path: 'event.amount' },
      op: 'gt',
      right: { uid: uid('o'), type: 'ref', path: selected.value?.columns[0]?.name ?? 'value' },
    }
  }
})
</script>

<template>
  <div class="space-y-4">
    <div class="grid lg:grid-cols-2 gap-4">
      <UFormField
        :label="t('rules.reference.list')"
        required
      >
        <USelectMenu
          v-model="model.list"
          :items="lists.map(l => ({ label: l.name, value: l.name, description: `${t(`listTypes.${l.list_type}`)} · ${l.scope === 'tenant' ? t('rules.reference.tenantWide') : t('rules.reference.project')}` }))"
          value-key="value"
          class="w-full fp-mono"
          :disabled="disabled"
        />
        <template #hint>
          <NuxtLink
            :to="`${base}/reference-lists`"
            class="text-xs text-primary"
          >
            {{ t('rules.reference.manage') }}
          </NuxtLink>
        </template>
      </UFormField>
      <UFormField
        :label="t('rules.reference.key')"
        required
        :help="t('rules.reference.keyHelp')"
      >
        <OperandEditor
          v-model="model.key"
          :allowed="['field', 'formula']"
          :disabled="disabled"
        />
      </UFormField>
    </div>
    <UFormField :label="t('rules.reference.mode')">
      <URadioGroup
        v-model="model.mode"
        orientation="horizontal"
        :items="(['exists', 'not_exists', 'attribute'] as const).map(m => ({ label: t(`rules.reference.modes.${m}`), value: m }))"
        :disabled="disabled"
      />
    </UFormField>
    <div v-if="model.mode === 'attribute' && model.attribute_condition">
      <p class="text-sm text-muted mb-2">
        {{ t('rules.reference.attributeHelp') }}
        <span
          v-if="selected?.columns.length"
          class="fp-mono text-xs"
        >({{ selected.columns.map(c => c.name).join(', ') }})</span>
      </p>
      <ConditionNode
        v-model="model.attribute_condition"
        :left-types="['field', 'ref', 'formula']"
        :right-types="['const', 'field', 'ref', 'formula']"
        :disabled="disabled"
      />
    </div>
  </div>
</template>
