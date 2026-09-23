<script setup lang="ts">
// Definition list. Override a value's rendering with a slot named after the item's `key`.
defineProps<{ items: { key?: string, label: string, value: unknown, mono?: boolean }[] }>()
</script>

<template>
  <dl class="grid grid-cols-[minmax(8rem,auto)_1fr] gap-x-4 gap-y-1.5 text-sm">
    <template
      v-for="it in items"
      :key="it.key ?? it.label"
    >
      <dt class="text-muted">
        {{ it.label }}
      </dt>
      <dd
        class="min-w-0 break-words"
        :class="it.mono ? 'fp-mono text-xs' : ''"
      >
        <slot
          :name="it.key ?? 'unused'"
          :value="it.value"
        >
          {{ it.value === null || it.value === undefined || it.value === '' ? '—' : it.value }}
        </slot>
      </dd>
    </template>
  </dl>
</template>
