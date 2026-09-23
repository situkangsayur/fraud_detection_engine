<script setup lang="ts">
// Evaluate a (possibly unsaved) rule against a stored event or a pasted context (POST /rules/test).
import type { RuleEnvelope } from '#shared/rules/dsl'
import type { RuleTestResult } from '#shared/types/api'

const props = defineProps<{ getEnvelope: () => RuleEnvelope }>()
const { t } = useI18n()
const api = useApi()
const { apiBase } = useProject()

const source = ref<'event' | 'context'>('event')
const eventId = ref('')
const contextText = ref(JSON.stringify({ event: { amount: 6250000, issuer_country: 'SG', geo_country: 'ID', event_type: 'transaction' }, features: { cust_cnt_24h: 2 } }, null, 2))
const result = ref<RuleTestResult | null>(null)
const busy = ref(false)
const parseError = ref('')

async function run() {
  parseError.value = ''
  let context: unknown
  if (source.value === 'context') {
    try { context = JSON.parse(contextText.value) }
    catch { parseError.value = t('errors.invalidJson'); return }
  }
  busy.value = true
  try {
    result.value = await api.post<RuleTestResult>(`${apiBase.value}/rules/test`, {
      rule: props.getEnvelope(),
      event_id: source.value === 'event' ? eventId.value || undefined : undefined,
      context: source.value === 'context' ? context : undefined,
    })
  }
  catch { result.value = null }
  finally { busy.value = false }
}
</script>

<template>
  <div class="grid lg:grid-cols-2 gap-4">
    <UCard>
      <div class="space-y-3">
        <URadioGroup
          v-model="source"
          orientation="horizontal"
          :items="[{ label: t('rules.test.fromEvent'), value: 'event' }, { label: t('rules.test.fromContext'), value: 'context' }]"
        />
        <UFormField
          v-if="source === 'event'"
          :label="t('rules.test.eventId')"
          :help="t('rules.test.eventIdHelp')"
        >
          <UInput
            v-model="eventId"
            class="w-full fp-mono"
            placeholder="uuid"
          />
        </UFormField>
        <template v-else>
          <CodeEditor
            v-model="contextText"
            height="260px"
          />
          <p
            v-if="parseError"
            class="text-xs text-error"
          >
            {{ parseError }}
          </p>
        </template>
        <UButton
          :label="t('actions.test')"
          icon="i-lucide-flask-conical"
          :loading="busy"
          :disabled="source === 'event' && !eventId"
          @click="run"
        />
      </div>
    </UCard>
    <UCard>
      <UEmpty
        v-if="!result"
        icon="i-lucide-flask-conical"
        :title="t('rules.test.noResult')"
        size="sm"
      />
      <div
        v-else
        class="space-y-3"
      >
        <div class="flex items-center gap-3">
          <StatusBadge
            :value="result.outcome"
            size="md"
          />
          <span class="text-sm">{{ t('rules.contribution') }}: <b class="tabular-nums">{{ result.contribution.toFixed(1) }}</b></span>
        </div>
        <UAlert
          v-if="result.trapped_reason"
          color="warning"
          variant="subtle"
          icon="i-lucide-triangle-alert"
          :title="t('rules.trappedBecause')"
          :description="result.trapped_reason"
        />
        <JsonTree
          :value="result.trace"
          name="trace"
          :open="true"
        />
      </div>
    </UCard>
  </div>
</template>
