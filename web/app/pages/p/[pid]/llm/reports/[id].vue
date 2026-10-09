<script setup lang="ts">
import type { LlmReport, Page, Proposal } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const { base, apiBase } = useProject()
const id = computed(() => String(route.params.id))
const { data: report, refresh } = await useAsyncData(`report-${id.value}`, () => api.get<LlmReport>(`${apiBase.value}/llm/reports/${id.value}`))
const { data: proposals } = await useAsyncData<Proposal[]>(`report-proposals-${id.value}`, () => api.get<Page<Proposal>>(`${apiBase.value}/proposals`, { query: { report_id: id.value }, silent: true }).then(p => p.items.filter(x => x.report_id === id.value)).catch(() => []), { default: () => [] as Proposal[] })
useHead({ title: () => report.value?.title ?? t('nav.llmReports') })
let timer: ReturnType<typeof setInterval> | undefined
watch(() => report.value?.status, (s) => {
  clearInterval(timer)
  if (s === 'running' && import.meta.client) timer = setInterval(() => refresh(), 2500)
}, { immediate: true })
onBeforeUnmount(() => clearInterval(timer))
const showJson = ref(false)
</script>

<template>
  <PagePanel :title="report?.title ?? t('nav.llmReports')">
    <template #actions>
      <StatusBadge
        v-if="report"
        :value="report.status"
      />
      <UButton
        v-if="report?.structured"
        icon="i-lucide-download"
        color="neutral"
        variant="ghost"
        :aria-label="t('actions.download')"
        @click="downloadJson(`report-${id}.json`, report.structured)"
      />
    </template>
    <div
      v-if="report"
      class="grid xl:grid-cols-3 gap-4"
    >
      <UCard class="xl:col-span-2">
        <div
          v-if="report.status === 'running'"
          class="flex items-center gap-2 text-sm text-muted"
        >
          <UIcon
            name="i-lucide-loader-circle"
            class="animate-spin"
          /> {{ t('llm.generating') }}
        </div>
        <UAlert
          v-else-if="report.status === 'failed'"
          color="error"
          variant="subtle"
          :description="report.error ?? t('errors.generic')"
        />
        <MarkdownView
          v-else
          :source="report.content_md"
        />
        <template v-if="report.structured">
          <UButton
            size="xs"
            variant="link"
            class="px-0 mt-3"
            :label="showJson ? t('llm.hideStructured') : t('llm.showStructured')"
            @click="showJson = !showJson"
          />
          <JsonTree
            v-if="showJson"
            :value="report.structured"
            :open="true"
          />
        </template>
      </UCard>
      <div class="space-y-4">
        <UCard>
          <KeyValue :items="[{ label: t('common.type'), value: t(`llm.reportTypes.${report.report_type}`) }, { label: t('llm.model'), value: report.model, mono: true }, { label: t('common.created'), value: fmtDate(report.created_at) }, { label: t('llm.finished'), value: fmtDate(report.finished_at) }]" />
          <p class="text-xs text-muted uppercase mt-3 mb-1">
            {{ t('llm.params') }}
          </p>
          <JsonTree
            :value="report.params"
            :open="true"
          />
        </UCard>
        <UCard v-if="proposals.length">
          <template #header>
            <h3 class="font-medium">
              {{ t('nav.proposals') }}
            </h3>
          </template>
          <ul class="space-y-2">
            <li
              v-for="p in proposals"
              :key="p.id"
              class="flex items-center gap-2 text-sm"
            >
              <NuxtLink
                :to="`${base}/proposals/${p.id}`"
                class="text-primary fp-mono text-xs"
              >
                {{ p.definition?.code ?? p.target_rule_code }}
              </NuxtLink>
              <span class="text-xs text-muted">{{ t(`proposals.types.${p.proposal_type}`) }}</span>
              <StatusBadge
                :value="p.status"
                size="xs"
                class="ml-auto"
              />
            </li>
          </ul>
        </UCard>
      </div>
    </div>
  </PagePanel>
</template>
