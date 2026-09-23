<script setup lang="ts">
import type { DataSource, IngestError, IngestJob, InspectResult, Items, MappingPreviewRow, MappingSpec, MappingVersion, Page } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const toast = useToast()
const { apiBase, can } = useProject()
const id = computed(() => String(route.params.id))
const srcPath = computed(() => `${apiBase.value}/data-sources/${id.value}`)
const { data: ds, refresh: refreshDs } = await useAsyncData(`source-${id.value}`, () => api.get<DataSource>(srcPath.value))
const { data: mappings, refresh: refreshMappings } = await useAsyncData<MappingVersion[]>(`mappings-${id.value}`, () => api.get<Items<MappingVersion>>(`${srcPath.value}/mappings`).then(r => r.items), { default: () => [] as MappingVersion[] })
useHead({ title: () => ds.value?.name ?? t('nav.dataSources') })

const tab = ref(String(route.query.tab ?? 'mapping'))

// ---- inspect
const inspectMode = ref<'file' | 'json' | 'sql'>(ds.value?.kind === 'file' ? 'file' : ds.value?.kind === 'postgres' || ds.value?.kind === 'mysql' ? 'sql' : 'json')
const file = ref<File | null>(null)
const sampleJson = ref('[\n  {"trx_id": "T-1", "created": "23/09/2026 10:00:00", "total": 150000, "user": {"id": "U-1", "hp": "081234567890"}}\n]')
const sqlQuery = ref('')
const inspect = ref<InspectResult | null>(null)
const inspecting = ref(false)
async function runInspect() {
  inspecting.value = true
  try {
    if (inspectMode.value === 'file') {
      if (!file.value) return
      const fd = new FormData()
      fd.append('file', file.value)
      inspect.value = await api.upload<InspectResult>(`${srcPath.value}/inspect`, fd)
    }
    else if (inspectMode.value === 'json') {
      inspect.value = await api.post<InspectResult>(`${srcPath.value}/inspect`, { records: JSON.parse(sampleJson.value) })
    }
    else {
      inspect.value = await api.post<InspectResult>(`${srcPath.value}/inspect`, { sql: sqlQuery.value.trim().toLowerCase().startsWith('select') ? { query: sqlQuery.value } : { table: sqlQuery.value || undefined, limit: 1000 } })
    }
    draft.value = structuredClone(inspect.value.suggested_mapping)
    toast.add({ title: t('datasources.inspected', { n: inspect.value.schema.fields.length }), color: 'success' })
  }
  catch (err) {
    if (err instanceof SyntaxError) toast.add({ title: t('errors.invalidJson'), color: 'error' })
  }
  finally { inspecting.value = false }
}

// ---- mapping draft
const active = computed(() => mappings.value.find(m => m.status === 'active'))
const draft = ref<MappingSpec>(structuredClone(active.value?.mapping ?? mappings.value.at(-1)?.mapping ?? { event: {}, customer: {}, drop_fields: [] }))
const fields = computed(() => inspect.value?.schema.fields ?? ds.value?.inferred_schema?.fields ?? [])
const previewRows = computed(() => inspect.value?.preview ?? [])
const preview = ref<MappingPreviewRow[] | null>(null)
async function runPreview() {
  preview.value = await api.post<Items<MappingPreviewRow>>(`${srcPath.value}/mappings/preview`, { mapping: draft.value, records: previewRows.value.length ? previewRows.value : JSON.parse(sampleJson.value) }).then(r => r.items)
}
const saving = ref(false)
async function saveDraft(activate: boolean) {
  saving.value = true
  try {
    const v = await api.post<MappingVersion>(`${srcPath.value}/mappings`, { mapping: draft.value })
    if (activate) await api.post(`${srcPath.value}/mappings/${v.version}/activate`)
    toast.add({ title: activate ? t('datasources.mappingActivated', { v: v.version }) : t('datasources.mappingSaved', { v: v.version }), color: 'success' })
    await Promise.all([refreshMappings(), refreshDs()])
  }
  catch { /* toast */ }
  finally { saving.value = false }
}
function loadVersion(m: MappingVersion) {
  draft.value = structuredClone(toRaw(m.mapping))
  tab.value = 'mapping'
}
async function activateVersion(v: number) {
  await api.post(`${srcPath.value}/mappings/${v}/activate`)
  await Promise.all([refreshMappings(), refreshDs()])
}

// ---- jobs
const { data: jobs, refresh: refreshJobs } = await useAsyncData<IngestJob[]>(`jobs-${id.value}`, () => api.get<Page<IngestJob>>(`${srcPath.value}/jobs`, { query: { page_size: 50 }, silent: true }).then(p => p.items).catch(() => []), { default: () => [] as IngestJob[] })
const jobMode = ref<'score' | 'load_only'>(ds.value?.mode ?? 'score')
async function startJob() {
  const res = await api.post<{ job_id: string }>(`${srcPath.value}/jobs`, { upload_id: inspect.value?.upload_id ?? undefined, mode: jobMode.value })
  toast.add({ title: t('datasources.jobStarted'), color: 'success' })
  await refreshJobs()
  tab.value = 'jobs'
  return res
}
let timer: ReturnType<typeof setInterval> | undefined
watch(() => jobs.value.some(j => j.status === 'running' || j.status === 'queued'), (running) => {
  clearInterval(timer)
  if (running && import.meta.client) timer = setInterval(async () => {
    for (const j of jobs.value.filter(x => x.status === 'running')) await api.get(`${apiBase.value}/ingest-jobs/${j.id}`, { silent: true }).catch(() => null)
    await refreshJobs()
  }, 2500)
}, { immediate: true })
onBeforeUnmount(() => clearInterval(timer))
async function cancelJob(j: IngestJob) {
  await api.post(`${apiBase.value}/ingest-jobs/${j.id}/cancel`)
  await refreshJobs()
}

// ---- errors
const errPage = ref(1)
const { data: errors } = await useAsyncData(`ingest-errors-${id.value}`, () => api.get<Page<IngestError>>(`${srcPath.value}/errors`, { query: { page: errPage.value, page_size: 50 }, silent: true }).catch(() => null), { watch: [errPage] })

async function rotateKey() {
  const res = await api.post<DataSource>(`${srcPath.value}/rotate-key`)
  newKey.value = res.api_key ?? null
}
const newKey = ref<string | null>(null)
</script>

<template>
  <PagePanel :title="ds?.name ?? t('nav.dataSources')">
    <template #actions>
      <UBadge
        v-if="ds"
        color="neutral"
        variant="soft"
      >
        {{ t(`datasources.kinds.${ds.kind}`) }}
      </UBadge>
      <ConfirmAction
        v-if="ds?.kind === 'webhook' && can('project_admin')"
        :label="t('actions.rotateKey')"
        icon="i-lucide-key-round"
        color="warning"
        :description="t('datasources.rotateHelp')"
        :action="rotateKey"
      />
    </template>
    <UAlert
      v-if="newKey"
      color="warning"
      variant="subtle"
      icon="i-lucide-key-round"
      :title="t('datasources.apiKeyOnce')"
      :description="newKey"
      class="mb-4 fp-mono"
    />

    <UTabs
      v-model="tab"
      :items="[
        { label: t('datasources.tabs.mapping'), value: 'mapping', icon: 'i-lucide-arrow-right-left' },
        { label: t('datasources.tabs.versions', { n: mappings.length }), value: 'versions', icon: 'i-lucide-git-branch' },
        { label: t('datasources.tabs.jobs'), value: 'jobs', icon: 'i-lucide-loader' },
        { label: t('datasources.tabs.errors'), value: 'errors', icon: 'i-lucide-circle-alert' },
        { label: t('datasources.tabs.settings'), value: 'settings', icon: 'i-lucide-settings' },
      ]"
      variant="link"
      :content="false"
      class="mb-4"
    />

    <div
      v-if="tab === 'mapping'"
      class="space-y-4"
    >
      <UCard v-if="can('analyst')">
        <template #header>
          <div class="flex items-center gap-2">
            <span class="rounded-full bg-primary text-inverted size-6 grid place-items-center text-xs">1</span>
            <h3 class="font-medium">
              {{ t('datasources.step1') }}
            </h3>
          </div>
        </template>
        <div class="space-y-3">
          <UTabs
            v-model="inspectMode"
            :items="[{ label: t('datasources.inspectFile'), value: 'file' }, { label: t('datasources.inspectJson'), value: 'json' }, { label: t('datasources.inspectSql'), value: 'sql' }]"
            :content="false"
            size="sm"
          />
          <UFileUpload
            v-if="inspectMode === 'file'"
            v-model="file"
            accept=".csv,.tsv,.json,.jsonl,.ndjson,.parquet,.xlsx"
            :label="t('datasources.dropDataset')"
            :description="t('datasources.datasetFormats')"
            class="w-full min-h-28"
          />
          <CodeEditor
            v-else-if="inspectMode === 'json'"
            v-model="sampleJson"
            height="180px"
          />
          <UInput
            v-else
            v-model="sqlQuery"
            class="w-full fp-mono"
            :placeholder="t('datasources.sqlPlaceholder')"
          />
          <UButton
            :label="t('datasources.inspect')"
            icon="i-lucide-scan-search"
            :loading="inspecting"
            :disabled="inspectMode === 'file' && !file"
            @click="runInspect"
          />
        </div>
      </UCard>

      <UCard v-if="fields.length">
        <template #header>
          <div class="flex items-center gap-2">
            <span class="rounded-full bg-primary text-inverted size-6 grid place-items-center text-xs">2</span>
            <h3 class="font-medium">
              {{ t('datasources.inferredSchema') }}
            </h3>
            <span class="text-xs text-muted">{{ t('datasources.inferredSchemaHelp') }}</span>
          </div>
        </template>
        <UTable
          :data="fields"
          :columns="[{ accessorKey: 'path', header: t('datasources.field') }, { accessorKey: 'inferred_type', header: t('common.type') }, { accessorKey: 'null_ratio', header: t('datasources.nulls') }, { accessorKey: 'distinct_ratio', header: t('datasources.distinct') }, { accessorKey: 'pii', header: 'PII' }, { accessorKey: 'sample_values', header: t('datasources.samples') }]"
          class="text-sm max-h-80 overflow-auto"
        >
          <template #path-cell="{ row }">
            <span class="fp-mono text-xs">{{ row.original.path }}</span>
          </template>
          <template #inferred_type-cell="{ row }">
            {{ row.original.inferred_type }}<span
              v-if="row.original.datetime_format"
              class="text-xs text-muted fp-mono"
            > {{ row.original.datetime_format }}</span>
          </template>
          <template #null_ratio-cell="{ row }">
            {{ fmtPct(row.original.null_ratio, 0) }}
          </template>
          <template #distinct_ratio-cell="{ row }">
            {{ fmtPct(row.original.distinct_ratio, 0) }}
          </template>
          <template #pii-cell="{ row }">
            <UBadge
              v-if="row.original.pii"
              color="error"
              variant="soft"
              size="xs"
            >
              {{ row.original.pii }}
            </UBadge>
          </template>
          <template #sample_values-cell="{ row }">
            <span class="fp-mono text-xs truncate">{{ row.original.sample_values.slice(0, 3).map(v => JSON.stringify(v)).join(', ') }}</span>
          </template>
        </UTable>
      </UCard>

      <UCard>
        <template #header>
          <div class="flex flex-wrap items-center gap-2">
            <span class="rounded-full bg-primary text-inverted size-6 grid place-items-center text-xs">3</span>
            <h3 class="font-medium">
              {{ t('datasources.step3') }}
            </h3>
            <UBadge
              v-if="active"
              color="success"
              variant="soft"
              size="sm"
              class="ml-2"
            >
              {{ t('datasources.mappingActive', { v: active.version }) }}
            </UBadge>
          </div>
        </template>
        <UEmpty
          v-if="!fields.length && !Object.keys(draft.event).length"
          icon="i-lucide-scan-search"
          :title="t('datasources.inspectFirst')"
          size="sm"
        />
        <MappingEditor
          v-else
          v-model="draft"
          :fields="fields"
          :confidence="inspect?.confidence"
          :disabled="!can('analyst')"
        />
      </UCard>

      <UCard>
        <template #header>
          <div class="flex items-center gap-2">
            <span class="rounded-full bg-primary text-inverted size-6 grid place-items-center text-xs">4</span>
            <h3 class="font-medium">
              {{ t('datasources.step4') }}
            </h3>
          </div>
        </template>
        <div class="flex flex-wrap gap-2">
          <UButton
            :label="t('actions.preview')"
            icon="i-lucide-eye"
            color="neutral"
            variant="outline"
            @click="runPreview"
          />
          <UButton
            v-if="can('analyst')"
            :label="t('datasources.saveDraft')"
            icon="i-lucide-save"
            color="neutral"
            variant="outline"
            :loading="saving"
            @click="saveDraft(false)"
          />
          <UButton
            v-if="can('analyst')"
            :label="t('datasources.saveActivate')"
            icon="i-lucide-check-check"
            :loading="saving"
            @click="saveDraft(true)"
          />
          <template v-if="can('analyst') && ds?.kind !== 'webhook' && ds?.kind !== 'internal'">
            <USeparator
              orientation="vertical"
              class="h-8"
            />
            <USelect
              v-model="jobMode"
              :items="[{ label: t('datasources.modes.score'), value: 'score' }, { label: t('datasources.modes.load_only'), value: 'load_only' }]"
              class="w-44"
            />
            <UButton
              :label="t('datasources.startImport')"
              icon="i-lucide-play"
              :disabled="!active"
              @click="startJob"
            />
          </template>
        </div>
        <div
          v-if="preview"
          class="mt-4 space-y-2"
        >
          <p class="text-sm">
            <UBadge
              color="success"
              variant="soft"
            >
              {{ preview.filter(p => p.ok).length }} OK
            </UBadge>
            <UBadge
              color="error"
              variant="soft"
              class="ml-1"
            >
              {{ preview.filter(p => !p.ok).length }} {{ t('datasources.failed') }}
            </UBadge>
          </p>
          <div
            v-for="(row, i) in preview"
            :key="i"
            class="rounded-md border p-2 text-xs"
            :class="row.ok ? 'border-default' : 'border-error/50 bg-error/5'"
          >
            <p
              v-for="(e, j) in row.errors ?? []"
              :key="j"
              class="text-error"
            >
              <span class="fp-mono">{{ e.field }}</span> {{ e.message }}
            </p>
            <JsonTree
              v-if="row.event"
              :value="row.event"
              name="event"
              :open="i === 0"
            />
          </div>
        </div>
      </UCard>
    </div>

    <UCard v-else-if="tab === 'versions'">
      <UTable
        :data="[...mappings].reverse()"
        :columns="[{ accessorKey: 'version', header: 'v' }, { accessorKey: 'status', header: t('common.status') }, { accessorKey: 'created_at', header: t('common.created') }, { accessorKey: 'activated_at', header: t('datasources.activatedAt') }, { id: 'actions', header: '' }]"
        :empty="t('datasources.noMapping')"
      >
        <template #status-cell="{ row }">
          <StatusBadge
            :value="row.original.status"
            size="xs"
          />
        </template>
        <template #created_at-cell="{ row }">
          {{ fmtDate(row.original.created_at) }}
        </template>
        <template #activated_at-cell="{ row }">
          {{ fmtDate(row.original.activated_at) }}
        </template>
        <template #actions-cell="{ row }">
          <div class="flex gap-1 justify-end">
            <UButton
              size="xs"
              variant="ghost"
              :label="t('datasources.loadIntoEditor')"
              @click="loadVersion(row.original)"
            />
            <UButton
              v-if="can('analyst') && row.original.status !== 'active'"
              size="xs"
              variant="soft"
              :label="t('actions.activate')"
              @click="activateVersion(row.original.version)"
            />
          </div>
        </template>
      </UTable>
    </UCard>

    <UCard v-else-if="tab === 'jobs'">
      <UTable
        :data="jobs"
        :columns="[{ accessorKey: 'created_at', header: t('common.created') }, { accessorKey: 'mode', header: t('datasources.mode') }, { accessorKey: 'status', header: t('common.status') }, { accessorKey: 'progress', header: t('datasources.progress') }, { accessorKey: 'rejected_rows', header: t('datasources.rejected') }, { id: 'actions', header: '' }]"
        :empty="t('datasources.noJobs')"
      >
        <template #created_at-cell="{ row }">
          {{ fmtDate(row.original.created_at) }}
        </template>
        <template #mode-cell="{ row }">
          {{ t(`datasources.modes.${row.original.mode}`) }}
        </template>
        <template #status-cell="{ row }">
          <StatusBadge
            :value="row.original.status"
            size="xs"
          />
        </template>
        <template #progress-cell="{ row }">
          <div class="flex items-center gap-2 min-w-48">
            <UProgress
              :model-value="row.original.total_rows ? (row.original.processed_rows / row.original.total_rows) * 100 : undefined"
              size="xs"
              class="flex-1"
            />
            <span class="text-xs tabular-nums">{{ fmtCompact(row.original.processed_rows) }} / {{ fmtCompact(row.original.total_rows) }}</span>
          </div>
        </template>
        <template #rejected_rows-cell="{ row }">
          <span :class="row.original.rejected_rows ? 'text-error' : ''">{{ fmtNumber(row.original.rejected_rows) }}</span>
        </template>
        <template #actions-cell="{ row }">
          <UButton
            v-if="row.original.status === 'running' && can('analyst')"
            size="xs"
            color="error"
            variant="ghost"
            :label="t('actions.cancel')"
            @click="cancelJob(row.original)"
          />
        </template>
      </UTable>
    </UCard>

    <UCard v-else-if="tab === 'errors'">
      <UTable
        :data="errors?.items ?? []"
        :columns="[{ accessorKey: 'created_at', header: t('common.created') }, { accessorKey: 'reason', header: t('common.reason') }, { accessorKey: 'record', header: t('datasources.record') }]"
        :empty="t('datasources.noErrors')"
      >
        <template #created_at-cell="{ row }">
          <span class="text-xs">{{ fmtDate(row.original.created_at) }}</span>
        </template>
        <template #reason-cell="{ row }">
          <span class="text-error text-sm">{{ row.original.reason }}</span>
        </template>
        <template #record-cell="{ row }">
          <span class="fp-mono text-xs">{{ JSON.stringify(row.original.record).slice(0, 160) }}</span>
        </template>
      </UTable>
      <TablePager
        v-model:page="errPage"
        :total="errors?.total ?? 0"
        :page-size="50"
      />
    </UCard>

    <UCard v-else-if="tab === 'settings' && ds">
      <KeyValue :items="[{ label: 'Slug', value: ds.slug, mono: true }, { label: t('datasources.mode'), value: t(`datasources.modes.${ds.mode}`) }, { label: t('datasources.defaultEventType'), value: ds.default_event_type }, { label: t('datasources.apiKeyPrefix'), value: ds.api_key_prefix, mono: true }, { label: t('common.created'), value: fmtDate(ds.created_at) }]" />
      <p class="text-xs text-muted uppercase mt-4 mb-1">
        {{ t('datasources.connection') }}
      </p>
      <JsonTree
        :value="ds.connection"
        :open="true"
      />
    </UCard>
  </PagePanel>
</template>
