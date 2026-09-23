<script setup lang="ts">
import type { MlAlgorithm } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const toast = useToast()
const session = useSessionStore()
useHead({ title: () => t('nav.algorithms') })
const { data, refresh } = await useAsyncData<MlAlgorithm[]>('ml-algorithms', () => api.get<MlAlgorithm[]>('/ml/algorithms'), { default: () => [] as MlAlgorithm[] })
const kind = ref<'' | 'supervised' | 'anomaly' | 'clustering'>('')
const rows = computed(() => data.value.filter(a => !kind.value || a.kind === kind.value))
const expanded = ref<string | null>(null)
async function reload() {
  const res = await api.post<{ loaded: number, invalid: { module: string, error: string }[] }>('/ml/algorithms/reload')
  toast.add({ title: t('ml.pluginsReloaded', { n: res.loaded }), description: res.invalid.length ? t('ml.invalidPlugins', { n: res.invalid.length }) : undefined, color: res.invalid.length ? 'warning' : 'success' })
  await refresh()
}
</script>

<template>
  <PagePanel
    :title="t('nav.algorithms')"
    :description="t('ml.algorithmsDescription')"
  >
    <template #actions>
      <UButton
        v-if="session.isPlatformAdmin"
        icon="i-lucide-refresh-cw"
        :label="t('ml.reloadPlugins')"
        @click="reload"
      />
    </template>
    <template #toolbar>
      <UTabs
        v-model="kind"
        :items="[{ label: t('common.all'), value: '' }, { label: t('ml.kinds.supervised'), value: 'supervised' }, { label: t('ml.kinds.anomaly'), value: 'anomaly' }, { label: t('ml.kinds.clustering'), value: 'clustering' }]"
        variant="link"
        size="sm"
        :content="false"
      />
    </template>
    <div class="grid md:grid-cols-2 xl:grid-cols-3 gap-4">
      <UCard
        v-for="a in rows"
        :key="a.name"
        :class="a.status !== 'available' ? 'opacity-70' : ''"
      >
        <div class="flex items-start gap-2">
          <UIcon
            :name="a.kind === 'supervised' ? 'i-lucide-brain-circuit' : a.kind === 'anomaly' ? 'i-lucide-radar' : 'i-lucide-scatter-chart'"
            class="size-5 text-primary mt-0.5"
          />
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center gap-1.5">
              <h3 class="font-medium">
                {{ a.display_name }}
              </h3>
              <UBadge
                :color="a.source === 'plugin' ? 'info' : 'neutral'"
                variant="soft"
                size="xs"
              >
                {{ a.source }}
              </UBadge>
              <StatusBadge
                :value="a.status"
                size="xs"
              />
            </div>
            <p class="fp-mono text-xs text-muted">
              {{ a.name }} v{{ a.version }} · {{ t(`ml.kinds.${a.kind}`) }}
            </p>
            <p
              v-if="a.description"
              class="text-sm mt-1"
            >
              {{ a.description }}
            </p>
            <p
              v-if="a.error"
              class="text-xs text-error mt-1"
            >
              {{ a.error }}
            </p>
            <UButton
              size="xs"
              variant="link"
              :label="expanded === a.name ? t('ml.hideParams') : t('ml.showParams')"
              class="px-0"
              @click="expanded = expanded === a.name ? null : a.name"
            />
            <JsonTree
              v-if="expanded === a.name"
              :value="a.param_schema.properties ?? {}"
              name="params"
              :open="true"
            />
          </div>
        </div>
      </UCard>
    </div>
    <UAlert
      class="mt-4"
      color="neutral"
      variant="subtle"
      icon="i-lucide-puzzle"
      :title="t('ml.pluginsHowTo')"
      :description="t('ml.pluginsHowToHelp')"
    />
  </PagePanel>
</template>
