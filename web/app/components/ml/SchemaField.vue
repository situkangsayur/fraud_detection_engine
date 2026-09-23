<script setup lang="ts">
import type { FieldSpec } from '#shared/schema-form'
import { formatNumberList, getAt, parseNumberList, setAt } from '#shared/schema-form'

const props = defineProps<{ field: FieldSpec, root: Record<string, unknown>, errors: Record<string, string>, disabled?: boolean }>()
const emit = defineEmits<{ 'update:root': [value: Record<string, unknown>] }>()
const { t } = useI18n()

const value = computed(() => getAt(props.root, props.field.path))
const pathKey = computed(() => props.field.path.join('.'))
const error = computed(() => props.errors[pathKey.value] ?? Object.entries(props.errors).find(([k]) => k.startsWith(`${pathKey.value}[`))?.[1])

function set(v: unknown) {
  emit('update:root', setAt(props.root, props.field.path, v))
}

const listText = ref(formatNumberList(value.value))
watch(value, (v) => { if (props.field.widget === 'number-list') listText.value = formatNumberList(v) })
function onListInput(text: string) {
  listText.value = text
  const parsed = parseNumberList(text, props.field.step === 1)
  if (parsed) set(parsed)
}

const jsonText = ref(value.value === undefined ? '' : JSON.stringify(value.value))
const jsonError = ref('')
function onJsonInput(text: string) {
  jsonText.value = text
  if (!text.trim()) { jsonError.value = ''; set(undefined); return }
  try { set(JSON.parse(text)); jsonError.value = '' }
  catch { jsonError.value = t('errors.invalidJson') }
}

const nullableToggle = computed({
  get: () => value.value === null,
  set: (isNull: boolean) => set(isNull ? null : props.field.default ?? (props.field.valueType === 'string' ? '' : 0)),
})
</script>

<template>
  <div
    v-if="field.widget === 'group'"
    class="col-span-full rounded-md border border-default p-3 space-y-3"
  >
    <p class="text-sm font-medium">
      {{ field.label }}
    </p>
    <div class="grid sm:grid-cols-2 gap-3">
      <SchemaField
        v-for="child in field.children"
        :key="child.key"
        :field="child"
        :root="root"
        :errors="errors"
        :disabled="disabled"
        @update:root="v => emit('update:root', v)"
      />
    </div>
  </div>
  <UFormField
    v-else
    :label="field.label"
    :description="field.description"
    :required="field.required"
    :error="error ?? (jsonError || undefined)"
  >
    <div class="flex items-center gap-2">
      <template v-if="!(field.nullable && value === null)">
        <UInputNumber
          v-if="field.widget === 'number'"
          :model-value="typeof value === 'number' ? value : undefined"
          :min="field.min"
          :max="field.max"
          :step="field.step === 'any' ? 0.001 : field.step"
          :format-options="{ maximumFractionDigits: 6 }"
          :disabled="disabled"
          class="w-full"
          @update:model-value="(v: number | null) => set(v === null ? undefined : v)"
        />
        <USwitch
          v-else-if="field.widget === 'switch'"
          :model-value="!!value"
          :disabled="disabled"
          @update:model-value="set"
        />
        <USelect
          v-else-if="field.widget === 'select'"
          :model-value="value as string"
          :items="field.options?.map(o => ({ label: o.label, value: o.value as string }))"
          :disabled="disabled"
          class="w-full"
          @update:model-value="set"
        />
        <USelectMenu
          v-else-if="field.widget === 'multiselect'"
          :model-value="(value as string[]) ?? []"
          multiple
          :items="field.options?.map(o => o.value as string)"
          :disabled="disabled"
          class="w-full"
          @update:model-value="set"
        />
        <UInput
          v-else-if="field.widget === 'number-list'"
          :model-value="listText"
          placeholder="64, 32"
          :disabled="disabled"
          class="w-full"
          @update:model-value="(v: string | number) => onListInput(String(v))"
        />
        <UInputTags
          v-else-if="field.widget === 'string-list'"
          :model-value="(value as string[]) ?? []"
          :disabled="disabled"
          class="w-full"
          @update:model-value="set"
        />
        <UInput
          v-else-if="field.widget === 'text'"
          :model-value="(value as string) ?? ''"
          :disabled="disabled"
          class="w-full"
          @update:model-value="set"
        />
        <UInput
          v-else
          :model-value="jsonText"
          class="w-full fp-mono"
          :disabled="disabled"
          placeholder="JSON"
          @update:model-value="(v: string | number) => onJsonInput(String(v))"
        />
      </template>
      <span
        v-else
        class="text-sm text-muted italic flex-1"
      >null ({{ t('common.auto') }})</span>
      <UCheckbox
        v-if="field.nullable"
        v-model="nullableToggle"
        label="null"
        :disabled="disabled"
      />
    </div>
  </UFormField>
</template>
