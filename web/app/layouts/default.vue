<script setup lang="ts">
import type { NavigationMenuItem } from '@nuxt/ui'

const { t } = useI18n()
const route = useRoute()
const session = useSessionStore()
const pid = computed(() => (route.params.pid as string | undefined) ?? null)

const projectNav = computed<NavigationMenuItem[][]>(() => {
  if (!pid.value) return []
  const base = `/p/${pid.value}`
  const canAdmin = session.can(pid.value, 'project_admin')
  const canApprove = session.can(pid.value, 'approver')
  return [
    [
      { label: t('nav.sections.overview'), type: 'label' },
      { label: t('nav.dashboard'), icon: 'i-lucide-layout-dashboard', to: base, exact: true },
      { label: t('nav.events'), icon: 'i-lucide-activity', to: `${base}/events` },
      { label: t('nav.cases'), icon: 'i-lucide-briefcase', to: `${base}/cases` },
    ],
    [
      { label: t('nav.sections.ruleEngine'), type: 'label' },
      { label: t('nav.rules'), icon: 'i-lucide-list-checks', to: `${base}/rules` },
      { label: t('nav.rulesets'), icon: 'i-lucide-layers', to: `${base}/rulesets` },
      { label: t('nav.referenceLists'), icon: 'i-lucide-book-marked', to: `${base}/reference-lists` },
      { label: t('nav.formulas'), icon: 'i-lucide-sigma', to: `${base}/formulas` },
      { label: t('nav.proposals'), icon: 'i-lucide-git-pull-request', to: `${base}/proposals` },
    ],
    [
      { label: t('nav.sections.ml'), type: 'label' },
      { label: t('nav.mlSupervised'), icon: 'i-lucide-brain-circuit', to: `${base}/ml/supervised` },
      { label: t('nav.mlUnsupervised'), icon: 'i-lucide-scatter-chart', to: `${base}/ml/unsupervised` },
      { label: t('nav.algorithms'), icon: 'i-lucide-puzzle', to: `${base}/ml/algorithms` },
    ],
    [
      { label: t('nav.sections.graph'), type: 'label' },
      { label: t('nav.graph'), icon: 'i-lucide-share-2', to: `${base}/graph` },
    ],
    [
      { label: t('nav.sections.llm'), type: 'label' },
      { label: t('nav.llmChat'), icon: 'i-lucide-message-square-text', to: `${base}/llm/chat` },
      { label: t('nav.llmReports'), icon: 'i-lucide-file-text', to: `${base}/llm/reports` },
      { label: t('nav.llmRegulations'), icon: 'i-lucide-scale', to: `${base}/llm/regulations` },
    ],
    [
      { label: t('nav.sections.data'), type: 'label' },
      { label: t('nav.dataSources'), icon: 'i-lucide-database', to: `${base}/data-sources` },
      { label: t('nav.fieldCatalog'), icon: 'i-lucide-table-properties', to: `${base}/field-catalog` },
    ],
    [
      { label: t('nav.sections.project'), type: 'label' },
      { label: t('nav.settings'), icon: 'i-lucide-sliders-horizontal', to: `${base}/settings` },
      ...(canAdmin ? [{ label: t('nav.members'), icon: 'i-lucide-users', to: `${base}/members` }] : []),
      ...(canApprove ? [{ label: t('nav.audit'), icon: 'i-lucide-scroll-text', to: `${base}/audit` }] : []),
    ],
  ]
})

const globalNav = computed<NavigationMenuItem[][]>(() => {
  const items: NavigationMenuItem[] = [{ label: t('nav.projects'), icon: 'i-lucide-folder-kanban', to: '/projects' }]
  if (session.isTenantAdmin || session.isPlatformAdmin) {
    items.push(
      { label: t('nav.tenantUsers'), icon: 'i-lucide-user-cog', to: '/tenant/users' },
      { label: t('nav.tenantLists'), icon: 'i-lucide-shield-ban', to: '/tenant/reference-lists' },
    )
  }
  if (session.tenant) items.push({ label: t('nav.regulationLibrary'), icon: 'i-lucide-library', to: '/tenant/regulations' })
  if (session.isPlatformAdmin) items.push({ label: t('nav.tenants'), icon: 'i-lucide-building-2', to: '/admin/tenants' })
  return [[{ label: session.tenant?.name ?? t('nav.sections.admin'), type: 'label' }, ...items]]
})
</script>

<template>
  <UDashboardGroup
    unit="rem"
    storage="local"
  >
    <UDashboardSidebar
      id="main"
      collapsible
      resizable
      :default-size="16"
      :min-size="13"
      :max-size="22"
      class="bg-elevated/25"
    >
      <template #header="{ collapsed }">
        <NuxtLink
          to="/projects"
          class="flex items-center gap-2 font-semibold"
        >
          <UIcon
            name="i-lucide-shield-check"
            class="size-6 text-primary"
          />
          <span
            v-if="!collapsed"
            class="truncate"
          >{{ t('app.name') }}</span>
        </NuxtLink>
      </template>

      <template #default="{ collapsed }">
        <ProjectSwitcher v-if="!collapsed" />
        <UNavigationMenu
          v-if="pid"
          :collapsed="collapsed"
          :items="projectNav"
          orientation="vertical"
          tooltip
        />
        <UNavigationMenu
          :collapsed="collapsed"
          :items="globalNav"
          orientation="vertical"
          tooltip
          class="mt-auto"
        />
      </template>

      <template #footer="{ collapsed }">
        <UserMenu :collapsed="collapsed" />
      </template>
    </UDashboardSidebar>

    <slot />
  </UDashboardGroup>
</template>
