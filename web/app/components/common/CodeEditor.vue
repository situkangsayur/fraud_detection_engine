<script setup lang="ts">
// JSON code editor (CodeMirror 6), client-only. v-model is the raw text.
import { Codemirror } from 'vue-codemirror'
import { json } from '@codemirror/lang-json'
import { oneDark } from '@codemirror/theme-one-dark'

const model = defineModel<string>({ required: true })
defineProps<{ readonly?: boolean, height?: string, ariaLabel?: string }>()
const colorMode = useColorMode()
const extensions = computed(() => (colorMode.value === 'dark' ? [json(), oneDark] : [json()]))
</script>

<template>
  <ClientOnly>
    <Codemirror
      v-model="model"
      :extensions="extensions"
      :disabled="readonly"
      :style="{ height: height ?? '420px', fontSize: '12.5px' }"
      :indent-with-tab="true"
      :tab-size="2"
      :aria-label="ariaLabel ?? 'JSON editor'"
      class="rounded-md border border-default overflow-hidden"
    />
    <template #fallback>
      <USkeleton
        class="w-full"
        :style="{ height: height ?? '420px' }"
      />
    </template>
  </ClientOnly>
</template>
