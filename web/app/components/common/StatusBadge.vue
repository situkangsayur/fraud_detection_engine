<script setup lang="ts">
// Colour-coded badge for any status/decision/outcome value; label is translated when a key exists.
const props = defineProps<{ value: string | null | undefined, size?: 'xs' | 'sm' | 'md' }>()
const { t, te } = useI18n()
const label = computed(() => {
  if (!props.value) return '—'
  const key = `status.${props.value}`
  return te(key) ? t(key) : props.value.replace(/_/g, ' ')
})
</script>

<template>
  <UBadge
    :color="statusColor(value)"
    variant="subtle"
    :size="size ?? 'sm'"
  >
    {{ label }}
  </UBadge>
</template>
