<script setup lang="ts">
// ECharts wrapper: client-only, autoresize, follows light/dark mode.
import type { EChartsOption } from 'echarts'

const props = defineProps<{ option: EChartsOption, height?: string }>()
const emit = defineEmits<{ click: [params: unknown] }>()
const colorMode = useColorMode()
const merged = computed<EChartsOption>(() => ({
  backgroundColor: 'transparent',
  textStyle: { fontFamily: 'Inter, ui-sans-serif, system-ui' },
  animationDuration: 300,
  ...props.option,
}))
</script>

<template>
  <ClientOnly>
    <VChart
      :option="merged"
      :theme="colorMode.value === 'dark' ? 'dark' : undefined"
      autoresize
      :style="{ height: height ?? '280px', width: '100%' }"
      @click="(p: unknown) => emit('click', p)"
    />
    <template #fallback>
      <USkeleton
        class="w-full"
        :style="{ height: height ?? '280px' }"
      />
    </template>
  </ClientOnly>
</template>
