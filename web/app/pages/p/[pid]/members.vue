<script setup lang="ts">
import type { Items, Page, ProjectMember, ProjectRole, UserSummary } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const session = useSessionStore()
const { pid, apiBase, can } = useProject()
useHead({ title: () => t('nav.members') })
const ROLES: ProjectRole[] = ['viewer', 'analyst', 'approver', 'project_admin']
const { data: members, refresh } = await useAsyncData<ProjectMember[]>(`members-${pid.value}`, () => api.get<Items<ProjectMember>>(`${apiBase.value}/members`).then(r => r.items), { default: () => [] as ProjectMember[] })
const { data: users } = await useAsyncData<UserSummary[]>(`tenant-users-${session.tenant?.id}`, () => (session.tenant && session.isTenantAdmin ? api.get<Page<UserSummary>>(`/tenants/${session.tenant.id}/users`, { query: { page_size: 200 }, silent: true }).then(p => p.items).catch(() => []) : Promise.resolve([])), { default: () => [] as UserSummary[] })
const candidates = computed(() => users.value.filter(u => !members.value.some(m => m.user_id === u.id)).map(u => ({ label: `${u.full_name} <${u.email}>`, value: u.id })))
const add = reactive({ user_id: undefined as string | undefined, role: 'analyst' as ProjectRole })
async function setRole(user_id: string, role: ProjectRole) {
  await api.put(`${apiBase.value}/members`, { user_id, role })
  await refresh()
}
async function remove(user_id: string) {
  await api.del(`${apiBase.value}/members/${user_id}`)
  await refresh()
}
</script>

<template>
  <PagePanel
    :title="t('nav.members')"
    :description="t('members.description')"
  >
    <UCard
      v-if="can('project_admin') && candidates.length"
      class="mb-4"
    >
      <div class="flex flex-wrap gap-2 items-end">
        <UFormField
          :label="t('members.user')"
          class="flex-1 min-w-64"
        >
          <USelectMenu
            v-model="add.user_id"
            :items="candidates"
            value-key="value"
            class="w-full"
          />
        </UFormField>
        <UFormField :label="t('common.role')">
          <USelect
            v-model="add.role"
            :items="ROLES.map(r => ({ label: t(`roles.${r}`), value: r }))"
            class="w-44"
          />
        </UFormField>
        <UButton
          :label="t('actions.add')"
          icon="i-lucide-user-plus"
          :disabled="!add.user_id"
          @click="setRole(add.user_id!, add.role).then(() => (add.user_id = undefined))"
        />
      </div>
    </UCard>
    <UTable
      :data="members"
      :columns="[{ accessorKey: 'full_name', header: t('common.name') }, { accessorKey: 'email', header: t('common.email') }, { accessorKey: 'role', header: t('common.role') }, { id: 'actions', header: '' }]"
    >
      <template #role-cell="{ row }">
        <span v-if="!can('project_admin') || row.original.user_id === session.user?.id">{{ t(`roles.${row.original.role}`) }}</span>
        <ClientOnly v-else>
          <USelect
            :model-value="row.original.role"
            :items="ROLES.map(r => ({ label: t(`roles.${r}`), value: r }))"
            size="sm"
            class="w-44"
            :disabled="!can('project_admin') || row.original.user_id === session.user?.id"
            @update:model-value="(r: ProjectRole) => setRole(row.original.user_id, r)"
          />
          <template #fallback>
            <USkeleton class="h-8 w-40" />
          </template>
        </ClientOnly>
      </template>
      <template #actions-cell="{ row }">
        <ConfirmAction
          v-if="can('project_admin') && row.original.user_id !== session.user?.id"
          :label="t('actions.remove')"
          color="error"
          variant="ghost"
          size="xs"
          icon="i-lucide-user-minus"
          :action="() => remove(row.original.user_id)"
        />
      </template>
    </UTable>
    <UAlert
      class="mt-4"
      color="neutral"
      variant="subtle"
      icon="i-lucide-info"
      :description="t('members.roleHelp')"
    />
  </PagePanel>
</template>
