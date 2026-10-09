<script setup lang="ts">
import type { Page, TenantRole, UserSummary } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const toast = useToast()
const session = useSessionStore()
useHead({ title: () => t('nav.tenantUsers') })
const route = useRoute()
// Tenant admins manage their own tenant; platform admins arrive from /admin/tenants with ?tid=
const tid = computed(() => (route.query.tid as string | undefined) ?? session.tenant?.id)
const { data, refresh } = await useAsyncData<UserSummary[]>(`tenant-users-${tid.value}`, () => (tid.value ? api.get<Page<UserSummary>>(`/tenants/${tid.value}/users`, { query: { page_size: 200 } }).then(p => p.items) : Promise.resolve([])), { default: () => [] as UserSummary[] })
const open = ref(false)
const form = reactive({ email: '', full_name: '', password: '', tenant_role: 'member' as TenantRole })
async function create() {
  await api.post(`/tenants/${tid.value}/users`, form)
  toast.add({ title: t('users.created'), color: 'success' })
  open.value = false
  Object.assign(form, { email: '', full_name: '', password: '', tenant_role: 'member' })
  await refresh()
}
async function patch(u: UserSummary, body: Record<string, unknown>) {
  await api.patch(`/tenants/${tid.value}/users/${u.id}`, body)
  await refresh()
}
</script>

<template>
  <PagePanel
    :title="t('nav.tenantUsers')"
    :description="session.tenant?.name ?? (route.query.name as string | undefined)"
  >
    <template #actions>
      <UModal
        v-model:open="open"
        :title="t('actions.newUser')"
      >
        <UButton
          icon="i-lucide-user-plus"
          :label="t('actions.newUser')"
        />
        <template #body>
          <div class="space-y-3">
            <UFormField
              :label="t('common.email')"
              required
            >
              <UInput
                v-model="form.email"
                type="email"
                class="w-full"
              />
            </UFormField>
            <UFormField
              :label="t('common.name')"
              required
            >
              <UInput
                v-model="form.full_name"
                class="w-full"
              />
            </UFormField>
            <UFormField
              :label="t('login.password')"
              required
              :help="t('users.passwordHelp')"
            >
              <UInput
                v-model="form.password"
                type="password"
                autocomplete="new-password"
                class="w-full"
              />
            </UFormField>
            <UFormField :label="t('common.role')">
              <USelect
                v-model="form.tenant_role"
                :items="[{ label: t('roles.member'), value: 'member' }, { label: t('roles.tenant_admin'), value: 'tenant_admin' }]"
                class="w-full"
              />
            </UFormField>
          </div>
        </template>
        <template #footer>
          <UButton
            :label="t('actions.create')"
            :disabled="!form.email || !form.full_name || form.password.length < 12"
            @click="create"
          />
        </template>
      </UModal>
    </template>
    <UTable
      :data="data"
      :columns="[{ accessorKey: 'full_name', header: t('common.name') }, { accessorKey: 'email', header: t('common.email') }, { accessorKey: 'tenant_role', header: t('common.role') }, { accessorKey: 'is_active', header: t('common.status') }]"
    >
      <template #tenant_role-cell="{ row }">
        <USelect
          :model-value="row.original.tenant_role"
          :items="[{ label: t('roles.member'), value: 'member' }, { label: t('roles.tenant_admin'), value: 'tenant_admin' }]"
          size="sm"
          class="w-44"
          :disabled="row.original.id === session.user?.id"
          @update:model-value="(r: string) => patch(row.original, { tenant_role: r as TenantRole })"
        />
      </template>
      <template #is_active-cell="{ row }">
        <USwitch
          :model-value="row.original.is_active !== false"
          size="sm"
          :disabled="row.original.id === session.user?.id"
          @update:model-value="(v: boolean) => patch(row.original, { is_active: v })"
        />
      </template>
    </UTable>
    <p class="text-xs text-muted mt-3">
      {{ t('users.projectRolesHint') }}
    </p>
  </PagePanel>
</template>
