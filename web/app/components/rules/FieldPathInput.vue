<script setup lang="ts">
// Field path autocomplete from the project's field catalog; free text is allowed (e.g. new source.* paths).
const model = defineModel<string>({ required: true })
const props = defineProps<{ items?: string[], placeholder?: string, disabled?: boolean, prefix?: string }>()
const { paths } = useFieldCatalog()
const options = computed(() => {
  const base = props.items ?? paths.value
  const list = props.prefix ? base.filter(p => p.startsWith(props.prefix!)) : base
  return model.value && !list.includes(model.value) ? [model.value, ...list] : list
})
</script>

<template>
  <UInputMenu
    v-model="model"
    :items="options"
    :placeholder="placeholder ?? 'event.amount'"
    :disabled="disabled"
    create-item
    class="min-w-48 fp-mono"
    size="sm"
    @create="(v: string) => (model = v)"
  />
</template>
