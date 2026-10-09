<script setup lang="ts">
import type { Decision, Project, ProjectSettings } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const toast = useToast()
const session = useSessionStore()
const { pid, apiBase, project, refresh: refreshProject, can } = useProject()
// Profile fields are filled from the project: await it so SSR renders the same values as the client.
if (!project.value) await refreshProject()
useHead({ title: () => t('nav.settings') })
const { data: settings, refresh } = await useAsyncData(`settings-${pid.value}`, () => api.get<ProjectSettings>(`${apiBase.value}/settings`))
const s = ref<ProjectSettings | null>(settings.value ? structuredClone(toRaw(settings.value)) : null)
watch(settings, (v) => { s.value = v ? structuredClone(toRaw(v)) : null })
const readonly = computed(() => !can('project_admin'))

const weightSum = computed(() => (s.value ? Object.values(s.value.engine_weights).reduce((a, b) => a + b, 0) : 0))
const weightMax = computed(() => (s.value ? Math.max(...Object.values(s.value.engine_weights)) : 0))
const isNoisyOr = computed(() => (s.value?.engine_combination ?? 'noisy_or') === 'noisy_or')
/** noisy_or: relative exponent wᵢ/w_max; weighted_average: share of Σw. */
function weightLabel(w: number): string {
  if (isNoisyOr.value) return weightMax.value ? `^${(w / weightMax.value).toFixed(2)}` : '—'
  return weightSum.value ? fmtPct(w / weightSum.value, 0) : '—'
}
async function saveEngines() {
  await saveKey('engine_combination')
  await saveKey('engine_weights')
}
const thresholdsValid = computed(() => !!s.value && s.value.decision_thresholds.review < s.value.decision_thresholds.decline)

async function saveKey<K extends keyof ProjectSettings>(key: K) {
  if (!s.value) return
  await api.put(`${apiBase.value}/settings/${key}`, s.value[key])
  toast.add({ title: t('settings.saved', { key }), color: 'success' })
  await refresh()
}
const DECISIONS: Decision[] = ['approve', 'review', 'decline']

// project profile
const profile = reactive({ name: '', description: '', business_context: '' })
watch(project, (p) => { if (p) Object.assign(profile, { name: p.name, description: p.description ?? '', business_context: p.business_context ?? '' }) }, { immediate: true })
async function saveProfile() {
  await api.patch<Project>(`${apiBase.value}`, profile)
  toast.add({ title: t('settings.profileSaved'), color: 'success' })
  await refreshProject()
  await session.load(true)
}
async function archive() {
  await api.post(`${apiBase.value}/archive`)
  await session.load(true)
  await navigateTo('/projects')
}
</script>

<template>
  <PagePanel :title="t('nav.settings')">
    <div
      v-if="s"
      class="grid xl:grid-cols-2 gap-4 max-w-6xl"
    >
      <UCard>
        <template #header>
          <h3 class="font-medium">
            {{ t('settings.profile') }}
          </h3>
        </template>
        <div class="space-y-3">
          <UFormField :label="t('common.name')">
            <UInput
              v-model="profile.name"
              class="w-full"
              :disabled="readonly"
            />
          </UFormField>
          <UFormField :label="t('common.description')">
            <UInput
              v-model="profile.description"
              class="w-full"
              :disabled="readonly"
            />
          </UFormField>
          <UFormField
            :label="t('projects.businessContext')"
            :help="t('projects.businessContextHelp')"
          >
            <UTextarea
              v-model="profile.business_context"
              :rows="4"
              class="w-full"
              :disabled="readonly"
            />
          </UFormField>
          <div
            v-if="!readonly"
            class="flex justify-between"
          >
            <ConfirmAction
              v-if="session.isTenantAdmin"
              :label="t('settings.archive')"
              color="error"
              variant="ghost"
              icon="i-lucide-archive"
              :description="t('settings.archiveHelp')"
              :action="archive"
            />
            <UButton
              :label="t('actions.save')"
              @click="saveProfile"
            />
          </div>
        </div>
      </UCard>

      <UCard>
        <template #header>
          <h3 class="font-medium">
            {{ t('settings.thresholds') }}
          </h3>
        </template>
        <p class="text-sm text-muted mb-3">
          {{ t('settings.thresholdsHelp') }}
        </p>
        <div
          class="relative h-3 rounded-full overflow-hidden flex mb-4"
          aria-hidden="true"
        >
          <div
            class="bg-emerald-500"
            :style="{ width: `${s.decision_thresholds.review}%` }"
          />
          <div
            class="bg-amber-500"
            :style="{ width: `${Math.max(0, s.decision_thresholds.decline - s.decision_thresholds.review)}%` }"
          />
          <div class="bg-rose-500 flex-1" />
        </div>
        <div class="grid grid-cols-2 gap-3">
          <UFormField :label="`${t('decision.review')} ≥`">
            <UInputNumber
              v-model="s.decision_thresholds.review"
              :min="0"
              :max="100"
              :disabled="readonly"
            />
          </UFormField>
          <UFormField
            :label="`${t('decision.decline')} ≥`"
            :error="thresholdsValid ? undefined : t('settings.thresholdOrder')"
          >
            <UInputNumber
              v-model="s.decision_thresholds.decline"
              :min="0"
              :max="100"
              :disabled="readonly"
            />
          </UFormField>
        </div>
        <UButton
          v-if="!readonly"
          class="mt-3"
          :label="t('actions.save')"
          :disabled="!thresholdsValid"
          @click="saveKey('decision_thresholds')"
        />
      </UCard>

      <UCard>
        <template #header>
          <div class="flex items-center justify-between">
            <h3 class="font-medium">
              {{ t('settings.engineWeights') }}
            </h3>
            <UBadge
              v-if="!isNoisyOr"
              :color="Math.abs(weightSum - 1) < 0.001 ? 'success' : 'warning'"
              variant="soft"
            >
              Σ = {{ weightSum.toFixed(2) }}
            </UBadge>
          </div>
        </template>
        <UFormField
          :label="t('settings.engineCombination')"
          :help="t(`settings.combinationHelp.${s.engine_combination ?? 'noisy_or'}`)"
          class="mb-3"
        >
          <USelect
            v-model="s.engine_combination"
            :items="(['noisy_or', 'weighted_average'] as const).map(c => ({ label: t(`settings.combination.${c}`), value: c }))"
            class="w-64"
            :disabled="readonly"
          />
        </UFormField>
        <p class="text-sm text-muted mb-3">
          {{ isNoisyOr ? t('settings.engineWeightsHelpNoisyOr') : t('settings.engineWeightsHelp') }}
        </p>
        <div class="space-y-3">
          <div
            v-for="e in (['rules', 'supervised', 'unsupervised', 'graph'] as const)"
            :key="e"
            class="grid grid-cols-[8rem_1fr_4rem] items-center gap-3"
          >
            <span class="text-sm">{{ t(`engines.${e}`) }}</span>
            <USlider
              v-model="s.engine_weights[e]"
              :min="0"
              :max="1"
              :step="0.05"
              :disabled="readonly"
            />
            <span class="tabular-nums text-sm text-right">{{ weightLabel(s.engine_weights[e]) }}</span>
          </div>
        </div>
        <UButton
          v-if="!readonly"
          class="mt-3"
          :label="t('actions.save')"
          :disabled="weightSum <= 0"
          @click="saveEngines"
        />
      </UCard>

      <UCard>
        <template #header>
          <h3 class="font-medium">
            {{ t('settings.graphScores') }}
          </h3>
        </template>
        <p class="text-sm text-muted mb-3">
          {{ t('settings.graphScoresHelp') }}
        </p>
        <div class="grid grid-cols-4 gap-3">
          <UFormField
            v-for="d in ['1', '2', '3']"
            :key="d"
            :label="t('settings.distance', { d })"
          >
            <UInputNumber
              v-model="s.graph_scores.fraud_distance_scores[d]"
              :min="0"
              :max="100"
              :disabled="readonly"
            />
          </UFormField>
          <UFormField :label="t('settings.sharedEntity')">
            <UInputNumber
              v-model="s.graph_scores.shared_fraud_entity_score"
              :min="0"
              :max="100"
              :disabled="readonly"
            />
          </UFormField>
        </div>
        <UButton
          v-if="!readonly"
          class="mt-3"
          :label="t('actions.save')"
          @click="saveKey('graph_scores')"
        />
      </UCard>

      <UCard>
        <template #header>
          <h3 class="font-medium">
            {{ t('settings.timeouts') }}
          </h3>
        </template>
        <p class="text-sm text-muted mb-3">
          {{ t('settings.timeoutsHelp') }}
        </p>
        <div class="grid grid-cols-3 gap-3">
          <UFormField :label="t('engines.graph')">
            <UInputNumber
              v-model="s.timeouts.graph_ms"
              :min="10"
              :max="5000"
              :disabled="readonly"
            />
          </UFormField>
          <UFormField label="ML">
            <UInputNumber
              v-model="s.timeouts.ml_ms"
              :min="10"
              :max="5000"
              :disabled="readonly"
            />
          </UFormField>
          <UFormField :label="t('engines.rules')">
            <UInputNumber
              v-model="s.timeouts.rules_ms"
              :min="10"
              :max="5000"
              :disabled="readonly"
            />
          </UFormField>
        </div>
        <UFormField
          :label="t('settings.rulesUnavailable')"
          :help="t('settings.rulesUnavailableHelp')"
          class="mt-3"
        >
          <USelect
            v-model="s.rules_unavailable_decision"
            :items="DECISIONS.map(d => ({ label: t(`decision.${d}`), value: d }))"
            class="w-48"
            :disabled="readonly"
          />
        </UFormField>
        <div
          v-if="!readonly"
          class="flex gap-2 mt-3"
        >
          <UButton
            :label="t('actions.save')"
            @click="saveKey('timeouts').then(() => saveKey('rules_unavailable_decision'))"
          />
        </div>
      </UCard>

      <UCard>
        <template #header>
          <h3 class="font-medium">
            {{ t('nav.cases') }}
          </h3>
        </template>
        <UFormField :label="t('settings.autoCreateOn')">
          <USelectMenu
            v-model="s.cases.auto_create_on"
            :items="DECISIONS.map(d => ({ label: t(`decision.${d}`), value: d }))"
            value-key="value"
            multiple
            class="w-full"
            :disabled="readonly"
          />
        </UFormField>
        <p class="text-xs text-muted mt-2">
          {{ t('settings.oneOpenCaseNote') }}
        </p>
        <UButton
          v-if="!readonly"
          class="mt-3"
          :label="t('actions.save')"
          @click="saveKey('cases')"
        />
      </UCard>
    </div>
  </PagePanel>
</template>
