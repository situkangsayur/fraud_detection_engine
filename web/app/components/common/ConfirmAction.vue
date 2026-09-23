<script setup lang="ts">
// Button that asks for confirmation (and optional comment) before running `action`.
const props = withDefaults(defineProps<{
  label: string
  title?: string
  description?: string
  color?: 'primary' | 'neutral' | 'error' | 'warning' | 'success' | 'info'
  variant?: 'solid' | 'outline' | 'soft' | 'subtle' | 'ghost' | 'link'
  icon?: string
  size?: 'xs' | 'sm' | 'md'
  withComment?: boolean
  commentRequired?: boolean
  disabled?: boolean
  action: (comment: string) => Promise<unknown> | unknown
}>(), { color: 'primary', variant: 'soft', size: 'sm' })

const { t } = useI18n()
const open = ref(false)
const busy = ref(false)
const comment = ref('')

async function run() {
  busy.value = true
  try {
    await props.action(comment.value)
    open.value = false
    comment.value = ''
  }
  catch { /* toast already shown by useApi */ }
  finally {
    busy.value = false
  }
}
</script>

<template>
  <!-- Trigger lives outside UModal: a modal trigger slot renders SSR/client-divergent ids (hydration mismatch). -->
  <UButton
    :label="label"
    :color="color"
    :variant="variant"
    :icon="icon"
    :size="size"
    :disabled="disabled"
    @click="open = true"
  />
  <UModal
    v-model:open="open"
    :title="title ?? label"
    :description="description"
  >
    <template #body>
      <UFormField
        v-if="withComment"
        :label="t('common.comment')"
        :required="commentRequired"
      >
        <UTextarea
          v-model="comment"
          :rows="3"
          class="w-full"
          autofocus
        />
      </UFormField>
      <p
        v-else
        class="text-sm"
      >
        {{ description ?? t('common.areYouSure') }}
      </p>
    </template>
    <template #footer>
      <div class="flex justify-end gap-2 w-full">
        <UButton
          :label="t('actions.cancel')"
          color="neutral"
          variant="ghost"
          @click="open = false"
        />
        <UButton
          :label="label"
          :color="color"
          :loading="busy"
          :disabled="commentRequired && !comment.trim()"
          @click="run"
        />
      </div>
    </template>
  </UModal>
</template>
