<script setup lang="ts">
import type { Page, Ruleset, RulesetAggregation } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const toast = useToast()
const { pid, base, apiBase, can } = useProject()
useHead({ title: () => t('nav.rulesets') })

const { data, status } = await useAsyncData(`rulesets-${pid.value}`, () => api.get<Page<Ruleset>>(`${apiBase.value}/rulesets`, { query: { page_size: 200 } }))
const AGGS: RulesetAggregation[] = ['probabilistic_or', 'sum', 'max', 'weighted_average']
const open = ref(false)
const form = reactive({ code: '', name: '', description: '', aggregation: 'probabilistic_or' as RulesetAggregation, max_score: 100, event_types: ['transaction'] as string[] })
async function create() {
  const rs = await api.post<Ruleset>(`${apiBase.value}/rulesets`, form)
  open.value = false
  toast.add({ title: t('rulesets.created'), color: 'success' })
  await navigateTo(`${base.value}/rulesets/${rs.id}`)
}
</script>

<template>
  <PagePanel
    :title="t('nav.rulesets')"
    :description="t('rulesets.description')"
  >
    <template #actions>
      <UModal
        v-if="can('analyst')"
        v-model:open="open"
        :title="t('actions.newRuleset')"
      >
        <UButton
          icon="i-lucide-plus"
          :label="t('actions.newRuleset')"
        />
        <template #body>
          <div class="space-y-3">
            <UFormField
              :label="t('common.code')"
              required
              :help="t('rules.codeHelp')"
            >
              <UInput
                v-model="form.code"
                class="w-full fp-mono uppercase"
                placeholder="RS-CARDING"
              />
            </UFormField>
            <UFormField
              :label="t('common.name')"
              required
            >
              <UInput
                v-model="form.name"
                class="w-full"
              />
            </UFormField>
            <UFormField :label="t('common.description')">
              <UInput
                v-model="form.description"
                class="w-full"
              />
            </UFormField>
            <UFormField
              :label="t('rulesets.aggregation')"
              :help="t(`rulesets.aggHelp.${form.aggregation}`)"
            >
              <USelect
                v-model="form.aggregation"
                :items="AGGS.map(a => ({ label: t(`rulesets.agg.${a}`), value: a }))"
                class="w-full"
              />
            </UFormField>
            <UFormField :label="t('common.eventTypes')">
              <UInputTags
                v-model="form.event_types"
                class="w-full"
              />
            </UFormField>
          </div>
        </template>
        <template #footer>
          <UButton
            :label="t('actions.create')"
            :disabled="!/^[A-Z0-9-]{3,40}$/.test(form.code) || !form.name"
            @click="create"
          />
        </template>
      </UModal>
    </template>
    <UTable
      :data="data?.items ?? []"
      :loading="status === 'pending'"
      :columns="[{ accessorKey: 'code', header: t('common.code') }, { accessorKey: 'name', header: t('common.name') }, { accessorKey: 'aggregation', header: t('rulesets.aggregation') }, { accessorKey: 'rules', header: t('nav.rules') }, { accessorKey: 'event_types', header: t('common.eventTypes') }, { accessorKey: 'status', header: t('common.status') }]"
      class="cursor-pointer"
      :empty="t('common.empty')"
      @select="(_, row) => navigateTo(`${base}/rulesets/${row.original.id}`)"
    >
      <template #code-cell="{ row }">
        <span class="fp-mono text-xs">{{ row.original.code }}</span> <span class="text-xs text-muted">v{{ row.original.version }}</span>
      </template>
      <template #aggregation-cell="{ row }">
        {{ t(`rulesets.agg.${row.original.aggregation}`) }}
      </template>
      <template #rules-cell="{ row }">
        {{ row.original.rules.length }}
      </template>
      <template #event_types-cell="{ row }">
        <span class="text-xs">{{ row.original.event_types.join(', ') || t('common.all') }}</span>
      </template>
      <template #status-cell="{ row }">
        <StatusBadge
          :value="row.original.status"
          size="xs"
        />
      </template>
    </UTable>
  </PagePanel>
</template>
