<script setup lang="ts">
// The signed-in user's own page: account, roles per project, appearance (theme, language) and sign-out.
const { t } = useI18n()
const session = useSessionStore()
const { logout } = useAccount()
useHead({ title: () => t('profile.title') })

const account = computed(() => {
  const u = session.user
  if (!u) return []
  return [
    { label: t('common.name'), value: u.full_name },
    { label: t('common.email'), value: u.email },
    { label: t('profile.tenant'), value: session.tenant?.name ?? '—' },
    { label: t('profile.tenantRole'), value: u.is_platform_admin ? t('profile.platformAdmin') : t(`roles.${u.tenant_role}`) },
    { label: t('profile.lastLogin'), value: u.last_login_at ? fmtDate(u.last_login_at) : '—' },
  ]
})
</script>

<template>
  <PagePanel
    :title="t('profile.title')"
    :description="t('profile.description')"
  >
    <div class="grid lg:grid-cols-2 gap-4 max-w-5xl">
      <UCard>
        <template #header>
          <div class="flex items-center gap-3">
            <UAvatar
              :alt="session.user?.full_name ?? '?'"
              size="lg"
            />
            <div class="min-w-0">
              <h3 class="font-medium truncate">
                {{ session.user?.full_name }}
              </h3>
              <p class="text-sm text-muted truncate">
                {{ session.user?.email }}
              </p>
            </div>
          </div>
        </template>
        <KeyValue :items="account" />
      </UCard>

      <UCard>
        <template #header>
          <h3 class="font-medium">
            {{ t('profile.appearance') }}
          </h3>
        </template>
        <div class="space-y-5">
          <div class="space-y-2">
            <p class="text-sm font-medium">
              {{ t('common.theme') }}
            </p>
            <ThemeSwitch />
            <p class="text-xs text-muted">
              {{ t('profile.themeHelp') }}
            </p>
          </div>
          <div class="space-y-2">
            <p class="text-sm font-medium">
              {{ t('common.language') }}
            </p>
            <LocaleSwitch />
          </div>
        </div>
      </UCard>

      <UCard class="lg:col-span-2">
        <template #header>
          <h3 class="font-medium">
            {{ t('profile.projects') }}
          </h3>
        </template>
        <UTable
          :data="session.projects"
          :columns="[{ accessorKey: 'name', header: t('common.name') }, { accessorKey: 'stage', header: t('profile.stage') }, { accessorKey: 'role', header: t('common.role') }]"
          :empty="t('profile.noProjects')"
        >
          <template #name-cell="{ row }">
            <NuxtLink
              :to="`/p/${row.original.id}`"
              class="text-primary hover:underline"
            >
              {{ row.original.name }}
            </NuxtLink>
          </template>
          <template #role-cell="{ row }">
            <UBadge
              color="neutral"
              variant="soft"
            >
              {{ t(`roles.${row.original.role}`) }}
            </UBadge>
          </template>
        </UTable>
      </UCard>

      <UCard class="lg:col-span-2">
        <template #header>
          <h3 class="font-medium">
            {{ t('profile.security') }}
          </h3>
        </template>
        <div class="flex flex-wrap items-center justify-between gap-3">
          <p class="text-sm text-muted max-w-2xl">
            {{ t('profile.passwordHelp') }}
          </p>
          <UButton
            color="error"
            variant="soft"
            icon="i-lucide-log-out"
            :label="t('actions.logout')"
            @click="logout"
          />
        </div>
      </UCard>
    </div>
  </PagePanel>
</template>
