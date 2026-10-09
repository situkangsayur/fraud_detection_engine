<script setup lang="ts">
// Cytoscape.js canvas for the customer ↔ entity graph. Client-only (cytoscape needs the DOM).
import type { Core, ElementDefinition } from 'cytoscape'
import type { GraphEdge, GraphNode } from '#shared/types/api'

const props = defineProps<{ nodes: GraphNode[], edges: GraphEdge[], highlightPath?: string[] }>()
const emit = defineEmits<{ select: [node: GraphNode], expand: [node: GraphNode] }>()
const el = ref<HTMLDivElement>()
const colorMode = useColorMode()
let cy: Core | null = null

const KIND_COLORS: Record<string, string> = {
  email: '#0ea5e9', phone: '#8b5cf6', device: '#f59e0b', ip: '#64748b', card: '#ec4899',
  bank_account: '#14b8a6', address: '#84cc16', ref_transaction: '#f97316', api_client: '#6366f1',
}

function elements(): ElementDefinition[] {
  return [
    ...props.nodes.map(n => ({
      data: { id: n.id, label: n.label, type: n.type, kind: n.kind ?? '', risk: n.risk_label ?? 'unknown', center: n.is_center ? 1 : 0, raw: n },
    })),
    ...props.edges.map(e => ({ data: { id: e.id, source: e.source, target: e.target, kind: e.kind, similarity: e.similarity ?? 1 } })),
  ]
}

function style() {
  const dark = colorMode.value === 'dark'
  const text = dark ? '#e2e8f0' : '#1e293b'
  return [
    { selector: 'node', style: { 'label': 'data(label)', 'font-size': 9, 'color': text, 'text-valign': 'bottom', 'text-margin-y': 4, 'width': 18, 'height': 18, 'border-width': 1, 'border-color': dark ? '#0f172a' : '#fff' } },
    { selector: 'node[type = "customer"]', style: { 'shape': 'ellipse', 'background-color': '#6366f1', 'width': 26, 'height': 26 } },
    { selector: 'node[risk = "fraud"]', style: { 'background-color': '#e11d48', 'border-width': 3, 'border-color': '#fda4af' } },
    { selector: 'node[risk = "legit"]', style: { 'background-color': '#10b981' } },
    { selector: 'node[center = 1]', style: { 'width': 36, 'height': 36, 'border-width': 4, 'border-color': '#facc15' } },
    { selector: 'node[type = "entity"]', style: { 'shape': 'round-rectangle', 'background-color': (n: { data: (k: string) => string }) => KIND_COLORS[n.data('kind')] ?? '#94a3b8', 'font-size': 8 } },
    { selector: 'edge', style: { 'width': 1.5, 'line-color': dark ? '#475569' : '#cbd5e1', 'curve-style': 'bezier' } },
    { selector: 'edge[kind = "similar"]', style: { 'line-style': 'dashed', 'line-color': '#f59e0b', 'label': 'data(similarity)', 'font-size': 7, 'color': text } },
    { selector: '.path', style: { 'line-color': '#e11d48', 'width': 4, 'background-color': '#e11d48', 'z-index': 10 } },
    { selector: ':selected', style: { 'overlay-opacity': 0.15, 'overlay-color': '#6366f1' } },
  ]
}

async function render() {
  if (!el.value) return
  const cytoscape = (await import('cytoscape')).default
  const fcose = (await import('cytoscape-fcose')).default
  cytoscape.use(fcose)
  cy?.destroy()
  cy = cytoscape({
    container: el.value,
    elements: elements(),
    style: style() as never,
    layout: { name: 'fcose', animate: false, nodeRepulsion: 6000, idealEdgeLength: 70, randomize: true } as never,
    wheelSensitivity: 0.3,
    minZoom: 0.2,
    maxZoom: 3,
  })
  cy.on('tap', 'node', evt => emit('select', evt.target.data('raw') as GraphNode))
  cy.on('dbltap', 'node', evt => emit('expand', evt.target.data('raw') as GraphNode))
  applyPath()
}

function applyPath() {
  if (!cy) return
  cy.elements().removeClass('path')
  const path = props.highlightPath ?? []
  path.forEach((id, i) => {
    cy!.getElementById(id).addClass('path')
    const next = path[i + 1]
    if (next) cy!.edges(`[source = "${id}"][target = "${next}"], [source = "${next}"][target = "${id}"]`).addClass('path')
  })
}

function fit() { cy?.fit(undefined, 30) }
function exportPng() {
  if (!cy) return
  const a = document.createElement('a')
  a.href = cy.png({ full: true, scale: 2, bg: colorMode.value === 'dark' ? '#0f172a' : '#ffffff' })
  a.download = 'graph.png'
  a.click()
}
defineExpose({ fit, exportPng })

onMounted(render)
watch(() => [props.nodes, props.edges], render, { deep: false })
watch(() => props.highlightPath, applyPath)
watch(() => colorMode.value, () => cy?.style(style() as never))
onBeforeUnmount(() => cy?.destroy())
</script>

<template>
  <div
    ref="el"
    class="w-full h-full min-h-[480px] rounded-md bg-elevated/30"
    role="img"
    aria-label="Graph visualisation"
  />
</template>
