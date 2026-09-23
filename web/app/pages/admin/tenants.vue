<script setup lang="ts">
import type { Page, Tenant } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const toast = useToast()
useHead({ title: () => t('nav.tenants') })
const { data, refresh } = await useAsyncData('tenants', () => api.get<Page<Tenant>>('/tenants', { query: { page_size: 200 } }))
const open = ref(false)
const form = reactive({ slug: '', name: '', admin: { email: '', full_name: '', password: '' } })
async function create() {
  await api.post('/tenants', form)
  toast.add({ title: t('tenants.created'), color: 'success' })
  open.value = false
  await refresh()
}
async function toggle(tn: Tenant) {
  await api.patch(`/tenants/${tn.id}`, { status: tn.status === 'active' ? 'suspended' : 'active' })
  await refresh()
}
</script>

<template>
  <PagePanel
    :title="t('nav.tenants')"
    :description="t('tenants.description')"
  >
    <template #actions>
      <UModal
        v-model:open="open"
        :title="t('actions.newTenant')"
      >
        <UButton
          icon="i-lucide-plus"
          :label="t('actions.newTenant')"
        />
        <template #body>
          <div class="space-y-3">
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
                label="Slug"
                required
              >
                <UInput
                  v-model="form.slug"
                  class="w-full fp-mono"
                />
              </UFormField>
            </div>
            <USeparator :label="t('tenants.firstAdmin')" />
            <UFormField
              :label="t('common.email')"
              required
            >
              <UInput
                v-model="form.admin.email"
                type="email"
                class="w-full"
              />
            </UFormField>
            <UFormField
              :label="t('common.name')"
              required
            >
              <UInput
                v-model="form.admin.full_name"
                class="w-full"
              />
            </UFormField>
            <UFormField
              :label="t('login.password')"
              required
              :help="t('users.passwordHelp')"
            >
              <UInput
                v-model="form.admin.password"
                type="password"
                autocomplete="new-password"
                class="w-full"
              />
            </UFormField>
          </div>
        </template>
        <template #footer>
          <UButton
            :label="t('actions.create')"
            :disabled="!/^[a-z0-9][a-z0-9-]{1,62}$/.test(form.slug) || !form.name || !form.admin.email || form.admin.password.length < 12"
            @click="create"
          />
        </template>
      </UModal>
    </template>
    <UTable
      :data="data?.items ?? []"
      :columns="[{ accessorKey: 'name', header: t('common.name') }, { accessorKey: 'slug', header: 'Slug' }, { accessorKey: 'status', header: t('common.status') }, { accessorKey: 'users', header: t('nav.tenantUsers') }, { accessorKey: 'created_at', header: t('common.created') }, { id: 'actions', header: '' }]"
    >
      <template #slug-cell="{ row }">
        <span class="fp-mono text-xs">{{ row.original.slug }}</span>
      </template>
      <template #status-cell="{ row }">
        <StatusBadge
          :value="row.original.status"
          size="xs"
        />
      </template>
      <template #created_at-cell="{ row }">
        {{ fmtDate(row.original.created_at) }}
      </template>
      <template #users-cell="{ row }">
        {{ fmtNumber(row.original.users) }}
      </template>
      <template #actions-cell="{ row }">
        <UButton
          size="xs"
          variant="ghost"
          icon="i-lucide-users"
          :label="t('nav.tenantUsers')"
          :to="{ path: '/tenant/users', query: { tid: row.original.id, name: row.original.name } }"
        />
        <UButton
          size="xs"
          variant="ghost"
          icon="i-lucide-shield-ban"
          :label="t('nav.tenantLists')"
          :to="{ path: '/tenant/reference-lists', query: { tid: row.original.id } }"
        />
        <ConfirmAction
          :label="row.original.status === 'active' ? t('tenants.suspend') : t('tenants.reactivate')"
          :color="row.original.status === 'active' ? 'error' : 'success'"
          variant="ghost"
          size="xs"
          :action="() => toggle(row.original)"
        />
      </template>
    </UTable>
    <UAlert
      class="mt-4"
      color="neutral"
      variant="subtle"
      icon="i-lucide-lock"
      :description="t('tenants.privacyNote')"
    />
  </PagePanel>
</template>
