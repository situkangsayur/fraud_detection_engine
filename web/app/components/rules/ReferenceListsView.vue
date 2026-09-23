<script setup lang="ts">
// Reference lists (whitelist/blacklist/watchlist/lookup) manager, shared by project and tenant-wide pages.
import type { ListType, Page, ReferenceEntry, ReferenceList } from '#shared/types/api'

const props = defineProps<{ basePath: string, entriesBase?: string, canEdit: boolean }>()
const { t } = useI18n()
const api = useApi()
const toast = useToast()
const { data, refresh, status } = await useAsyncData(`lists-${props.basePath}`, () => api.get<ReferenceList[] | Page<ReferenceList>>(props.basePath).then(r => (Array.isArray(r) ? r : r.items)))

const TYPES: ListType[] = ['blacklist', 'whitelist', 'watchlist', 'lookup']
const KEY_KINDS = ['card_fingerprint', 'device_id', 'email', 'phone', 'ip', 'customer_external_id', 'merchant_id', 'bin', 'generic']
const createOpen = ref(false)
const form = reactive({ name: '', description: '', list_type: 'blacklist' as ListType, key_kind: 'generic', columns: [] as { name: string, type: string }[] })
async function create() {
  await api.post(props.basePath, form)
  createOpen.value = false
  Object.assign(form, { name: '', description: '', columns: [] })
  await refresh()
}

// ---- entries slideover
const selected = ref<ReferenceList | null>(null)
const entriesPath = computed(() => (selected.value ? `${props.entriesBase ?? props.basePath}/${selected.value.id}` : ''))
const q = ref('')
const dq = useDebounced(q)
const page = ref(1)
const entries = ref<Page<ReferenceEntry> | null>(null)
async function loadEntries() {
  if (!selected.value) return
  entries.value = await api.get<Page<ReferenceEntry>>(`${entriesPath.value}/entries`, { query: { q: dq.value, page: page.value, page_size: 50 } })
}
watch([selected, dq, page], loadEntries)
const entry = reactive({ key: '', attributes: '{}', valid_until: '', reason: '' })
async function addEntry() {
  let attributes: Record<string, unknown> = {}
  try { attributes = JSON.parse(entry.attributes || '{}') }
  catch { toast.add({ title: t('errors.invalidJson'), color: 'error' }); return }
  await api.post(`${entriesPath.value}/entries`, { entries: [{ key: entry.key.trim(), attributes, reason: entry.reason || undefined, valid_until: entry.valid_until ? new Date(entry.valid_until).toISOString() : undefined }] })
  Object.assign(entry, { key: '', attributes: '{}', valid_until: '', reason: '' })
  await loadEntries()
}
async function removeEntry(e: ReferenceEntry) {
  await api.del(`${entriesPath.value}/entries/${e.id}`)
  await loadEntries()
}
const csv = ref<File | null>(null)
async function importCsv() {
  if (!csv.value) return
  const fd = new FormData()
  fd.append('file', csv.value)
  const res = await api.upload<{ imported: number }>(`${entriesPath.value}/import`, fd)
  toast.add({ title: t('lists.imported', { n: res.imported }), color: 'success' })
  csv.value = null
  await loadEntries()
}
async function removeList(l: ReferenceList) {
  await api.del(`${props.basePath}/${l.id}`)
  await refresh()
}
</script>

<template>
  <div>
    <div class="flex justify-end mb-3">
      <UModal
        v-if="canEdit"
        v-model:open="createOpen"
        :title="t('actions.newList')"
      >
        <UButton
          icon="i-lucide-plus"
          :label="t('actions.newList')"
        />
        <template #body>
          <div class="space-y-3">
            <UFormField
              :label="t('common.name')"
              required
              :help="t('lists.nameHelp')"
            >
              <UInput
                v-model="form.name"
                class="w-full fp-mono"
                placeholder="card_blacklist"
              />
            </UFormField>
            <UFormField :label="t('common.description')">
              <UInput
                v-model="form.description"
                class="w-full"
              />
            </UFormField>
            <div class="grid grid-cols-2 gap-3">
              <UFormField :label="t('common.type')">
                <USelect
                  v-model="form.list_type"
                  :items="TYPES.map(x => ({ label: t(`listTypes.${x}`), value: x }))"
                  class="w-full"
                />
              </UFormField>
              <UFormField :label="t('lists.keyKind')">
                <USelect
                  v-model="form.key_kind"
                  :items="KEY_KINDS"
                  class="w-full"
                />
              </UFormField>
            </div>
            <UFormField
              :label="t('lists.columns')"
              :help="t('lists.columnsHelp')"
            >
              <div class="space-y-2">
                <div
                  v-for="(c, i) in form.columns"
                  :key="i"
                  class="flex gap-2"
                >
                  <UInput
                    v-model="c.name"
                    class="flex-1 fp-mono"
                    placeholder="max_amount"
                  />
                  <USelect
                    v-model="c.type"
                    :items="['string', 'number', 'bool', 'datetime']"
                    class="w-32"
                  />
                  <UButton
                    icon="i-lucide-x"
                    color="neutral"
                    variant="ghost"
                    :aria-label="t('actions.remove')"
                    @click="form.columns.splice(i, 1)"
                  />
                </div>
                <UButton
                  size="xs"
                  variant="link"
                  icon="i-lucide-plus"
                  :label="t('lists.addColumn')"
                  @click="form.columns.push({ name: '', type: 'string' })"
                />
              </div>
            </UFormField>
          </div>
        </template>
        <template #footer>
          <UButton
            :label="t('actions.create')"
            :disabled="!/^[a-z0-9][a-z0-9_]{1,62}$/.test(form.name)"
            @click="create"
          />
        </template>
      </UModal>
    </div>
    <UTable
      :data="data ?? []"
      :loading="status === 'pending'"
      :columns="[{ accessorKey: 'name', header: t('common.name') }, { accessorKey: 'list_type', header: t('common.type') }, { accessorKey: 'key_kind', header: t('lists.keyKind') }, { accessorKey: 'scope', header: t('lists.scope') }, { accessorKey: 'entry_count', header: t('lists.entries') }, { id: 'actions', header: '' }]"
      class="cursor-pointer"
      :empty="t('common.empty')"
      @select="(_, row) => { selected = row.original; page = 1; q = '' }"
    >
      <template #name-cell="{ row }">
        <span class="fp-mono text-xs font-medium">{{ row.original.name }}</span>
        <p class="text-xs text-muted">
          {{ row.original.description }}
        </p>
      </template>
      <template #list_type-cell="{ row }">
        <UBadge
          :color="row.original.list_type === 'blacklist' ? 'error' : row.original.list_type === 'whitelist' ? 'success' : 'neutral'"
          variant="soft"
          size="sm"
        >
          {{ t(`listTypes.${row.original.list_type}`) }}
        </UBadge>
      </template>
      <template #scope-cell="{ row }">
        <UBadge
          :color="row.original.scope === 'tenant' ? 'info' : 'neutral'"
          variant="outline"
          size="xs"
        >
          {{ row.original.scope === 'tenant' ? t('rules.reference.tenantWide') : t('rules.reference.project') }}
        </UBadge>
      </template>
      <template #entry_count-cell="{ row }">
        {{ fmtNumber(row.original.entry_count) }}
      </template>
      <template #actions-cell="{ row }">
        <ConfirmAction
          v-if="canEdit"
          :label="t('actions.delete')"
          icon="i-lucide-trash-2"
          color="error"
          variant="ghost"
          size="xs"
          :description="t('lists.deleteHelp')"
          :action="() => removeList(row.original)"
          @click.stop
        />
      </template>
    </UTable>

    <USlideover
      :open="!!selected"
      :title="selected?.name"
      :description="selected?.description ?? undefined"
      :ui="{ content: 'max-w-3xl' }"
      @update:open="(v: boolean) => { if (!v) selected = null }"
    >
      <template #body>
        <div class="space-y-4">
          <div
            v-if="canEdit"
            class="rounded-md border border-default p-3 space-y-2"
          >
            <p class="text-sm font-medium">
              {{ t('lists.addEntry') }}
            </p>
            <div class="grid sm:grid-cols-2 gap-2">
              <UInput
                v-model="entry.key"
                :placeholder="t('lists.key')"
                class="fp-mono"
              />
              <UInput
                v-model="entry.reason"
                :placeholder="t('common.reason')"
              />
              <UInput
                v-model="entry.attributes"
                placeholder="{&quot;max_amount&quot;: 1000000}"
                class="fp-mono"
              />
              <UInput
                v-model="entry.valid_until"
                type="datetime-local"
                :aria-label="t('lists.validUntil')"
              />
            </div>
            <div class="flex flex-wrap gap-2 items-center">
              <UButton
                size="sm"
                icon="i-lucide-plus"
                :label="t('actions.add')"
                :disabled="!entry.key.trim()"
                @click="addEntry"
              />
              <USeparator
                orientation="vertical"
                class="h-6"
              />
              <UFileUpload
                v-model="csv"
                accept=".csv,text/csv"
                :label="t('lists.csvImport')"
                :description="t('lists.csvHelp')"
                class="w-64"
                size="sm"
                variant="button"
              />
              <UButton
                size="sm"
                color="neutral"
                variant="outline"
                icon="i-lucide-upload"
                :label="t('actions.import')"
                :disabled="!csv"
                @click="importCsv"
              />
            </div>
          </div>
          <UInput
            v-model="q"
            icon="i-lucide-search"
            :placeholder="t('common.search')"
            class="w-full"
          />
          <UTable
            :data="entries?.items ?? []"
            :columns="[{ accessorKey: 'key', header: t('lists.key') }, { accessorKey: 'attributes', header: t('lists.attributes') }, { accessorKey: 'valid_until', header: t('lists.validUntil') }, { accessorKey: 'reason', header: t('common.reason') }, { id: 'actions', header: '' }]"
            :empty="t('common.empty')"
          >
            <template #key-cell="{ row }">
              <span class="fp-mono text-xs">{{ row.original.key }}</span>
            </template>
            <template #attributes-cell="{ row }">
              <span class="fp-mono text-xs">{{ JSON.stringify(row.original.attributes) }}</span>
            </template>
            <template #valid_until-cell="{ row }">
              <span class="text-xs">{{ row.original.valid_until ? fmtDate(row.original.valid_until) : '∞' }}</span>
            </template>
            <template #actions-cell="{ row }">
              <UButton
                v-if="canEdit"
                icon="i-lucide-trash-2"
                size="xs"
                color="neutral"
                variant="ghost"
                :aria-label="t('actions.remove')"
                @click="removeEntry(row.original)"
              />
            </template>
          </UTable>
          <TablePager
            v-model:page="page"
            :total="entries?.total ?? 0"
            :page-size="50"
          />
        </div>
      </template>
    </USlideover>
  </div>
</template>
