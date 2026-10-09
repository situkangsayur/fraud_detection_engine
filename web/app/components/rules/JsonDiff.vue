<script setup lang="ts">
import { diffLines, stableJson } from '#shared/utils/diff'

const props = defineProps<{ before: unknown, after: unknown, beforeLabel?: string, afterLabel?: string }>()
const lines = computed(() => diffLines(props.before === undefined ? '' : stableJson(props.before), props.after === undefined ? '' : stableJson(props.after)))
const changed = computed(() => lines.value.some(l => l.type !== 'same'))
const { t } = useI18n()
</script>

<template>
  <div>
    <div class="flex gap-3 text-xs mb-1">
      <span class="text-error">− {{ beforeLabel ?? t('common.before') }}</span>
      <span class="text-success">+ {{ afterLabel ?? t('common.after') }}</span>
      <span
        v-if="!changed"
        class="text-muted"
      >{{ t('common.noChanges') }}</span>
    </div>
    <div class="fp-mono text-xs rounded-md border border-default overflow-auto max-h-[32rem]">
      <div
        v-for="(l, i) in lines"
        :key="i"
        class="flex whitespace-pre"
        :class="l.type === 'add' ? 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-300' : l.type === 'del' ? 'bg-rose-500/10 text-rose-700 dark:text-rose-300' : ''"
      >
        <span class="w-8 shrink-0 text-right pr-1 text-muted select-none">{{ l.oldNo ?? '' }}</span>
        <span class="w-8 shrink-0 text-right pr-1 text-muted select-none">{{ l.newNo ?? '' }}</span>
        <span class="w-4 shrink-0 select-none">{{ l.type === 'add' ? '+' : l.type === 'del' ? '−' : ' ' }}</span>
        <span>{{ l.text }}</span>
      </div>
    </div>
  </div>
</template>
