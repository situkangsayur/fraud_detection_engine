<script setup lang="ts">
// Operand editor (rule-dsl §3): const | field | formula | ref | hist. Formula args are nested operands.
import type { OperandModel } from '#shared/rules/model'
import { newOperand, uid } from '#shared/rules/model'

const model = defineModel<OperandModel>({ required: true })
const props = withDefaults(defineProps<{ allowed?: OperandModel['type'][], disabled?: boolean, depth?: number }>(), {
  allowed: () => ['field', 'const', 'formula'],
  depth: 0,
})
const { t } = useI18n()

const type = computed({
  get: () => model.value.type,
  set: (next: OperandModel['type']) => {
    const fresh = newOperand(next)
    model.value = { ...fresh, uid: model.value.uid }
  },
})
const typeItems = computed(() => props.allowed.map(a => ({ label: t(`rules.operand.${a}`), value: a })))

// Formula helpers: keep args in sync with the header "F(x,y,z) =" when present.
function syncArgsFromHeader() {
  if (model.value.type !== 'formula') return
  const m = /^\s*[A-Za-z_]\w*\s*\(([^)]*)\)\s*=/.exec(model.value.expr)
  if (!m) return
  const names = m[1]!.split(',').map(s => s.trim()).filter(Boolean)
  const existing = model.value.args
  model.value.args = names.map(name => existing.find(a => a.name === name) ?? { uid: uid('a'), name, operand: newOperand('field') })
}
</script>

<template>
  <div
    class="inline-flex flex-wrap items-start gap-1"
    :class="depth > 0 ? 'pl-2 border-l-2 border-primary/30' : ''"
  >
    <USelect
      v-if="typeItems.length > 1"
      v-model="type"
      :items="typeItems"
      size="sm"
      class="w-24"
      :disabled="disabled"
      :aria-label="t('rules.operandType')"
    />
    <FieldPathInput
      v-if="model.type === 'field'"
      v-model="model.path"
      :disabled="disabled"
    />
    <UInput
      v-else-if="model.type === 'ref'"
      v-model="model.path"
      size="sm"
      class="w-36 fp-mono"
      placeholder="max_amount"
      :disabled="disabled"
    />
    <UInput
      v-else-if="model.type === 'hist'"
      v-model="model.path"
      size="sm"
      class="w-36 fp-mono"
      placeholder="amount"
      :disabled="disabled"
    />
    <ConstValueInput
      v-else-if="model.type === 'const'"
      v-model="model.value"
      :disabled="disabled"
    />
    <div
      v-else-if="model.type === 'formula'"
      class="flex flex-col gap-1 rounded-md bg-elevated/60 p-2"
    >
      <UInput
        v-model="model.expr"
        size="sm"
        class="w-72 fp-mono"
        placeholder="F(x,y,z) = 2x + 2^y / z^2"
        :disabled="disabled"
        @blur="syncArgsFromHeader"
      />
      <div
        v-for="(arg, i) in model.args"
        :key="arg.uid"
        class="flex items-start gap-1"
      >
        <UInput
          v-model="arg.name"
          size="sm"
          class="w-14 fp-mono"
          :disabled="disabled"
          :aria-label="t('rules.argName')"
        />
        <span class="text-muted text-sm pt-1">=</span>
        <OperandEditor
          v-model="model.args[i]!.operand"
          :allowed="['field', 'const', 'formula']"
          :depth="depth + 1"
          :disabled="disabled"
        />
        <UButton
          v-if="!disabled"
          size="xs"
          color="neutral"
          variant="ghost"
          icon="i-lucide-x"
          :aria-label="t('actions.remove')"
          @click="model.args.splice(i, 1)"
        />
      </div>
      <UButton
        v-if="!disabled"
        size="xs"
        variant="link"
        icon="i-lucide-plus"
        :label="t('rules.addArg')"
        class="self-start"
        @click="model.args.push({ uid: uid('a'), name: `v${model.args.length + 1}`, operand: newOperand('field') })"
      />
    </div>
  </div>
</template>
