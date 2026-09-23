<script setup lang="ts">
import type { Page, Rule, Ruleset, RulesetAggregation, RulesetMember } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const toast = useToast()
const { pid, base, apiBase, can } = useProject()
const id = computed(() => String(route.params.id))
const { data: rs, refresh } = await useAsyncData(`ruleset-${id.value}`, () => api.get<Ruleset>(`${apiBase.value}/rulesets/${id.value}`))
const { data: rules } = await useAsyncData<Rule[]>(`rules-all-${pid.value}`, () => api.get<Page<Rule>>(`${apiBase.value}/rules`, { query: { page_size: 200 } }).then(p => p.items), { default: () => [] as Rule[] })
useHead({ title: () => rs.value?.code ?? t('nav.rulesets') })

const AGGS: RulesetAggregation[] = ['probabilistic_or', 'sum', 'max', 'weighted_average']
const editable = computed(() => can('analyst') && rs.value?.status !== 'retired' && rs.value?.status !== 'pending_approval')
const meta = reactive({ name: '', description: '', aggregation: 'probabilistic_or' as RulesetAggregation, max_score: 100, event_types: [] as string[], typologies: [] as string[] })
const members = ref<RulesetMember[]>([])
watch(rs, (r) => {
  if (!r) return
  Object.assign(meta, { name: r.name, description: r.description ?? '', aggregation: r.aggregation, max_score: r.max_score, event_types: [...r.event_types], typologies: [...r.typologies] })
  members.value = r.rules.map(m => ({ ...m }))
}, { immediate: true })

const addRuleId = ref<string>()
const available = computed(() => rules.value.filter(r => r.status !== 'retired' && !members.value.some(m => m.rule_id === r.id)).map(r => ({ label: `${r.code} · ${r.name}`, value: r.id, description: t(`status.${r.status}`) })))
function addMember() {
  const r = rules.value.find(x => x.id === addRuleId.value)
  if (!r) return
  members.value.push({ rule_id: r.id, rule_code: r.code, rule_name: r.name, weight: 1, pinned_version: null })
  addRuleId.value = undefined
}
function versionsOf(ruleId: string) {
  const r = rules.value.find(x => x.id === ruleId)
  return [{ label: t('rulesets.latest'), value: null as number | null }, ...Array.from({ length: r?.current_version ?? 0 }, (_, i) => ({ label: `v${i + 1}`, value: i + 1 as number | null }))]
}
const maxPossible = computed(() => members.value.reduce((s, m) => s + m.weight * (rules.value.find(r => r.id === m.rule_id)?.envelope.risk_score ?? 0), 0))

const saving = ref(false)
async function save() {
  saving.value = true
  try {
    await api.put(`${apiBase.value}/rulesets/${id.value}`, meta)
    await api.put(`${apiBase.value}/rulesets/${id.value}/rules`, members.value.map(m => ({ rule_id: m.rule_id, weight: m.weight, pinned_version: m.pinned_version })))
    toast.add({ title: t('rulesets.saved'), color: 'success' })
    await refresh()
  }
  catch { /* toast */ }
  finally { saving.value = false }
}
const tab = ref('members')
</script>

<template>
  <PagePanel :title="rs ? `${rs.code} · ${rs.name}` : t('nav.rulesets')">
    <template #actions>
      <StatusBadge
        v-if="rs"
        :value="rs.status"
      />
      <ApprovalActions
        v-if="rs"
        :endpoint="`${apiBase}/rulesets/${id}`"
        :status="rs.status"
        @changed="refresh"
      />
      <UButton
        v-if="editable"
        :label="t('actions.save')"
        icon="i-lucide-save"
        :loading="saving"
        @click="save"
      />
    </template>
    <template v-if="rs">
      <UTabs
        v-model="tab"
        :items="[{ label: t('rulesets.members'), value: 'members', icon: 'i-lucide-list' }, { label: t('actions.backtest'), value: 'backtest', icon: 'i-lucide-history' }]"
        variant="link"
        :content="false"
        class="mb-4"
      />
      <div
        v-if="tab === 'members'"
        class="grid xl:grid-cols-[22rem_1fr] gap-4"
      >
        <UCard>
          <div class="space-y-3">
            <UFormField :label="t('common.name')">
              <UInput
                v-model="meta.name"
                class="w-full"
                :disabled="!editable"
              />
            </UFormField>
            <UFormField :label="t('common.description')">
              <UTextarea
                v-model="meta.description"
                :rows="2"
                class="w-full"
                :disabled="!editable"
              />
            </UFormField>
            <UFormField
              :label="t('rulesets.aggregation')"
              :help="t(`rulesets.aggHelp.${meta.aggregation}`)"
            >
              <USelect
                v-model="meta.aggregation"
                :items="AGGS.map(a => ({ label: t(`rulesets.agg.${a}`), value: a }))"
                class="w-full"
                :disabled="!editable"
              />
            </UFormField>
            <UFormField :label="t('rulesets.maxScore')">
              <UInputNumber
                v-model="meta.max_score"
                :min="0"
                :max="100"
                :disabled="!editable"
              />
            </UFormField>
            <UFormField :label="t('common.eventTypes')">
              <UInputTags
                v-model="meta.event_types"
                class="w-full"
                :disabled="!editable"
              />
            </UFormField>
          </div>
        </UCard>
        <UCard>
          <div
            v-if="editable"
            class="flex gap-2 mb-3"
          >
            <USelectMenu
              v-model="addRuleId"
              :items="available"
              value-key="value"
              :placeholder="t('rulesets.addRule')"
              class="flex-1"
            />
            <UButton
              icon="i-lucide-plus"
              :label="t('actions.add')"
              :disabled="!addRuleId"
              @click="addMember"
            />
          </div>
          <UTable
            :data="members"
            :columns="[{ accessorKey: 'rule_code', header: t('common.rule') }, { accessorKey: 'weight', header: t('rules.weight') }, { accessorKey: 'pinned_version', header: t('rulesets.pinnedVersion') }, { id: 'actions', header: '' }]"
            :empty="t('rulesets.noMembers')"
          >
            <template #rule_code-cell="{ row }">
              <NuxtLink
                :to="`${base}/rules/${row.original.rule_id}`"
                class="fp-mono text-xs text-primary"
              >
                {{ row.original.rule_code }}
              </NuxtLink>
              <p class="text-xs text-muted">
                {{ row.original.rule_name ?? rules.find(r => r.id === row.original.rule_id)?.name }}
                <StatusBadge
                  v-if="row.original.rule_status && row.original.rule_status !== 'active'"
                  :value="row.original.rule_status"
                  size="xs"
                />
              </p>
            </template>
            <template #weight-cell="{ row }">
              <UInputNumber
                v-model="members[row.index]!.weight"
                :min="0"
                :max="10"
                :step="0.25"
                size="sm"
                class="w-28"
                :disabled="!editable"
              />
            </template>
            <template #pinned_version-cell="{ row }">
              <ClientOnly>
                <USelect
                  v-model="members[row.index]!.pinned_version"
                  :items="versionsOf(row.original.rule_id)"
                  size="sm"
                  class="w-28"
                  :disabled="!editable"
                />
                <template #fallback>
                  <USkeleton class="h-8 w-40" />
                </template>
              </ClientOnly>
            </template>
            <template #actions-cell="{ row }">
              <UButton
                v-if="editable"
                icon="i-lucide-trash-2"
                size="xs"
                color="neutral"
                variant="ghost"
                :aria-label="t('actions.remove')"
                @click="members.splice(row.index, 1)"
              />
            </template>
          </UTable>
          <p class="text-xs text-muted mt-3">
            {{ t('rulesets.maxPossible', { n: maxPossible.toFixed(1) }) }}
          </p>
        </UCard>
      </div>
      <BacktestPanel
        v-else
        :endpoint="`${apiBase}/rulesets/${id}/backtest`"
      />
    </template>
  </PagePanel>
</template>
