<script setup lang="ts">
import type { CaseDetail, CaseStatus, FraudType } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const toast = useToast()
const session = useSessionStore()
const { pid, base, apiBase, can } = useProject()
const id = computed(() => String(route.params.id))
const { data: c, refresh } = await useAsyncData(`case-${id.value}`, () => api.get<CaseDetail>(`${apiBase.value}/cases/${id.value}`))
const cs = computed(() => c.value?.case)
const { members } = useMembers()
useHead({ title: () => `${t('nav.cases')} ${shortId(id.value)}` })

const resolved = computed(() => cs.value?.status === 'resolved_fraud' || cs.value?.status === 'resolved_legit')
const note = ref('')
async function patch(body: Record<string, unknown>) {
  await api.patch(`${apiBase.value}/cases/${id.value}`, body)
  await refresh()
}
async function addNote() {
  if (!note.value.trim()) return
  await patch({ note: note.value.trim() })
  note.value = ''
}

const FRAUD_TYPES: FraudType[] = ['carding', 'account_takeover', 'bank_account_takeover', 'system_breach', 'promo_abuse', 'refund_abuse', 'money_mule', 'other']
const resolveForm = reactive({ label: 'fraud' as 'fraud' | 'legit', fraud_type: 'carding' as FraudType, notes: '', apply_to_customer: true })
const resolveOpen = ref(false)
async function resolve() {
  await api.post(`${apiBase.value}/cases/${id.value}/resolve`, { ...resolveForm, fraud_type: resolveForm.label === 'fraud' ? resolveForm.fraud_type : undefined })
  resolveOpen.value = false
  toast.add({ title: t('cases.resolved'), color: 'success' })
  await refresh()
}
const assignee = computed({
  get: () => cs.value?.assigned_to ?? undefined,
  set: v => patch({ assigned_to: v ?? null }),
})
</script>

<template>
  <PagePanel :title="`${t('nav.cases')} ${shortId(id)}`">
    <template #actions>
      <template v-if="c && can('analyst') && !resolved">
        <UButton
          v-if="c.case.status === 'open'"
          :label="t('cases.startReview')"
          color="neutral"
          variant="outline"
          @click="patch({ status: 'in_review' as CaseStatus })"
        />
        <UButton
          v-if="c.case.assigned_to !== session.user?.id"
          :label="t('cases.assignToMe')"
          color="neutral"
          variant="outline"
          icon="i-lucide-user-check"
          @click="patch({ assigned_to: session.user?.id })"
        />
        <UModal
          v-model:open="resolveOpen"
          :title="t('cases.resolve')"
        >
          <UButton
            :label="t('cases.resolve')"
            icon="i-lucide-check-check"
          />
          <template #body>
            <div class="space-y-3">
              <URadioGroup
                v-model="resolveForm.label"
                orientation="horizontal"
                :items="[{ label: t('status.fraud'), value: 'fraud' }, { label: t('status.legit'), value: 'legit' }]"
              />
              <UFormField
                v-if="resolveForm.label === 'fraud'"
                :label="t('common.typology')"
              >
                <USelect
                  v-model="resolveForm.fraud_type"
                  :items="FRAUD_TYPES.map(f => ({ label: t(`typologies.${f}`), value: f }))"
                  class="w-full"
                />
              </UFormField>
              <UFormField :label="t('common.notes')">
                <UTextarea
                  v-model="resolveForm.notes"
                  :rows="3"
                  class="w-full"
                />
              </UFormField>
              <UCheckbox
                v-model="resolveForm.apply_to_customer"
                :label="t('events.applyToCustomer')"
                :description="t('events.applyToCustomerHelp')"
              />
            </div>
          </template>
          <template #footer>
            <div class="flex justify-end gap-2 w-full">
              <UButton
                :label="t('actions.cancel')"
                color="neutral"
                variant="ghost"
                @click="resolveOpen = false"
              />
              <UButton
                :label="t('cases.resolve')"
                :color="resolveForm.label === 'fraud' ? 'error' : 'success'"
                @click="resolve"
              />
            </div>
          </template>
        </UModal>
      </template>
    </template>

    <div
      v-if="c"
      class="grid xl:grid-cols-3 gap-4"
    >
      <div class="xl:col-span-2 space-y-4">
        <UCard v-if="c.decision">
          <template #header>
            <div class="flex items-center justify-between">
              <h3 class="font-medium">
                {{ t('cases.triggeringEvent') }}
              </h3>
              <UButton
                :to="`${base}/events/${c.decision.event_id}`"
                size="xs"
                variant="link"
                :label="c.decision.external_id"
                trailing-icon="i-lucide-arrow-up-right"
              />
            </div>
          </template>
          <DecisionBreakdown :decision="c.decision" />
        </UCard>
        <UCard v-if="c.decision">
          <template #header>
            <h3 class="font-medium">
              {{ t('events.tabs.rules') }}
            </h3>
          </template>
          <RuleTraceTable
            :results="c.decision.rule_results"
            :pid="pid"
          />
        </UCard>
      </div>
      <div class="space-y-4">
        <UCard>
          <KeyValue
            :items="[
              { key: 'status', label: t('common.status'), value: c.case.status },
              { label: t('common.priority'), value: `P${c.case.priority}` },
              { key: 'customer', label: t('common.customer'), value: c.customer?.external_id ?? c.case.customer_id },
              { label: t('common.created'), value: fmtDate(c.case.created_at) },
              { label: t('cases.resolvedAt'), value: fmtDate(c.case.resolved_at) },
              { label: t('cases.events'), value: c.case.event_ids.length },
            ]"
          >
            <template #status>
              <StatusBadge :value="c.case.status" />
            </template>
            <template #customer>
              <NuxtLink
                :to="`${base}/customers/${c.case.customer_id}`"
                class="text-primary fp-mono text-xs"
              >
                {{ c.customer?.external_id ?? shortId(c.case.customer_id) }}
              </NuxtLink>
            </template>
          </KeyValue>
          <UFormField
            :label="t('cases.assignee')"
            class="mt-3"
          >
            <USelect
              v-model="assignee"
              :items="members.map(m => ({ label: m.full_name, value: m.user_id }))"
              :disabled="!can('analyst') || resolved"
              :placeholder="t('cases.unassigned')"
              class="w-full"
            />
          </UFormField>
        </UCard>
        <UCard v-if="c.graph">
          <template #header>
            <div class="flex items-center justify-between">
              <h3 class="font-medium">
                {{ t('nav.graph') }}
              </h3>
              <UButton
                :to="`${base}/graph?customer=${c.case.customer_id}`"
                size="xs"
                variant="link"
                :label="t('actions.openGraph')"
              />
            </div>
          </template>
          <KeyValue
            :items="[
              { label: t('graph.distanceToFraud'), value: c.graph.distance_to_fraud ?? '∞' },
              { label: t('graph.fraudNeighbors2'), value: c.graph.fraud_neighbors_2 },
              { label: t('graph.componentSize'), value: c.graph.component_size },
              { label: t('graph.sharedEntities'), value: c.graph.shared_entity_count },
            ]"
          />
        </UCard>
        <UCard>
          <template #header>
            <h3 class="font-medium">
              {{ t('common.notes') }}
            </h3>
          </template>
          <ul class="space-y-3 mb-3">
            <li
              v-for="(n, i) in c.case.notes"
              :key="i"
              class="text-sm"
            >
              <p>{{ n.text }}</p>
              <p class="text-xs text-muted">
                {{ n.by }} · {{ fmtDate(n.at) }}
              </p>
            </li>
            <li
              v-if="!c.case.notes.length"
              class="text-sm text-muted"
            >
              {{ t('cases.noNotes') }}
            </li>
          </ul>
          <div
            v-if="can('analyst')"
            class="flex gap-2"
          >
            <UTextarea
              v-model="note"
              :rows="2"
              class="flex-1"
              :placeholder="t('cases.addNote')"
            />
            <UButton
              icon="i-lucide-send"
              :aria-label="t('actions.add')"
              :disabled="!note.trim()"
              @click="addNote"
            />
          </div>
        </UCard>
      </div>
    </div>
  </PagePanel>
</template>
