<script setup lang="ts">
// Which regulations / internal policies from the tenant library this project follows (used by RAG and analyses).
import type { Items, Page, Regulation, RegulationSearchHit } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const toast = useToast()
const session = useSessionStore()
const { pid, apiBase, can } = useProject()
useHead({ title: () => t('nav.llmRegulations') })

const tid = computed(() => session.tenant?.id)
const { data: library } = await useAsyncData<Regulation[]>(`reg-library-${tid.value}`, () => (tid.value ? api.get<Page<Regulation>>(`/tenants/${tid.value}/regulations`, { query: { page_size: 200 } }).then(p => p.items) : Promise.resolve([])), { default: () => [] as Regulation[] })
const { data: attached, refresh } = await useAsyncData<string[]>(`reg-attached-${pid.value}`, () => api.get<{ items: Regulation[], regulation_ids: string[] }>(`${apiBase.value}/llm/regulations`).then(r => r.regulation_ids), { default: () => [] as string[] })
const selection = ref<string[]>([...attached.value])
watch(attached, a => (selection.value = [...a]))
const dirty = computed(() => [...selection.value].sort().join() !== [...attached.value].sort().join())
async function save() {
  await api.put(`${apiBase.value}/llm/regulations`, { regulation_ids: selection.value })
  toast.add({ title: t('llm.attachmentsSaved'), color: 'success' })
  await refresh()
}
function toggle(id: string, on: boolean) {
  selection.value = on ? [...selection.value, id] : selection.value.filter(x => x !== id)
}

const query = ref('')
const hits = ref<RegulationSearchHit[]>([])
const searching = ref(false)
async function search() {
  if (!query.value.trim()) return
  searching.value = true
  try { hits.value = await api.post<Items<RegulationSearchHit>>(`${apiBase.value}/llm/regulations/search`, { query: query.value, k: 8 }).then(r => r.items) }
  finally { searching.value = false }
}
</script>

<template>
  <PagePanel
    :title="t('nav.llmRegulations')"
    :description="t('llm.regulationsDescription')"
  >
    <template #actions>
      <UButton
        to="/tenant/regulations"
        icon="i-lucide-library"
        color="neutral"
        variant="outline"
        :label="t('nav.regulationLibrary')"
      />
      <UButton
        v-if="can('project_admin')"
        icon="i-lucide-save"
        :label="t('actions.save')"
        :disabled="!dirty"
        @click="save"
      />
    </template>
    <div class="grid xl:grid-cols-2 gap-4">
      <UCard>
        <template #header>
          <h3 class="font-medium">
            {{ t('llm.attached') }}
          </h3>
        </template>
        <ul class="divide-y divide-default">
          <li
            v-for="r in library"
            :key="r.id"
            class="py-2 flex items-start gap-3"
          >
            <UCheckbox
              :model-value="selection.includes(r.id)"
              :disabled="!can('project_admin') || r.status !== 'indexed'"
              :aria-label="r.code"
              @update:model-value="(v: boolean | 'indeterminate') => toggle(r.id, v === true)"
            />
            <div class="min-w-0">
              <p class="text-sm font-medium">
                {{ r.code }} <span class="text-muted">v{{ r.version }}</span> · {{ r.title }}
              </p>
              <p class="text-xs text-muted">
                {{ r.issuer }} · {{ t(`llm.docTypes.${r.doc_type}`) }} · {{ r.effective_date ?? '—' }}
              </p>
            </div>
            <StatusBadge
              :value="r.status"
              size="xs"
              class="ml-auto"
            />
          </li>
        </ul>
      </UCard>
      <UCard>
        <template #header>
          <h3 class="font-medium">
            {{ t('llm.searchRegulations') }}
          </h3>
        </template>
        <form
          class="flex gap-2 mb-3"
          @submit.prevent="search"
        >
          <UInput
            v-model="query"
            class="flex-1"
            :placeholder="t('llm.searchPlaceholder')"
            icon="i-lucide-search"
          />
          <UButton
            type="submit"
            :loading="searching"
            :label="t('actions.search')"
          />
        </form>
        <ul class="space-y-3">
          <li
            v-for="(h, i) in hits"
            :key="i"
            class="text-sm"
          >
            <p class="font-medium">
              {{ h.code }} · {{ h.section }} <span class="text-xs text-muted tabular-nums">({{ h.score.toFixed(2) }})</span>
            </p>
            <p class="text-muted">
              {{ h.excerpt }}
            </p>
          </li>
        </ul>
      </UCard>
    </div>
  </PagePanel>
</template>
