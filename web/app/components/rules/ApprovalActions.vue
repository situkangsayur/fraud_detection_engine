<script setup lang="ts">
// Maker–checker lifecycle buttons (draft → pending_approval → active|shadow → retired) shared by rules,
// rulesets and models. The server enforces the rules; the UI only hides/disables what cannot succeed.
import type { RuleStatus } from '#shared/types/api'

const props = defineProps<{
  endpoint: string
  status: RuleStatus | string
  submittedBy?: string | null
  allowShadow?: boolean
}>()
const emit = defineEmits<{ changed: [] }>()
const { t } = useI18n()
const api = useApi()
const toast = useToast()
const session = useSessionStore()
const { can } = useProject()

const isSubmitter = computed(() => !!props.submittedBy && props.submittedBy === session.user?.id)
const target = ref<'active' | 'shadow'>('active')

async function act(action: string, body: Record<string, unknown> = {}) {
  await api.post(`${props.endpoint}/${action}`, body)
  toast.add({ title: t(`approval.done.${action}`), color: 'success' })
  emit('changed')
}
</script>

<template>
  <div class="flex flex-wrap items-center gap-2">
    <ConfirmAction
      v-if="can('analyst') && ['draft', 'shadow', 'ready'].includes(status)"
      :label="t('actions.submit')"
      icon="i-lucide-send"
      :description="t('approval.submitHelp')"
      :action="() => act('submit')"
    />
    <template v-if="can('approver') && status === 'pending_approval'">
      <UTooltip
        :text="isSubmitter ? t('approval.makerChecker') : ''"
        :disabled="!isSubmitter"
      >
        <UModal :title="t('actions.approve')">
          <UButton
            :label="t('actions.approve')"
            icon="i-lucide-badge-check"
            color="success"
            size="sm"
            :disabled="isSubmitter"
          />
          <template #body="{ close }">
            <div class="space-y-3">
              <URadioGroup
                v-if="allowShadow !== false"
                v-model="target"
                :items="[{ label: t('approval.targetActive'), value: 'active', description: t('approval.targetActiveHelp') }, { label: t('approval.targetShadow'), value: 'shadow', description: t('approval.targetShadowHelp') }]"
              />
              <div class="flex justify-end gap-2">
                <UButton
                  :label="t('actions.cancel')"
                  color="neutral"
                  variant="ghost"
                  @click="close"
                />
                <UButton
                  :label="t('actions.approve')"
                  color="success"
                  @click="act('approve', { target_status: target }).then(close)"
                />
              </div>
            </div>
          </template>
        </UModal>
      </UTooltip>
      <ConfirmAction
        :label="t('actions.reject')"
        color="error"
        icon="i-lucide-x"
        with-comment
        comment-required
        :action="(comment: string) => act('reject', { comment })"
      />
    </template>
    <ConfirmAction
      v-if="can('approver') && ['active', 'shadow'].includes(status)"
      :label="t('actions.retire')"
      color="error"
      variant="ghost"
      icon="i-lucide-archive"
      :description="t('approval.retireHelp')"
      :action="() => act('retire')"
    />
  </div>
</template>
