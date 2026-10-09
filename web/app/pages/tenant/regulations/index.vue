<script setup lang="ts">
// Tenant regulation / policy library (OJK, BI, internal SOPs). Uploading a new version with `supersedes` makes the
// llm-service diff the sections and enables a regulation-impact analysis per project.
import type { Page, Regulation } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const toast = useToast()
const session = useSessionStore()
useHead({ title: () => t('nav.regulationLibrary') })
const tid = computed(() => session.tenant?.id)
const { data, refresh } = await useAsyncData<Regulation[]>(`reg-library-${tid.value}`, () => (tid.value ? api.get<Page<Regulation>>(`/tenants/${tid.value}/regulations`, { query: { page_size: 200 } }).then(p => p.items) : Promise.resolve([])), { default: () => [] as Regulation[] })
const canUpload = computed(() => session.isTenantAdmin || session.projects.some(p => ['analyst', 'approver', 'project_admin'].includes(p.role)))

let timer: ReturnType<typeof setInterval> | undefined
watch(() => data.value.some(r => r.status === 'processing'), (p) => {
  clearInterval(timer)
  if (p && import.meta.client) timer = setInterval(() => refresh(), 3000)
}, { immediate: true })
onBeforeUnmount(() => clearInterval(timer))

const open = ref(false)
const file = ref<File | null>(null)
const form = reactive({ code: '', title: '', doc_type: 'regulation', issuer: 'OJK', effective_date: '', supersedes_id: ALL as string })
watch(() => form.supersedes_id, (id) => {
  const prev = data.value.find(r => r.id === id)
  if (prev) { form.code = prev.code; form.issuer = prev.issuer; form.doc_type = prev.doc_type }
})
const busy = ref(false)
async function upload() {
  if (!file.value || !tid.value) return
  busy.value = true
  try {
    const fd = new FormData()
    fd.append('file', file.value)
    for (const [k, v] of Object.entries(form)) if (v && v !== ALL) fd.append(k, v)
    await api.upload(`/tenants/${tid.value}/regulations`, fd)
    toast.add({ title: t('llm.uploaded'), description: t('llm.uploadedHelp'), color: 'success' })
    open.value = false
    file.value = null
    await refresh()
  }
  catch { /* toast */ }
  finally { busy.value = false }
}
</script>

<template>
  <PagePanel
    :title="t('nav.regulationLibrary')"
    :description="t('llm.libraryDescription')"
  >
    <template #actions>
      <UModal
        v-if="canUpload"
        v-model:open="open"
        :title="t('llm.uploadRegulation')"
      >
        <UButton
          icon="i-lucide-upload"
          :label="t('llm.uploadRegulation')"
        />
        <template #body>
          <div class="space-y-3">
            <UFileUpload
              v-model="file"
              accept=".pdf,.docx,.txt,.md"
              :label="t('llm.dropFile')"
              :description="t('llm.fileTypes')"
              class="w-full min-h-32"
            />
            <UFormField
              :label="t('llm.supersedes')"
              :help="t('llm.supersedesHelp')"
            >
              <USelect
                v-model="form.supersedes_id"
                :items="[{ label: t('llm.newDocument'), value: ALL }, ...data.filter(r => r.status !== 'superseded').map(r => ({ label: `${r.code} v${r.version}`, value: r.id }))]"
                class="w-full"
              />
            </UFormField>
            <div class="grid grid-cols-2 gap-3">
              <UFormField
                :label="t('common.code')"
                required
              >
                <UInput
                  v-model="form.code"
                  class="w-full fp-mono"
                  placeholder="POJK-12-2024"
                />
              </UFormField>
              <UFormField
                :label="t('llm.issuer')"
                required
              >
                <UInput
                  v-model="form.issuer"
                  class="w-full"
                />
              </UFormField>
              <UFormField :label="t('common.type')">
                <USelect
                  v-model="form.doc_type"
                  :items="['regulation', 'internal_policy', 'sop', 'other'].map(d => ({ label: t(`llm.docTypes.${d}`), value: d }))"
                  class="w-full"
                />
              </UFormField>
              <UFormField :label="t('llm.effectiveDate')">
                <UInput
                  v-model="form.effective_date"
                  type="date"
                  class="w-full"
                />
              </UFormField>
            </div>
            <UFormField
              :label="t('common.title')"
              required
            >
              <UInput
                v-model="form.title"
                class="w-full"
              />
            </UFormField>
          </div>
        </template>
        <template #footer>
          <UButton
            :label="t('actions.upload')"
            icon="i-lucide-upload"
            :loading="busy"
            :disabled="!file || !form.code || !form.title"
            @click="upload"
          />
        </template>
      </UModal>
    </template>
    <UTable
      :data="data"
      :columns="[{ accessorKey: 'code', header: t('common.code') }, { accessorKey: 'title', header: t('common.title') }, { accessorKey: 'doc_type', header: t('common.type') }, { accessorKey: 'effective_date', header: t('llm.effectiveDate') }, { accessorKey: 'chunk_count', header: t('llm.chunks') }, { accessorKey: 'status', header: t('common.status') }]"
      class="cursor-pointer"
      :empty="t('common.empty')"
      @select="(_, row) => navigateTo(`/tenant/regulations/${row.original.id}`)"
    >
      <template #code-cell="{ row }">
        <span class="fp-mono text-xs">{{ row.original.code }}</span> <span class="text-xs text-muted">v{{ row.original.version }}</span>
        <UBadge
          v-if="row.original.supersedes_id"
          color="info"
          variant="soft"
          size="xs"
          class="ml-1"
        >
          {{ t('llm.hasChanges') }}
        </UBadge>
      </template>
      <template #doc_type-cell="{ row }">
        {{ t(`llm.docTypes.${row.original.doc_type}`) }} · <span class="text-muted">{{ row.original.issuer }}</span>
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
