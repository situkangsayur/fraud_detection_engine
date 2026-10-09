<script setup lang="ts">
// Renders LLM markdown safely: marked → DOMPurify (client only, since DOMPurify needs a DOM).
import { marked } from 'marked'
import DOMPurify from 'dompurify'

const props = defineProps<{ source: string | null | undefined }>()
const html = computed(() => (import.meta.client && props.source ? DOMPurify.sanitize(marked.parse(props.source, { async: false }) as string) : ''))
</script>

<template>
  <ClientOnly>
    <div
      class="text-sm max-w-none fp-markdown"
      v-html="html"
    />
    <template #fallback>
      <pre class="fp-json text-sm">{{ source }}</pre>
    </template>
  </ClientOnly>
</template>

<style scoped>
.fp-markdown :deep(h2) { font-size: 1.05rem; font-weight: 600; margin: 1rem 0 0.5rem; }
.fp-markdown :deep(h3) { font-size: 0.95rem; font-weight: 600; margin: 0.75rem 0 0.25rem; }
.fp-markdown :deep(ul) { list-style: disc; padding-left: 1.25rem; }
.fp-markdown :deep(ol) { list-style: decimal; padding-left: 1.25rem; }
.fp-markdown :deep(code) { font-family: var(--font-mono); font-size: 0.8em; padding: 0.1rem 0.3rem; border-radius: 0.25rem; background: var(--ui-bg-accented); }
.fp-markdown :deep(p) { margin: 0.4rem 0; }
.fp-markdown :deep(a) { color: var(--ui-primary); text-decoration: underline; }
.fp-markdown :deep(table) { border-collapse: collapse; }
.fp-markdown :deep(td), .fp-markdown :deep(th) { border: 1px solid var(--ui-border); padding: 0.25rem 0.5rem; }
</style>
