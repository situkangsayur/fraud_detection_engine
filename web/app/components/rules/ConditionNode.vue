<script setup lang="ts">
// Recursive condition tree editor (rule-dsl §4): leaf | all | any | not | at_least.
import type { Operator } from '#shared/rules/dsl'
import { OPERATORS, UNARY_OPERATORS } from '#shared/rules/dsl'
import type { ConditionModel, OperandModel } from '#shared/rules/model'
import { newGroup, newLeaf, newOperand, uid } from '#shared/rules/model'

const model = defineModel<ConditionModel>({ required: true })
const props = withDefaults(defineProps<{
  leftTypes?: OperandModel['type'][]
  rightTypes?: OperandModel['type'][]
  operators?: Operator[]
  showWeight?: boolean
  removable?: boolean
  disabled?: boolean
  depth?: number
}>(), { leftTypes: () => ['field', 'formula'], rightTypes: () => ['const', 'field', 'formula'], operators: () => OPERATORS, depth: 0 })
const emit = defineEmits<{ remove: [] }>()
const { t } = useI18n()

const groupType = computed({
  get: () => model.value.type,
  set: (next: ConditionModel['type']) => {
    const cur = model.value
    const children = cur.type === 'all' || cur.type === 'any' || cur.type === 'at_least' ? cur.children : cur.type === 'not' ? [cur.child] : [cur]
    if (next === 'all' || next === 'any') model.value = { uid: cur.uid, type: next, children }
    else if (next === 'at_least') model.value = { uid: cur.uid, type: 'at_least', n: Math.min(2, children.length), children }
    else if (next === 'not') model.value = { uid: cur.uid, type: 'not', child: children[0] ?? newLeaf() }
  },
})

function onOpChange(op: Operator) {
  if (model.value.type !== 'leaf') return
  model.value.op = op
  if (UNARY_OPERATORS.includes(op)) delete model.value.right
  else if (!model.value.right) model.value.right = newOperand('const')
  if (op === 'between' && model.value.right?.type === 'const' && !Array.isArray(model.value.right.value)) model.value.right.value = [0, 100]
  if ((op === 'in' || op === 'not_in') && model.value.right?.type === 'const' && !Array.isArray(model.value.right.value)) model.value.right.value = []
}
function wrap(kind: 'all' | 'not') {
  const cur = model.value
  model.value = kind === 'not' ? { uid: uid('c'), type: 'not', child: cur } : { uid: uid('c'), type: 'all', children: [cur] }
}
function removeChild(i: number) {
  if (model.value.type === 'all' || model.value.type === 'any' || model.value.type === 'at_least') model.value.children.splice(i, 1)
}
const leafOptions = computed(() => (model.value.type === 'leaf' ? model.value.options ?? {} : {}))
function setOption(key: string, value: unknown) {
  if (model.value.type !== 'leaf') return
  model.value.options = { ...(model.value.options ?? {}), [key]: value }
}
const groupColor = computed(() => ['border-primary/40', 'border-emerald-500/40', 'border-amber-500/40', 'border-violet-500/40'][props.depth % 4])
</script>

<template>
  <!-- leaf -->
  <div
    v-if="model.type === 'leaf'"
    class="flex flex-wrap items-start gap-2 rounded-md border border-default bg-default p-2"
  >
    <OperandEditor
      v-model="model.left"
      :allowed="leftTypes"
      :disabled="disabled"
    />
    <USelect
      :model-value="model.op"
      :items="operators.map(o => ({ label: t(`rules.ops.${o}`), value: o }))"
      size="sm"
      class="w-36"
      :disabled="disabled"
      :aria-label="t('rules.operator')"
      @update:model-value="onOpChange"
    />
    <OperandEditor
      v-if="model.right"
      v-model="model.right"
      :allowed="rightTypes"
      :disabled="disabled"
    />
    <template v-if="model.op === 'similar'">
      <UInputNumber
        :model-value="(leafOptions.threshold as number) ?? 0.85"
        :min="0"
        :max="1"
        :step="0.01"
        size="sm"
        class="w-24"
        :disabled="disabled"
        @update:model-value="(v: number | null) => setOption('threshold', v)"
      />
      <USelect
        :model-value="(leafOptions.method as string) ?? 'jaro_winkler'"
        :items="['jaro_winkler', 'levenshtein_ratio']"
        size="sm"
        class="w-40"
        :disabled="disabled"
        @update:model-value="(v: string) => setOption('method', v)"
      />
    </template>
    <UCheckbox
      v-if="['eq', 'ne', 'in', 'not_in', 'contains', 'starts_with', 'ends_with'].includes(model.op)"
      :model-value="!!leafOptions.case_insensitive"
      :label="t('rules.caseInsensitive')"
      size="sm"
      class="pt-1"
      :disabled="disabled"
      @update:model-value="(v: boolean | 'indeterminate') => setOption('case_insensitive', v === true)"
    />
    <UFormField
      v-if="showWeight"
      :label="t('rules.weight')"
      size="xs"
      class="w-20"
    >
      <UInputNumber
        :model-value="model.weight ?? 1"
        :min="0"
        :step="0.5"
        size="sm"
        :disabled="disabled"
        @update:model-value="(v: number | null) => { if (model.type === 'leaf') model.weight = v ?? 1 }"
      />
    </UFormField>
    <div
      v-if="!disabled"
      class="ml-auto flex gap-1"
    >
      <UDropdownMenu :items="[[{ label: t('rules.wrapGroup'), icon: 'i-lucide-brackets', onSelect: () => wrap('all') }, { label: t('rules.wrapNot'), icon: 'i-lucide-circle-slash', onSelect: () => wrap('not') }]]">
        <UButton
          size="xs"
          color="neutral"
          variant="ghost"
          icon="i-lucide-ellipsis"
          :aria-label="t('actions.more')"
        />
      </UDropdownMenu>
      <UButton
        v-if="removable"
        size="xs"
        color="neutral"
        variant="ghost"
        icon="i-lucide-trash-2"
        :aria-label="t('actions.remove')"
        @click="emit('remove')"
      />
    </div>
  </div>

  <!-- NOT -->
  <div
    v-else-if="model.type === 'not'"
    class="rounded-md border-l-4 p-2 space-y-2 bg-elevated/40"
    :class="groupColor"
  >
    <div class="flex items-center gap-2">
      <UBadge
        color="error"
        variant="soft"
      >
        NOT
      </UBadge>
      <div
        v-if="!disabled"
        class="ml-auto flex gap-1"
      >
        <UButton
          size="xs"
          color="neutral"
          variant="ghost"
          :label="t('rules.unwrap')"
          @click="model = model.type === 'not' ? model.child : model"
        />
        <UButton
          v-if="removable"
          size="xs"
          color="neutral"
          variant="ghost"
          icon="i-lucide-trash-2"
          :aria-label="t('actions.remove')"
          @click="emit('remove')"
        />
      </div>
    </div>
    <ConditionNode
      v-model="model.child"
      :left-types="leftTypes"
      :right-types="rightTypes"
      :operators="operators"
      :show-weight="showWeight"
      :disabled="disabled"
      :depth="depth + 1"
    />
  </div>

  <!-- ALL / ANY / AT LEAST -->
  <div
    v-else
    class="rounded-md border-l-4 p-2 space-y-2 bg-elevated/40"
    :class="groupColor"
  >
    <div class="flex flex-wrap items-center gap-2">
      <USelect
        v-model="groupType"
        :items="[{ label: t('rules.group.all'), value: 'all' }, { label: t('rules.group.any'), value: 'any' }, { label: t('rules.group.at_least'), value: 'at_least' }, { label: t('rules.group.not'), value: 'not' }]"
        size="sm"
        class="w-44"
        :disabled="disabled"
      />
      <UInputNumber
        v-if="model.type === 'at_least'"
        v-model="model.n"
        :min="1"
        :max="Math.max(1, model.children.length)"
        size="sm"
        class="w-20"
        :disabled="disabled"
      />
      <span
        v-if="model.type === 'at_least'"
        class="text-sm text-muted"
      >{{ t('rules.ofTheFollowing', { n: model.children.length }) }}</span>
      <div
        v-if="!disabled"
        class="ml-auto flex gap-1"
      >
        <UButton
          size="xs"
          variant="soft"
          icon="i-lucide-plus"
          :label="t('rules.addCondition')"
          @click="model.children.push(newLeaf())"
        />
        <UButton
          size="xs"
          variant="soft"
          color="neutral"
          icon="i-lucide-brackets"
          :label="t('rules.addGroup')"
          @click="model.children.push(newGroup(model.type === 'all' ? 'any' : 'all'))"
        />
        <UButton
          v-if="removable"
          size="xs"
          color="neutral"
          variant="ghost"
          icon="i-lucide-trash-2"
          :aria-label="t('actions.remove')"
          @click="emit('remove')"
        />
      </div>
    </div>
    <ConditionNode
      v-for="(child, i) in model.children"
      :key="child.uid"
      v-model="model.children[i]!"
      :left-types="leftTypes"
      :right-types="rightTypes"
      :operators="operators"
      :show-weight="showWeight"
      :disabled="disabled"
      :depth="depth + 1"
      removable
      @remove="removeChild(i)"
    />
    <p
      v-if="!model.children.length"
      class="text-xs text-warning"
    >
      {{ t('rules.emptyGroup') }}
    </p>
  </div>
</template>
