<script setup lang="ts">
import type { Page, Proposal } from '#shared/types/api'
import { describeDefinition } from '#shared/rules/model'

const { t } = useI18n()
const api = useApi()
const { pid, base, apiBase } = useProject()
useHead({ title: () => t('nav.proposals') })
const status = ref('pending')
const source = ref(ALL)
const { data, status: loading } = await useAsyncData(`proposals-${pid.value}`, () => api.get<Page<Proposal>>(`${apiBase.value}/proposals`, { query: { status: status.value, source: source.value, page_size: 100 } }), { watch: [status, source] })
</script>

<template>
  <PagePanel
    :title="t('nav.proposals')"
    :description="t('proposals.description')"
  >
    <template #toolbar>
      <UTabs
        v-model="status"
        :items="[{ label: t('status.pending'), value: 'pending' }, { label: t('status.applied'), value: 'applied' }, { label: t('status.rejected'), value: 'rejected' }, { label: t('common.all'), value: '' }]"
        variant="link"
        size="sm"
        :content="false"
      />
      <USelect
        v-model="source"
        :items="[{ label: t('proposals.anySource'), value: ALL }, { label: 'LLM', value: 'llm' }, { label: t('proposals.analyst'), value: 'analyst' }]"
        size="sm"
        class="w-40 ml-auto"
      />
    </template>
    <div
      v-if="loading === 'pending'"
      class="space-y-3"
    >
      <USkeleton
        v-for="i in 3"
        :key="i"
        class="h-24"
      />
    </div>
    <UEmpty
      v-else-if="!data?.items.length"
      icon="i-lucide-git-pull-request"
      :title="t('proposals.empty')"
    />
    <div
      v-else
      class="space-y-3"
    >
      <NuxtLink
        v-for="p in data.items"
        :key="p.id"
        :to="`${base}/proposals/${p.id}`"
        class="block"
      >
        <UCard class="hover:ring-primary/50 transition">
          <div class="flex flex-wrap items-center gap-2">
            <UBadge
              :color="p.source === 'llm' ? 'primary' : 'neutral'"
              variant="soft"
              :icon="p.source === 'llm' ? 'i-lucide-sparkles' : 'i-lucide-user'"
            >
              {{ p.source === 'llm' ? 'LLM' : t('proposals.analyst') }}
            </UBadge>
            <UBadge
              color="neutral"
              variant="outline"
            >
              {{ t(`proposals.types.${p.proposal_type}`) }}
            </UBadge>
            <span class="fp-mono text-sm font-medium">{{ p.definition?.code ?? p.target_rule_code }}</span>
            <span class="text-sm truncate">{{ p.definition?.name }}</span>
            <StatusBadge
              :value="p.status"
              class="ml-auto"
            />
          </div>
          <p class="text-sm text-muted mt-2 line-clamp-2">
            {{ p.rationale }}
          </p>
          <div class="flex flex-wrap gap-3 mt-2 text-xs text-muted">
            <span
              v-if="p.definition"
              class="fp-mono truncate max-w-xl"
            >{{ describeDefinition(p.definition.definition) }}</span>
            <span v-if="p.backtest">{{ t('rules.backtest.precision') }} {{ fmtPct(p.backtest.precision) }} · {{ t('rules.backtest.recall') }} {{ fmtPct(p.backtest.recall) }}</span>
            <span v-if="p.citations.length"><UIcon name="i-lucide-scale" /> {{ p.citations.map(c => `${c.code} ${c.section}`).join(', ') }}</span>
            <span class="ml-auto">{{ fmtRelative(p.created_at) }}</span>
          </div>
        </UCard>
      </NuxtLink>
    </div>
  </PagePanel>
</template>
