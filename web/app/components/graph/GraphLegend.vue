<script setup lang="ts">
// Legend for GraphCanvas: same colours and shapes as the canvas (utils/graph.ts), with counts for the graph on screen.
import type { GraphEdge, GraphNode } from '#shared/types/api'

const props = defineProps<{ nodes: GraphNode[], edges: GraphEdge[], showPath: boolean }>()
const { t } = useI18n()
const open = ref(true)
const C = GRAPH_CUSTOMER_COLORS

const customerRows = computed(() => {
  const count = (risk: string) => props.nodes.filter(n => n.type === 'customer' && (n.risk_label ?? 'unknown') === risk).length
  return [
    { key: 'fraud', label: t('graph.legendFraud'), color: C.fraud, ring: '#fda4af', n: count('fraud') },
    { key: 'legit', label: t('graph.legendLegit'), color: C.legit, ring: null, n: count('legit') },
    { key: 'unknown', label: t('graph.legendUnknown'), color: C.unknown, ring: null, n: count('unknown') },
  ]
})

// Only kinds present in the current graph, most frequent first.
const entityRows = computed(() => {
  const counts = new Map<string, number>()
  for (const n of props.nodes) if (n.type === 'entity' && n.kind) counts.set(n.kind, (counts.get(n.kind) ?? 0) + 1)
  return [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([kind, n]) => ({ kind, n, color: graphEntityColor(kind) }))
})
const similarCount = computed(() => props.edges.filter(e => e.kind === 'similar').length)
</script>

<template>
  <div class="rounded-md border border-default bg-default/95 shadow-sm text-xs backdrop-blur-sm max-w-64">
    <button
      type="button"
      class="flex w-full items-center justify-between gap-3 px-3 py-2 font-medium focus-visible:outline-2 focus-visible:outline-primary rounded-md"
      :aria-expanded="open"
      @click="open = !open"
    >
      <span>{{ t('graph.legend') }}</span>
      <UIcon
        :name="open ? 'i-lucide-chevron-down' : 'i-lucide-chevron-up'"
        class="size-4 text-muted"
      />
    </button>
    <div
      v-if="open"
      class="px-3 pb-3 space-y-3 max-h-[50vh] overflow-y-auto"
    >
      <ul class="space-y-1.5">
        <li class="flex items-center gap-2">
          <span
            class="size-4 shrink-0 rounded-full border-[3px]"
            :style="{ background: C.unknown, borderColor: C.center }"
          />
          <span class="flex-1">{{ t('graph.legendCenter') }}</span>
        </li>
        <li
          v-for="r in customerRows"
          :key="r.key"
          class="flex items-center gap-2"
        >
          <span
            class="size-3.5 shrink-0 rounded-full"
            :style="{ background: r.color, boxShadow: r.ring ? `0 0 0 2px ${r.ring}` : undefined }"
          />
          <span class="flex-1">{{ r.label }}</span>
          <span class="tabular-nums text-muted">{{ r.n }}</span>
        </li>
      </ul>
      <div v-if="entityRows.length">
        <p class="text-muted mb-1.5">
          {{ t('graph.entities') }}
        </p>
        <ul class="space-y-1.5">
          <li
            v-for="r in entityRows"
            :key="r.kind"
            class="flex items-center gap-2"
          >
            <span
              class="size-3.5 shrink-0 rounded-[3px]"
              :style="{ background: r.color }"
            />
            <span class="flex-1">{{ t(`graph.kinds.${r.kind}`) }}</span>
            <span class="tabular-nums text-muted">{{ r.n }}</span>
          </li>
        </ul>
      </div>
      <ul class="space-y-1.5 border-t border-default pt-2">
        <li class="flex items-center gap-2">
          <span class="w-5 shrink-0 border-t-2 border-muted" />
          <span class="flex-1">{{ t('graph.legendLink') }}</span>
        </li>
        <li
          v-if="similarCount"
          class="flex items-center gap-2"
        >
          <span
            class="w-5 shrink-0 border-t-2 border-dashed"
            :style="{ borderColor: C.similar }"
          />
          <span class="flex-1">{{ t('graph.legendSimilar') }}</span>
          <span class="tabular-nums text-muted">{{ similarCount }}</span>
        </li>
        <li
          v-if="showPath"
          class="flex items-center gap-2"
        >
          <span
            class="w-5 shrink-0 border-t-4"
            :style="{ borderColor: C.path }"
          />
          <span class="flex-1">{{ t('graph.legendPath') }}</span>
        </li>
      </ul>
      <p class="text-muted">
        {{ t('graph.dblClickExpand') }}
      </p>
    </div>
  </div>
</template>
