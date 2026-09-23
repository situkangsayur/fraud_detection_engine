<script setup lang="ts">
import type { DataSource, Page, SourceKind } from '#shared/types/api'
import { EVENT_TYPES } from '#shared/utils/canonical'

const { t } = useI18n()
const api = useApi()
const { pid, base, apiBase, can } = useProject()
useHead({ title: () => t('nav.dataSources') })
const { data, refresh } = await useAsyncData(`sources-${pid.value}`, () => api.get<Page<DataSource>>(`${apiBase.value}/data-sources`, { query: { page_size: 200 } }))

const KINDS: SourceKind[] = ['webhook', 'file', 'postgres', 'mysql']
const kindIcon: Record<string, string> = { webhook: 'i-lucide-webhook', file: 'i-lucide-file-spreadsheet', postgres: 'i-lucide-database', mysql: 'i-lucide-database', internal: 'i-lucide-plug' }
const open = ref(false)
const form = reactive({
  name: '', slug: '', description: '', kind: 'webhook' as SourceKind, default_event_type: 'transaction', mode: 'score' as 'score' | 'load_only',
  conn: { host: '', port: 5432, database: '', user: '', password_env: '', table: '', poll_enabled: false, interval_seconds: 60, cursor_field: 'updated_at', batch_size: 500 },
})
watch(() => form.name, (n) => { form.slug = n.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 63) })
watch(() => form.kind, (k) => { if (k === 'mysql') form.conn.port = 3306; if (k === 'postgres') form.conn.port = 5432 })
const created = ref<DataSource | null>(null)
async function create() {
  const c = form.conn
  const connection = form.kind === 'postgres' || form.kind === 'mysql'
    ? { host: c.host, port: c.port, database: c.database, user: c.user, password_env: c.password_env, table: c.table, poll: { enabled: c.poll_enabled, interval_seconds: c.interval_seconds, cursor_field: c.cursor_field, batch_size: c.batch_size } }
    : {}
  const ds = await api.post<DataSource>(`${apiBase.value}/data-sources`, { name: form.name, slug: form.slug, description: form.description || undefined, kind: form.kind, default_event_type: form.default_event_type, mode: form.mode, connection })
  open.value = false
  await refresh()
  if (ds.api_key) created.value = ds
  else await navigateTo(`${base.value}/data-sources/${ds.id}`)
}
const origin = computed(() => (import.meta.client ? window.location.origin.replace(':3000', ':8080') : 'https://<gateway>'))
</script>

<template>
  <PagePanel
    :title="t('nav.dataSources')"
    :description="t('datasources.description')"
  >
    <template #actions>
      <UModal
        v-if="can('analyst')"
        v-model:open="open"
        :title="t('actions.newSource')"
        :ui="{ content: 'max-w-2xl' }"
      >
        <UButton
          icon="i-lucide-plus"
          :label="t('actions.newSource')"
        />
        <template #body>
          <div class="space-y-3">
            <URadioGroup
              v-model="form.kind"
              orientation="horizontal"
              variant="card"
              :items="KINDS.map(k => ({ label: t(`datasources.kinds.${k}`), value: k, description: t(`datasources.kindHelp.${k}`) }))"
            />
            <div class="grid grid-cols-2 gap-3">
              <UFormField
                :label="t('common.name')"
                required
              >
                <UInput
                  v-model="form.name"
                  class="w-full"
                />
              </UFormField>
              <UFormField
                :label="t('projects.slug')"
                required
              >
                <UInput
                  v-model="form.slug"
                  class="w-full fp-mono"
                />
              </UFormField>
              <UFormField :label="t('datasources.defaultEventType')">
                <USelect
                  v-model="form.default_event_type"
                  :items="[...EVENT_TYPES]"
                  class="w-full"
                />
              </UFormField>
              <UFormField
                :label="t('datasources.mode')"
                :help="t(`datasources.modeHelp.${form.mode}`)"
              >
                <USelect
                  v-model="form.mode"
                  :items="[{ label: t('datasources.modes.score'), value: 'score' }, { label: t('datasources.modes.load_only'), value: 'load_only' }]"
                  class="w-full"
                />
              </UFormField>
            </div>
            <UFormField :label="t('common.description')">
              <UInput
                v-model="form.description"
                class="w-full"
              />
            </UFormField>
            <template v-if="form.kind === 'postgres' || form.kind === 'mysql'">
              <USeparator :label="t('datasources.connection')" />
              <div class="grid grid-cols-3 gap-3">
                <UFormField
                  :label="t('datasources.host')"
                  class="col-span-2"
                >
                  <UInput
                    v-model="form.conn.host"
                    class="w-full"
                  />
                </UFormField>
                <UFormField :label="t('datasources.port')">
                  <UInputNumber v-model="form.conn.port" />
                </UFormField>
                <UFormField :label="t('datasources.database')">
                  <UInput
                    v-model="form.conn.database"
                    class="w-full"
                  />
                </UFormField>
                <UFormField :label="t('datasources.user')">
                  <UInput
                    v-model="form.conn.user"
                    class="w-full"
                  />
                </UFormField>
                <UFormField
                  :label="t('datasources.passwordEnv')"
                  :help="t('datasources.passwordEnvHelp')"
                >
                  <UInput
                    v-model="form.conn.password_env"
                    class="w-full fp-mono"
                    placeholder="SRC_ERP_DB_PASSWORD"
                  />
                </UFormField>
                <UFormField
                  :label="t('datasources.table')"
                  class="col-span-3"
                >
                  <UInput
                    v-model="form.conn.table"
                    class="w-full fp-mono"
                    placeholder="public.payments"
                  />
                </UFormField>
              </div>
              <USwitch
                v-model="form.conn.poll_enabled"
                :label="t('datasources.pollEnabled')"
                :description="t('datasources.pollHelp')"
              />
              <div
                v-if="form.conn.poll_enabled"
                class="grid grid-cols-3 gap-3"
              >
                <UFormField :label="t('datasources.cursorField')">
                  <UInput
                    v-model="form.conn.cursor_field"
                    class="w-full fp-mono"
                  />
                </UFormField>
                <UFormField :label="t('datasources.interval')">
                  <UInputNumber
                    v-model="form.conn.interval_seconds"
                    :min="10"
                  />
                </UFormField>
                <UFormField :label="t('datasources.batchSize')">
                  <UInputNumber
                    v-model="form.conn.batch_size"
                    :min="1"
                    :max="5000"
                  />
                </UFormField>
              </div>
            </template>
          </div>
        </template>
        <template #footer>
          <UButton
            :label="t('actions.create')"
            :disabled="!form.name || !/^[a-z0-9][a-z0-9_-]{1,62}$/.test(form.slug)"
            @click="create"
          />
        </template>
      </UModal>
    </template>

    <UAlert
      v-if="created"
      color="warning"
      variant="subtle"
      icon="i-lucide-key-round"
      :title="t('datasources.apiKeyOnce')"
      class="mb-4"
      :close="true"
      @update:open="created = null"
    >
      <template #description>
        <p class="fp-mono text-sm break-all select-all">
          {{ created.api_key }}
        </p>
        <pre class="fp-json mt-2 bg-elevated rounded p-2">curl -X POST {{ origin }}/api/v1/ingest/{{ created.slug }} \
  -H "X-Api-Key: {{ created.api_key }}" \
  -H "Content-Type: application/json" \
  -d '{"trx_id":"T-1","created":"2026-09-23T10:00:00Z","user":{"id":"U-1"},"total":150000}'</pre>
        <UButton
          size="xs"
          class="mt-2"
          :label="t('datasources.continueToMapping')"
          :to="`${base}/data-sources/${created.id}`"
        />
      </template>
    </UAlert>

    <div class="grid md:grid-cols-2 xl:grid-cols-3 gap-4">
      <NuxtLink
        v-for="s in data?.items ?? []"
        :key="s.id"
        :to="`${base}/data-sources/${s.id}`"
        class="block"
      >
        <UCard class="h-full hover:ring-primary/50 transition">
          <div class="flex items-start gap-3">
            <UIcon
              :name="kindIcon[s.kind] ?? 'i-lucide-plug'"
              class="size-6 text-primary"
            />
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <h3 class="font-medium truncate">
                  {{ s.name }}
                </h3>
                <UBadge
                  color="neutral"
                  variant="soft"
                  size="xs"
                >
                  {{ t(`datasources.kinds.${s.kind}`) }}
                </UBadge>
              </div>
              <p class="fp-mono text-xs text-muted">
                {{ s.slug }}
              </p>
              <p class="text-sm text-muted mt-1 line-clamp-2">
                {{ s.description }}
              </p>
              <div class="flex flex-wrap gap-1.5 mt-2">
                <UBadge
                  :color="s.active_mapping_version ? 'success' : 'warning'"
                  variant="soft"
                  size="xs"
                >
                  {{ s.active_mapping_version ? t('datasources.mappingActive', { v: s.active_mapping_version }) : s.kind === 'internal' ? t('datasources.canonical') : t('datasources.noMapping') }}
                </UBadge>
                <UBadge
                  color="neutral"
                  variant="outline"
                  size="xs"
                >
                  {{ t(`datasources.modes.${s.mode}`) }}
                </UBadge>
              </div>
            </div>
          </div>
        </UCard>
      </NuxtLink>
    </div>
  </PagePanel>
</template>
