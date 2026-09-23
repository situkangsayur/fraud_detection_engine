<script setup lang="ts">
defineProps<{ label: string, value: string | number, icon?: string, hint?: string, color?: string }>()
</script>

<template>
  <UCard :ui="{ body: 'p-4 sm:p-4' }">
    <div class="flex items-start gap-3">
      <div
        v-if="icon"
        class="rounded-md p-2 bg-primary/10 text-primary"
        :class="color"
      >
        <UIcon
          :name="icon"
          class="size-5"
        />
      </div>
      <div class="min-w-0">
        <p class="text-xs uppercase tracking-wide text-muted truncate">
          {{ label }}
        </p>
        <p class="text-2xl font-semibold tabular-nums">
          {{ value }}
        </p>
        <p
          v-if="hint"
          class="text-xs text-muted"
        >
          {{ hint }}
        </p>
      </div>
    </div>
  </UCard>
</template>
