<script setup lang="ts">
import type { Regulation } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const session = useSessionStore()
const id = computed(() => String(route.params.id))
const { data: reg } = await useAsyncData(`regulation-${id.value}`, () => api.get<Regulation>(`/tenants/${session.tenant?.id}/regulations/${id.value}`))
useHead({ title: () => reg.value?.code ?? t('nav.regulationLibrary') })
</script>

<template>
  <PagePanel :title="reg ? `${reg.code} v${reg.version}` : t('nav.regulationLibrary')">
    <template #actions>
      <StatusBadge
        v-if="reg"
        :value="reg.status"
      />
    </template>
    <div
      v-if="reg"
      class="grid xl:grid-cols-3 gap-4"
    >
      <UCard>
        <KeyValue :items="[{ label: t('common.title'), value: reg.title }, { label: t('llm.issuer'), value: reg.issuer }, { label: t('common.type'), value: t(`llm.docTypes.${reg.doc_type}`) }, { label: t('llm.effectiveDate'), value: reg.effective_date }, { label: t('llm.file'), value: reg.file_name, mono: true }, { label: t('llm.chunks'), value: reg.chunk_count }, { label: t('common.created'), value: fmtDate(reg.created_at) }]" />
        <UAlert
          v-if="reg.error"
          color="error"
          variant="subtle"
          :description="reg.error"
          class="mt-3"
        />
        <UButton
          v-if="reg.supersedes_id"
          :to="`/tenant/regulations/${reg.supersedes_id}`"
          variant="link"
          class="px-0 mt-2"
          :label="t('llm.previousVersion')"
          icon="i-lucide-history"
        />
      </UCard>
      <div class="xl:col-span-2 space-y-4">
        <UCard v-if="reg.summary">
          <template #header>
            <h3 class="font-medium">
              {{ t('llm.summary') }}
            </h3>
          </template>
          <MarkdownView :source="reg.summary" />
        </UCard>
        <UCard v-if="reg.changes">
          <template #header>
            <h3 class="font-medium">
              {{ t('llm.changesVsPrevious') }}
            </h3>
          </template>
          <p
            v-if="reg.changes.diff_summary"
            class="text-sm mb-3"
          >
            {{ reg.changes.diff_summary }}
          </p>
          <div class="space-y-3">
            <div
              v-for="(c, i) in reg.changes.changed_sections"
              :key="i"
              class="rounded-md border border-default p-3"
            >
              <div class="flex items-center gap-2 mb-2">
                <UBadge
                  :color="c.change === 'added' ? 'success' : c.change === 'removed' ? 'error' : 'warning'"
                  variant="soft"
                  size="sm"
                >
                  {{ t(`llm.change.${c.change}`) }}
                </UBadge>
                <span class="font-medium text-sm">{{ c.section }}</span>
              </div>
              <p
                v-if="c.before"
                class="text-sm bg-rose-500/10 rounded p-2 line-through decoration-rose-400/60"
              >
                {{ c.before }}
              </p>
              <p
                v-if="c.after"
                class="text-sm bg-emerald-500/10 rounded p-2 mt-1"
              >
                {{ c.after }}
              </p>
            </div>
          </div>
          <p class="text-xs text-muted mt-3">
            {{ t('llm.impactHint') }}
          </p>
        </UCard>
      </div>
    </div>
  </PagePanel>
</template>
