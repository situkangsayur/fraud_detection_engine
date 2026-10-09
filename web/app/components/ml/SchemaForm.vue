<script setup lang="ts">
// Renders any plugin `param_schema` (JSON Schema) as a form — new plugins need no UI changes.
import type { JsonSchema } from '#shared/types/api'
import { schemaToFields, validateParams } from '#shared/schema-form'

const props = defineProps<{ schema: JsonSchema, disabled?: boolean }>()
const model = defineModel<Record<string, unknown>>({ required: true })
const fields = computed(() => schemaToFields(props.schema))
const issues = computed(() => validateParams(model.value, props.schema))
const errors = computed(() => Object.fromEntries(issues.value.map(i => [i.path, i.message])))
defineExpose({ valid: computed(() => issues.value.length === 0) })
</script>

<template>
  <div class="grid sm:grid-cols-2 gap-3">
    <SchemaField
      v-for="f in fields"
      :key="f.key"
      :field="f"
      :root="model"
      :errors="errors"
      :disabled="disabled"
      @update:root="v => (model = v)"
    />
    <p
      v-if="!fields.length"
      class="text-sm text-muted col-span-full"
    >
      —
    </p>
  </div>
</template>
