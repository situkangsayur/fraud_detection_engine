<script setup lang="ts">
const { t } = useI18n()
const route = useRoute()
const session = useSessionStore()

const items = computed(() => session.projects.map(p => ({
  label: p.name,
  value: p.id,
  description: `${t(`stages.${p.stage}`)} · ${t(`roles.${session.roleIn(p.id) ?? p.role}`)}`,
  icon: 'i-lucide-folder',
})))

const current = computed({
  get: () => (route.params.pid as string | undefined) ?? undefined,
  set: (pid) => {
    if (!pid) return
    // keep the same section when switching projects (e.g. /p/A/rules → /p/B/rules)
    const section = route.path.split('/').slice(3, 4).join('/')
    navigateTo(`/p/${pid}${section ? `/${section}` : ''}`)
  },
})
</script>

<template>
  <USelectMenu
    v-model="current"
    :items="items"
    value-key="value"
    :placeholder="t('common.selectProject')"
    icon="i-lucide-folder-kanban"
    class="w-full"
    :search-input="{ placeholder: t('common.search') }"
  >
    <template #empty>
      <span class="text-sm text-muted">{{ t('common.noProjects') }}</span>
    </template>
  </USelectMenu>
</template>
