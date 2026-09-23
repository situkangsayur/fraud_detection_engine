<script setup lang="ts">
import type { RuleEnvelopeModel } from '#shared/rules/model'
import { describeDefinition, envelopeFromDsl, envelopeToDsl } from '#shared/rules/model'
import type { RuleDetail } from '#shared/types/api'
import type { RuleEditorExpose } from '~/types/ui'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const toast = useToast()
const { base, apiBase, can } = useProject()
const id = computed(() => String(route.params.id))

const { data: rule, refresh } = await useAsyncData(`rule-${id.value}`, () => api.get<RuleDetail>(`${apiBase.value}/rules/${id.value}`))
useHead({ title: () => `${rule.value?.code ?? t('common.rule')}` })

const envelope = ref<RuleEnvelopeModel | null>(rule.value ? envelopeFromDsl(rule.value.envelope) : null)
watch(rule, (r) => { if (r) envelope.value = envelopeFromDsl(r.envelope) })
const original = computed(() => (rule.value ? JSON.stringify(envelopeToDsl(envelopeFromDsl(rule.value.envelope))) : ''))
const dirty = computed(() => !!envelope.value && JSON.stringify(envelopeToDsl(envelope.value)) !== original.value)
const editable = computed(() => can('analyst') && rule.value?.status !== 'retired' && rule.value?.status !== 'pending_approval')

const editor = ref<RuleEditorExpose>()
const tab = ref(String(route.query.tab ?? 'definition'))
const changeNote = ref('')
const saving = ref(false)
async function saveVersion() {
  if (!editor.value) return
  saving.value = true
  try {
    if (!(await editor.value.validate())) { toast.add({ title: t('rules.fixErrors'), color: 'warning' }); return }
    await api.put(`${apiBase.value}/rules/${id.value}`, { ...editor.value.getEnvelope(), change_note: changeNote.value || undefined })
    changeNote.value = ''
    toast.add({ title: t('rules.versionSaved'), color: 'success' })
    await refresh()
  }
  catch { /* toast */ }
  finally { saving.value = false }
}

const compareA = ref<number>(0)
const compareB = ref<number>(0)
watch(rule, (r) => {
  if (!r) return
  compareB.value = r.current_version
  compareA.value = Math.max(1, r.current_version - 1)
}, { immediate: true })
const versionA = computed(() => rule.value?.versions.find(v => v.version === compareA.value))
const versionB = computed(() => rule.value?.versions.find(v => v.version === compareB.value))
</script>

<template>
  <PagePanel :title="rule ? `${rule.code} · ${rule.name}` : t('common.rule')">
    <template #actions>
      <StatusBadge
        v-if="rule"
        :value="rule.status"
      />
      <ApprovalActions
        v-if="rule"
        :endpoint="`${apiBase}/rules/${id}`"
        :status="rule.status"
        :submitted-by="rule.submitted_by"
        @changed="refresh"
      />
    </template>

    <template v-if="rule && envelope">
      <UAlert
        v-if="rule.status === 'pending_approval'"
        color="warning"
        variant="subtle"
        icon="i-lucide-hourglass"
        :title="t('rules.pendingBanner')"
        class="mb-3"
      />
      <UAlert
        v-if="rule.status === 'shadow'"
        color="info"
        variant="subtle"
        icon="i-lucide-eye-off"
        :title="t('rules.shadowBanner')"
        class="mb-3"
      />
      <p class="fp-mono text-xs text-muted mb-3 break-words">
        {{ describeDefinition(rule.envelope.definition) }}
      </p>

      <UTabs
        v-model="tab"
        :items="[
          { label: t('rules.tabs.definition'), value: 'definition', icon: 'i-lucide-pencil-ruler' },
          { label: t('actions.test'), value: 'test', icon: 'i-lucide-flask-conical' },
          { label: t('actions.backtest'), value: 'backtest', icon: 'i-lucide-history' },
          { label: t('rules.tabs.versions', { n: rule.versions.length }), value: 'versions', icon: 'i-lucide-git-compare' },
        ]"
        variant="link"
        :content="false"
        class="mb-4"
      />

      <div
        v-show="tab === 'definition'"
        class="space-y-3"
      >
        <RuleEditor
          ref="editor"
          v-model="envelope"
          :disabled="!editable"
        />
        <UCard
          v-if="editable"
          :ui="{ body: 'p-3 sm:p-3' }"
        >
          <div class="flex flex-wrap items-end gap-3">
            <UFormField
              :label="t('rules.changeNote')"
              class="flex-1 min-w-64"
            >
              <UInput
                v-model="changeNote"
                class="w-full"
                :placeholder="t('rules.changeNotePlaceholder')"
              />
            </UFormField>
            <UButton
              :label="t('rules.saveNewVersion')"
              icon="i-lucide-save"
              :loading="saving"
              :disabled="!dirty"
              @click="saveVersion"
            />
          </div>
          <p class="text-xs text-muted mt-2">
            {{ t('rules.newVersionHelp') }}
          </p>
        </UCard>
      </div>
      <RuleTestPanel
        v-if="tab === 'test' && editor"
        :get-envelope="editor.getEnvelope"
      />
      <BacktestPanel
        v-if="tab === 'backtest'"
        :endpoint="`${apiBase}/rules/${id}/backtest`"
      />
      <div
        v-if="tab === 'versions'"
        class="grid xl:grid-cols-[18rem_1fr] gap-4"
      >
        <UCard>
          <ul class="space-y-2">
            <li
              v-for="v in [...rule.versions].reverse()"
              :key="v.version"
              class="text-sm"
            >
              <div class="flex items-center gap-2">
                <UBadge
                  :color="v.version === rule.current_version ? 'primary' : 'neutral'"
                  variant="soft"
                  size="sm"
                >
                  v{{ v.version }}
                </UBadge>
                <span class="text-xs text-muted">{{ fmtDate(v.created_at) }}</span>
              </div>
              <p class="text-xs mt-0.5">
                {{ v.change_note ?? '—' }}
              </p>
            </li>
          </ul>
        </UCard>
        <UCard>
          <div class="flex gap-2 mb-3">
            <USelect
              v-model="compareA"
              :items="rule.versions.map(v => ({ label: `v${v.version}`, value: v.version }))"
              class="w-28"
            />
            <UIcon
              name="i-lucide-arrow-right"
              class="self-center"
            />
            <USelect
              v-model="compareB"
              :items="rule.versions.map(v => ({ label: `v${v.version}`, value: v.version }))"
              class="w-28"
            />
          </div>
          <JsonDiff
            :before="versionA?.envelope"
            :after="versionB?.envelope"
            :before-label="`v${compareA}`"
            :after-label="`v${compareB}`"
          />
        </UCard>
      </div>
    </template>
    <UButton
      :to="`${base}/rules`"
      icon="i-lucide-arrow-left"
      variant="link"
      :label="t('nav.rules')"
      class="mt-4"
    />
  </PagePanel>
</template>
