<script setup lang="ts">
// Recursive, collapsible JSON viewer (used for source payloads, features, traces).
const props = withDefaults(defineProps<{ value: unknown, name?: string, depth?: number, open?: boolean }>(), { depth: 0, open: undefined })
const isObj = computed(() => typeof props.value === 'object' && props.value !== null)
const entries = computed(() => (isObj.value ? Object.entries(props.value as Record<string, unknown>) : []))
const expanded = ref(props.open ?? props.depth < 2)
const preview = computed(() => (Array.isArray(props.value) ? `[${(props.value as unknown[]).length}]` : `{${entries.value.length}}`))
function fmt(v: unknown) {
  if (typeof v === 'string') return `"${v}"`
  return String(v)
}
function cls(v: unknown) {
  if (v === null || v === undefined) return 'text-muted italic'
  if (typeof v === 'number') return 'text-sky-600 dark:text-sky-400'
  if (typeof v === 'boolean') return 'text-violet-600 dark:text-violet-400'
  return 'text-emerald-700 dark:text-emerald-400'
}
</script>

<template>
  <div class="fp-mono text-xs leading-5">
    <template v-if="isObj">
      <button
        type="button"
        class="inline-flex items-center gap-1 hover:text-primary"
        :aria-expanded="expanded"
        @click="expanded = !expanded"
      >
        <UIcon
          :name="expanded ? 'i-lucide-chevron-down' : 'i-lucide-chevron-right'"
          class="size-3"
        />
        <span
          v-if="name !== undefined"
          class="font-medium"
        >{{ name }}:</span>
        <span class="text-muted">{{ preview }}</span>
      </button>
      <div
        v-if="expanded"
        class="pl-4 border-l border-default ml-1.5"
      >
        <JsonTree
          v-for="[k, v] in entries"
          :key="k"
          :name="k"
          :value="v"
          :depth="depth + 1"
        />
      </div>
    </template>
    <div
      v-else
      class="pl-4"
    >
      <span
        v-if="name !== undefined"
        class="font-medium"
      >{{ name }}: </span>
      <span :class="cls(value)">{{ fmt(value) }}</span>
    </div>
  </div>
</template>
