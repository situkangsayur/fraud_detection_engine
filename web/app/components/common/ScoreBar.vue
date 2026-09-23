<script setup lang="ts">
const props = defineProps<{ score: number | null | undefined, thresholds?: { review: number, decline: number }, compact?: boolean }>()
const color = computed(() => scoreColor(props.score, props.thresholds))
const barClass = computed(() => ({ success: 'bg-emerald-500', warning: 'bg-amber-500', error: 'bg-rose-500' } as Record<string, string>)[color.value] ?? 'bg-neutral-400')
</script>

<template>
  <div
    class="flex items-center gap-2"
    :class="compact ? 'min-w-24' : 'min-w-32'"
  >
    <div
      class="h-1.5 flex-1 rounded-full bg-accented overflow-hidden"
      role="meter"
      :aria-valuenow="score ?? 0"
      aria-valuemin="0"
      aria-valuemax="100"
    >
      <div
        class="h-full rounded-full"
        :class="barClass"
        :style="{ width: `${Math.max(0, Math.min(100, score ?? 0))}%` }"
      />
    </div>
    <span class="tabular-nums text-xs w-9 text-right">{{ score === null || score === undefined ? '—' : score.toFixed(1) }}</span>
  </div>
</template>
