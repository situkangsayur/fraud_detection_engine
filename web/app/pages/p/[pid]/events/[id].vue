<script setup lang="ts">
import type { DecisionOut, EventDetail, FraudType, ProjectSettings } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const toast = useToast()
const { pid, base, apiBase, can } = useProject()
const id = computed(() => String(route.params.id))

const { data: ev, refresh } = await useAsyncData(`event-${id.value}`, () => api.get<EventDetail>(`${apiBase.value}/events/${id.value}`))
const { data: settings } = await useAsyncData(`settings-${pid.value}`, () => api.get<ProjectSettings>(`${apiBase.value}/settings`, { silent: true }).catch(() => null))
useHead({ title: () => `${t('common.event')} ${ev.value?.event.external_id ?? ''}` })

const tab = ref('decision')
const tabs = computed(() => [
  { label: t('events.tabs.decision'), value: 'decision', icon: 'i-lucide-gavel' },
  { label: t('events.tabs.rules'), value: 'rules', icon: 'i-lucide-list-checks' },
  { label: t('events.tabs.event'), value: 'event', icon: 'i-lucide-file-json' },
  { label: t('events.tabs.source'), value: 'source', icon: 'i-lucide-database' },
  { label: t('events.tabs.features'), value: 'features', icon: 'i-lucide-sigma' },
  { label: t('events.tabs.labels'), value: 'labels', icon: 'i-lucide-tag' },
])

const rescoring = ref(false)
async function rescore() {
  rescoring.value = true
  try {
    await api.post<DecisionOut>(`${apiBase.value}/events/${id.value}/rescore`)
    toast.add({ title: t('events.rescored'), color: 'success' })
    await refresh()
  }
  catch { /* toast */ }
  finally { rescoring.value = false }
}

const FRAUD_TYPES: FraudType[] = ['carding', 'account_takeover', 'bank_account_takeover', 'system_breach', 'promo_abuse', 'refund_abuse', 'money_mule', 'other']
const labelForm = reactive({ label: 'fraud' as 'fraud' | 'legit', fraud_type: 'carding' as FraudType, notes: '', apply_to_customer: false })
async function addLabel() {
  await api.post(`${apiBase.value}/labels`, { subject_type: 'event', subject_id: id.value, source: 'analyst', ...labelForm, fraud_type: labelForm.label === 'fraud' ? labelForm.fraud_type : undefined })
  toast.add({ title: t('events.labelAdded'), color: 'success' })
  await refresh()
}
</script>

<template>
  <PagePanel :title="`${t('common.event')} ${ev?.event.external_id ?? ''}`">
    <template #actions>
      <UButton
        v-if="ev"
        :to="`${base}/graph?customer=${ev.event.customer_id}`"
        icon="i-lucide-share-2"
        color="neutral"
        variant="outline"
        :label="t('actions.openGraph')"
      />
      <UButton
        v-if="ev?.case"
        :to="`${base}/cases/${ev.case?.id}`"
        icon="i-lucide-briefcase"
        color="neutral"
        variant="outline"
        :label="t('events.openCase')"
      />
      <UButton
        v-if="can('analyst') && ev?.decision"
        icon="i-lucide-rotate-cw"
        :label="t('actions.rescore')"
        :loading="rescoring"
        @click="rescore"
      />
    </template>

    <template v-if="ev">
      <UCard class="mb-4">
        <KeyValue
          :items="[
            { label: t('common.type'), value: ev.event.event_type },
            { label: t('common.occurred'), value: fmtDate(ev.event.occurred_at) },
            { key: 'customer', label: t('common.customer'), value: (ev.customer?.external_id ?? shortId(ev.event.customer_id)) },
            { label: t('common.amount'), value: fmtMoney(ev.event.amount, ev.event.currency ?? 'IDR') },
            { label: t('common.channel'), value: ev.event.channel },
            { label: 'ID', value: ev.event.id, mono: true },
          ]"
        >
          <template #customer>
            <NuxtLink
              :to="`${base}/customers/${ev.event.customer_id}`"
              class="text-primary fp-mono text-xs"
            >
              {{ (ev.customer?.external_id ?? shortId(ev.event.customer_id)) }}
            </NuxtLink>
          </template>
        </KeyValue>
      </UCard>

      <UTabs
        v-model="tab"
        :items="tabs"
        variant="link"
        class="w-full"
      />
      <div class="mt-4">
        <template v-if="tab === 'decision'">
          <UCard v-if="ev.decision">
            <DecisionBreakdown
              :decision="ev.decision"
              :thresholds="settings?.decision_thresholds"
            />
          </UCard>
          <UEmpty
            v-else
            icon="i-lucide-circle-dashed"
            :title="t('events.noDecision')"
            :description="t('events.noDecisionHelp')"
          />
        </template>
        <UCard v-else-if="tab === 'rules'">
          <RuleTraceTable
            :results="ev.decision?.rule_results ?? []"
            :pid="pid"
          />
        </UCard>
        <UCard v-else-if="tab === 'event'">
          <JsonTree
            :value="ev.event"
            :open="true"
          />
        </UCard>
        <UCard v-else-if="tab === 'source'">
          <p class="text-xs text-muted mb-2">
            {{ t('events.sourceHelp') }}
          </p>
          <JsonTree
            :value="ev.source"
            name="source"
            :open="true"
          />
        </UCard>
        <UCard v-else-if="tab === 'features'">
          <div
            v-if="ev.features"
            class="grid sm:grid-cols-2 lg:grid-cols-3 gap-x-6 gap-y-1 text-sm"
          >
            <div
              v-for="(v, k) in ev.features"
              :key="k"
              class="flex justify-between gap-2 border-b border-default py-1"
            >
              <span class="fp-mono text-xs text-muted">features.{{ k }}</span>
              <span class="tabular-nums">{{ typeof v === 'number' ? fmtNumber(v, 3) : v }}</span>
            </div>
          </div>
          <UEmpty
            v-else
            icon="i-lucide-sigma"
            :title="t('common.empty')"
          />
        </UCard>
        <div
          v-else-if="tab === 'labels'"
          class="grid lg:grid-cols-2 gap-4"
        >
          <UCard>
            <UEmpty
              v-if="!ev.labels.length"
              icon="i-lucide-tag"
              :title="t('events.noLabels')"
              size="sm"
            />
            <ul
              v-else
              class="space-y-2"
            >
              <li
                v-for="l in ev.labels"
                :key="l.id"
                class="flex items-center gap-2 text-sm"
              >
                <StatusBadge :value="l.label" />
                <span v-if="l.fraud_type">{{ t(`typologies.${l.fraud_type}`) }}</span>
                <UBadge
                  color="neutral"
                  variant="outline"
                  size="xs"
                >
                  {{ l.source }}
                </UBadge>
                <span class="ml-auto text-xs text-muted">{{ fmtDate(l.created_at) }}</span>
              </li>
            </ul>
          </UCard>
          <UCard v-if="can('analyst')">
            <template #header>
              <h3 class="font-medium">
                {{ t('events.addLabel') }}
              </h3>
            </template>
            <div class="space-y-3">
              <URadioGroup
                v-model="labelForm.label"
                orientation="horizontal"
                :items="[{ label: t('status.fraud'), value: 'fraud' }, { label: t('status.legit'), value: 'legit' }]"
              />
              <UFormField
                v-if="labelForm.label === 'fraud'"
                :label="t('common.typology')"
              >
                <USelect
                  v-model="labelForm.fraud_type"
                  :items="FRAUD_TYPES.map(f => ({ label: t(`typologies.${f}`), value: f }))"
                  class="w-full"
                />
              </UFormField>
              <UFormField :label="t('common.notes')">
                <UTextarea
                  v-model="labelForm.notes"
                  :rows="2"
                  class="w-full"
                />
              </UFormField>
              <UCheckbox
                v-model="labelForm.apply_to_customer"
                :label="t('events.applyToCustomer')"
                :description="t('events.applyToCustomerHelp')"
              />
              <UButton
                :label="t('actions.save')"
                icon="i-lucide-tag"
                @click="addLabel"
              />
            </div>
          </UCard>
        </div>
      </div>
    </template>
  </PagePanel>
</template>
