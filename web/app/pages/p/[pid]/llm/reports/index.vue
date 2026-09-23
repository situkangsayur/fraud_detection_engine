<script setup lang="ts">
import type { LlmReport, Page, Regulation, ReportType, Typology } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const toast = useToast()
const session = useSessionStore()
const { pid, base, apiBase, can } = useProject()
useHead({ title: () => t('nav.llmReports') })
const { data, refresh } = await useAsyncData(`reports-${pid.value}`, () => api.get<Page<LlmReport>>(`${apiBase.value}/llm/reports`, { query: { page_size: 100 } }))
const { data: regs } = await useAsyncData<Regulation[]>(`reg-library-${session.tenant?.id}`, () => (session.tenant ? api.get<Page<Regulation>>(`/tenants/${session.tenant.id}/regulations`, { query: { page_size: 200 }, silent: true }).then(p => p.items).catch(() => []) : Promise.resolve([])), { default: () => [] as Regulation[] })

let timer: ReturnType<typeof setInterval> | undefined
watch(() => data.value?.items.some(r => r.status === 'running'), (running) => {
  clearInterval(timer)
  if (running && import.meta.client) timer = setInterval(() => refresh(), 3000)
}, { immediate: true })
onBeforeUnmount(() => clearInterval(timer))

const TYPES: { type: ReportType, path: string, icon: string }[] = [
  { type: 'fraud_situation', path: 'fraud-situation', icon: 'i-lucide-radar' },
  { type: 'rule_relevance', path: 'rule-relevance', icon: 'i-lucide-list-checks' },
  { type: 'regulation_impact', path: 'regulation-impact', icon: 'i-lucide-scale' },
  { type: 'recommend_rules', path: 'recommend-rules', icon: 'i-lucide-wand-sparkles' },
]
const TYPOLOGIES: Typology[] = ['carding', 'account_takeover', 'bank_account_takeover', 'system_breach', 'promo_abuse', 'refund_abuse', 'money_mule']
const form = reactive({ since_days: 30, regulation_id: '' as string, focus: ALL as Typology | typeof ALL, max_rules: 5 })
const launching = ref<ReportType | null>(null)
async function launch(tp: typeof TYPES[number]) {
  launching.value = tp.type
  try {
    const body = tp.type === 'regulation_impact' ? { regulation_id: form.regulation_id } : tp.type === 'recommend_rules' ? { since_days: form.since_days, focus: unlessAll(form.focus), max_rules: form.max_rules } : { since_days: form.since_days }
    const res = await api.post<{ report_id: string }>(`${apiBase.value}/llm/analysis/${tp.path}`, body)
    toast.add({ title: t('llm.analysisStarted'), color: 'success' })
    await refresh()
    await navigateTo(`${base.value}/llm/reports/${res.report_id}`)
  }
  catch { /* toast */ }
  finally { launching.value = null }
}
</script>

<template>
  <PagePanel
    :title="t('nav.llmReports')"
    :description="t('llm.reportsDescription')"
  >
    <UCard
      v-if="can('analyst')"
      class="mb-4"
    >
      <template #header>
        <h3 class="font-medium">
          {{ t('llm.runAnalysis') }}
        </h3>
      </template>
      <div class="flex flex-wrap gap-3 mb-4">
        <UFormField :label="t('ml.sinceDays')">
          <UInputNumber
            v-model="form.since_days"
            :min="1"
            :max="365"
            class="w-28"
          />
        </UFormField>
        <UFormField :label="t('llm.regulation')">
          <USelect
            v-model="form.regulation_id"
            :items="regs.map(r => ({ label: `${r.code} v${r.version}`, value: r.id }))"
            class="w-56"
            :placeholder="t('llm.pickRegulation')"
          />
        </UFormField>
        <UFormField :label="t('llm.focusTypology')">
          <USelect
            v-model="form.focus"
            :items="[{ label: t('common.all'), value: ALL }, ...TYPOLOGIES.map(x => ({ label: t(`typologies.${x}`), value: x }))]"
            class="w-52"
          />
        </UFormField>
        <UFormField :label="t('llm.maxRules')">
          <UInputNumber
            v-model="form.max_rules"
            :min="1"
            :max="20"
            class="w-24"
          />
        </UFormField>
      </div>
      <div class="grid sm:grid-cols-2 xl:grid-cols-4 gap-3">
        <UButton
          v-for="tp in TYPES"
          :key="tp.type"
          :icon="tp.icon"
          color="neutral"
          variant="outline"
          class="h-auto py-3 flex-col items-start text-left whitespace-normal"
          :loading="launching === tp.type"
          :disabled="tp.type === 'regulation_impact' && !form.regulation_id"
          @click="launch(tp)"
        >
          <span class="font-medium">{{ t(`llm.reportTypes.${tp.type}`) }}</span>
          <span class="text-xs text-muted">{{ t(`llm.reportHelp.${tp.type}`) }}</span>
        </UButton>
      </div>
    </UCard>
    <UTable
      :data="data?.items ?? []"
      :columns="[{ accessorKey: 'title', header: t('common.title') }, { accessorKey: 'report_type', header: t('common.type') }, { accessorKey: 'status', header: t('common.status') }, { accessorKey: 'model', header: t('llm.model') }, { accessorKey: 'created_at', header: t('common.created') }]"
      class="cursor-pointer"
      :empty="t('llm.noReports')"
      @select="(_, row) => navigateTo(`${base}/llm/reports/${row.original.id}`)"
    >
      <template #report_type-cell="{ row }">
        {{ t(`llm.reportTypes.${row.original.report_type}`) }}
      </template>
      <template #status-cell="{ row }">
        <StatusBadge
          :value="row.original.status"
          size="xs"
        />
      </template>
      <template #model-cell="{ row }">
        <span class="fp-mono text-xs">{{ row.original.model }}</span>
      </template>
      <template #created_at-cell="{ row }">
        <span class="text-xs">{{ fmtRelative(row.original.created_at) }}</span>
      </template>
    </UTable>
  </PagePanel>
</template>
