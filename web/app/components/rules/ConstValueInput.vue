<script setup lang="ts">
// Typed constant editor: number | string | bool | null | list. The stored value keeps its JSON type.
const model = defineModel<unknown>({ required: true })
defineProps<{ disabled?: boolean }>()
const { t } = useI18n()

type Kind = 'number' | 'string' | 'bool' | 'null' | 'list'
function kindOf(v: unknown): Kind {
  if (v === null || v === undefined) return 'null'
  if (Array.isArray(v)) return 'list'
  if (typeof v === 'number') return 'number'
  if (typeof v === 'boolean') return 'bool'
  return 'string'
}
const kind = computed<Kind>({
  get: () => kindOf(model.value),
  set: (k) => {
    model.value = k === 'number' ? 0 : k === 'string' ? '' : k === 'bool' ? true : k === 'null' ? null : []
  },
})
const listText = ref(Array.isArray(model.value) ? (model.value as unknown[]).join(', ') : '')
function onList(text: string) {
  listText.value = text
  const parts = text.split(',').map(s => s.trim()).filter(Boolean)
  model.value = parts.every(p => p !== '' && !Number.isNaN(Number(p))) ? parts.map(Number) : parts
}
</script>

<template>
  <div class="flex items-center gap-1">
    <USelect
      v-model="kind"
      :items="(['number', 'string', 'bool', 'null', 'list'] as const).map(k => ({ label: t(`rules.const.${k}`), value: k }))"
      size="sm"
      class="w-24"
      :disabled="disabled"
    />
    <UInputNumber
      v-if="kind === 'number'"
      v-model="(model as number)"
      size="sm"
      class="w-36"
      :disabled="disabled"
      :format-options="{ maximumFractionDigits: 8, useGrouping: true }"
    />
    <UInput
      v-else-if="kind === 'string'"
      v-model="(model as string)"
      size="sm"
      class="w-40"
      :disabled="disabled"
    />
    <USwitch
      v-else-if="kind === 'bool'"
      v-model="(model as boolean)"
      :disabled="disabled"
    />
    <UInput
      v-else-if="kind === 'list'"
      :model-value="listText"
      size="sm"
      class="w-48"
      placeholder="a, b, c"
      :disabled="disabled"
      @update:model-value="(v: string | number) => onList(String(v))"
    />
  </div>
</template>
