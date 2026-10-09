<script setup lang="ts">
import type { TransformStep } from '#shared/types/api'
import { TRANSFORMS } from '#shared/utils/canonical'

const model = defineModel<TransformStep[]>({ required: true })
defineProps<{ disabled?: boolean }>()
const { t } = useI18n()
function spec(fn: string) { return TRANSFORMS.find(x => x.fn === fn) }
function parseParam(name: string, raw: string, type: 'string' | 'number'): unknown {
  if (name === 'map') {
    try { return JSON.parse(raw) }
    catch { return raw }
  }
  return type === 'number' ? Number(raw) : raw
}
/** Replaces the step immutably (removing the param when the input is cleared). */
function setParam(index: number, name: string, raw: string, type: 'string' | 'number') {
  model.value = model.value.map((step, j) => {
    if (j !== index) return step
    const next = Object.fromEntries(Object.entries(step).filter(([k]) => k !== name)) as TransformStep
    if (raw !== '') next[name] = parseParam(name, raw, type)
    return next
  })
}
</script>

<template>
  <div class="flex flex-wrap items-center gap-1">
    <template
      v-for="(step, i) in model"
      :key="i"
    >
      <div class="flex items-center gap-1 rounded bg-elevated px-1.5 py-0.5">
        <USelect
          v-model="step.fn"
          :items="TRANSFORMS.map(x => x.fn)"
          size="xs"
          class="w-36 fp-mono"
          :disabled="disabled"
        />
        <UInput
          v-for="p in spec(step.fn)?.params ?? []"
          :key="p.name"
          :model-value="step[p.name] === undefined ? '' : typeof step[p.name] === 'object' ? JSON.stringify(step[p.name]) : String(step[p.name])"
          :placeholder="p.placeholder ?? p.name"
          size="xs"
          class="w-40 fp-mono"
          :disabled="disabled"
          :aria-label="p.name"
          @update:model-value="(v: string | number) => setParam(i, p.name, String(v), p.type)"
        />
        <UButton
          v-if="!disabled"
          icon="i-lucide-x"
          size="xs"
          color="neutral"
          variant="ghost"
          :aria-label="t('actions.remove')"
          @click="model = model.filter((_, j) => j !== i)"
        />
      </div>
      <UIcon
        v-if="i < model.length - 1"
        name="i-lucide-arrow-right"
        class="size-3 text-muted"
      />
    </template>
    <UButton
      v-if="!disabled"
      size="xs"
      variant="link"
      icon="i-lucide-plus"
      :label="t('datasources.addTransform')"
      @click="model = [...model, { fn: 'trim' }]"
    />
  </div>
</template>
