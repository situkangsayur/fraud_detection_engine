<script setup lang="ts">
import type { Page, ProjectListItem } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const session = useSessionStore()
useHead({ title: () => t('nav.projects') })

const showArchived = ref(false)
const { data, status } = await useAsyncData('projects', () => api.get<Page<ProjectListItem>>('/projects', { query: { page_size: 200, include_archived: showArchived.value || undefined } }), { watch: [showArchived] })
const stageIcon: Record<string, string> = {
  pre_payment: 'i-lucide-credit-card', post_payment: 'i-lucide-package-check', returns: 'i-lucide-undo-2', promo: 'i-lucide-ticket-percent',
  account_security: 'i-lucide-lock-keyhole', payout: 'i-lucide-banknote', custom: 'i-lucide-folder',
}
</script>

<template>
  <PagePanel
    :title="t('nav.projects')"
    :description="t('projects.description')"
  >
    <template #actions>
      <USwitch
        v-model="showArchived"
        :label="t('projects.showArchived')"
        size="sm"
      />
      <UButton
        v-if="session.isTenantAdmin"
        icon="i-lucide-plus"
        :label="t('actions.newProject')"
        to="/projects/new"
      />
    </template>

    <div
      v-if="status === 'pending'"
      class="grid md:grid-cols-2 xl:grid-cols-3 gap-4"
    >
      <USkeleton
        v-for="i in 3"
        :key="i"
        class="h-36"
      />
    </div>
    <UEmpty
      v-else-if="!data?.items.length"
      icon="i-lucide-folder-open"
      :title="t('projects.emptyTitle')"
      :description="session.isPlatformAdmin ? t('projects.emptyPlatformAdmin') : t('projects.emptyDescription')"
    />
    <div
      v-else
      class="grid md:grid-cols-2 xl:grid-cols-3 gap-4"
    >
      <NuxtLink
        v-for="p in data.items"
        :key="p.id"
        :to="`/p/${p.id}`"
        class="block focus:outline-none focus-visible:ring-2 ring-primary rounded-lg"
      >
        <UCard
          class="h-full hover:ring-primary/50 transition"
          :class="p.status === 'archived' ? 'opacity-60' : ''"
        >
          <div class="flex items-start gap-3">
            <div class="rounded-md p-2 bg-primary/10 text-primary">
              <UIcon
                :name="stageIcon[p.stage] ?? 'i-lucide-folder'"
                class="size-5"
              />
            </div>
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <h3 class="font-semibold truncate">
                  {{ p.name }}
                </h3>
                <UBadge
                  variant="outline"
                  size="sm"
                >
                  {{ t(`stages.${p.stage}`) }}
                </UBadge>
                <StatusBadge
                  v-if="p.status === 'archived'"
                  value="archived"
                  size="xs"
                />
              </div>
              <p class="text-xs text-muted fp-mono">
                {{ p.slug }}
              </p>
              <p class="text-sm mt-2 line-clamp-3 text-muted">
                {{ p.business_context ?? p.description }}
              </p>
              <div class="flex flex-wrap gap-1.5 mt-3 text-xs">
                <template v-if="p.ml_config">
                  <UBadge
                    color="neutral"
                    variant="soft"
                    size="sm"
                    icon="i-lucide-brain-circuit"
                  >
                    {{ p.ml_config.supervised.algorithm }}
                  </UBadge>
                  <UBadge
                    color="neutral"
                    variant="soft"
                    size="sm"
                    icon="i-lucide-scatter-chart"
                  >
                    {{ p.ml_config.unsupervised.anomaly_algorithm }} + {{ p.ml_config.unsupervised.clustering_algorithm }}
                  </UBadge>
                </template>
                <UBadge
                  color="neutral"
                  variant="soft"
                  size="sm"
                  icon="i-lucide-user"
                >
                  {{ t(`roles.${session.roleIn(p.id) ?? p.role ?? 'viewer'}`) }}
                </UBadge>
              </div>
            </div>
          </div>
        </UCard>
      </NuxtLink>
    </div>
  </PagePanel>
</template>
