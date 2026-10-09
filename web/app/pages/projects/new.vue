<script setup lang="ts">
import type { GraphConfig, LinkKind, LlmConfig, MlAlgorithm, MlConfig, Project, ProjectCreate, ProjectStage } from '#shared/types/api'

const { t } = useI18n()
const api = useApi()
const session = useSessionStore()
const toast = useToast()
useHead({ title: () => t('actions.newProject') })

if (!session.isTenantAdmin) await navigateTo('/projects')

const { data: algorithms } = await useAsyncData<MlAlgorithm[]>('ml-algorithms', () => api.get<MlAlgorithm[]>('/ml/algorithms'), { default: () => [] as MlAlgorithm[] })

const STAGES: ProjectStage[] = ['pre_payment', 'post_payment', 'returns', 'promo', 'account_security', 'payout', 'custom']
const LINK_KINDS: LinkKind[] = ['email', 'phone', 'device', 'ip', 'card', 'bank_account', 'address', 'ref_transaction', 'api_client']

const basics = reactive({
  name: '', slug: '', description: '', stage: 'pre_payment' as ProjectStage, business_context: '',
  timezone: 'Asia/Jakarta', currency: 'IDR', template: 'pre_payment' as ProjectStage | 'none',
})
const slugTouched = ref(false)
watch(() => basics.name, (name) => {
  if (!slugTouched.value) basics.slug = name.toLowerCase().normalize('NFKD').replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 63)
})
watch(() => basics.stage, (s) => { basics.template = s === 'custom' ? 'none' : s })

const ml = reactive<MlConfig>({
  supervised: { algorithm: 'mlp_backprop', params: {} },
  unsupervised: { anomaly_algorithm: 'isolation_forest', anomaly_params: {}, clustering_algorithm: 'hdbscan', clustering_params: {} },
  features: { include: ['*'], exclude: [], extra_source_fields: [] },
})
const llm = reactive<LlmConfig>({ chat_model: 'qwen2.5:7b-instruct', temperature: 0.1, language: 'id', system_prompt_extra: '' })
const graph = reactive<GraphConfig>({
  link_kinds: ['email', 'phone', 'device', 'card', 'bank_account', 'address', 'ref_transaction'],
  include_similar: true, max_depth: 3, supernode_degree_cap: 50, similarity_threshold: 0.85,
})

const supPicker = ref<{ valid: boolean }>()
const anomPicker = ref<{ valid: boolean }>()
const clusPicker = ref<{ valid: boolean }>()

const step = ref(0)
const steps = computed(() => [
  { title: t('projects.wizard.basics'), icon: 'i-lucide-info', value: 0 },
  { title: t('projects.wizard.ml'), icon: 'i-lucide-brain-circuit', value: 1 },
  { title: t('projects.wizard.llmGraph'), icon: 'i-lucide-share-2', value: 2 },
  { title: t('projects.wizard.review'), icon: 'i-lucide-check', value: 3 },
])

const slugValid = computed(() => /^[a-z0-9][a-z0-9-]{1,62}$/.test(basics.slug))
const stepValid = computed(() => {
  if (step.value === 0) return !!basics.name.trim() && slugValid.value
  if (step.value === 1) return (supPicker.value?.valid ?? true) && (anomPicker.value?.valid ?? true) && (clusPicker.value?.valid ?? true)
  return true
})

const payload = computed<ProjectCreate>(() => ({
  ...basics,
  description: basics.description || undefined,
  business_context: basics.business_context || undefined,
  ml_config: JSON.parse(JSON.stringify(ml)),
  llm_config: { ...llm },
  graph_config: JSON.parse(JSON.stringify(graph)),
}))

const busy = ref(false)
async function create() {
  busy.value = true
  try {
    const p = await api.post<Project>('/projects', payload.value)
    await session.load(true)
    toast.add({ title: t('projects.created'), description: p.name, color: 'success' })
    await navigateTo(`/p/${p.id}`)
  }
  catch { /* toast shown */ }
  finally { busy.value = false }
}
</script>

<template>
  <PagePanel :title="t('actions.newProject')">
    <div class="max-w-4xl space-y-6">
      <UStepper
        v-model="step"
        :items="steps"
        size="sm"
        disabled
      />

      <UCard v-show="step === 0">
        <div class="grid sm:grid-cols-2 gap-4">
          <UFormField
            :label="t('common.name')"
            required
          >
            <UInput
              v-model="basics.name"
              class="w-full"
              placeholder="Checkout protection"
            />
          </UFormField>
          <UFormField
            :label="t('projects.slug')"
            required
            :error="basics.slug && !slugValid ? t('projects.slugHelp') : undefined"
            :help="t('projects.slugHelp')"
          >
            <UInput
              v-model="basics.slug"
              class="w-full fp-mono"
              @update:model-value="slugTouched = true"
            />
          </UFormField>
          <UFormField
            :label="t('projects.stage')"
            required
          >
            <USelect
              v-model="basics.stage"
              :items="STAGES.map(s => ({ label: t(`stages.${s}`), value: s }))"
              class="w-full"
            />
          </UFormField>
          <UFormField
            :label="t('projects.template')"
            :help="t('projects.templateHelp')"
          >
            <USelect
              v-model="basics.template"
              :items="[{ label: t('projects.noTemplate'), value: 'none' }, ...STAGES.filter(s => s !== 'custom').map(s => ({ label: t(`stages.${s}`), value: s }))]"
              class="w-full"
            />
          </UFormField>
          <UFormField :label="t('projects.timezone')">
            <UInput
              v-model="basics.timezone"
              class="w-full"
            />
          </UFormField>
          <UFormField :label="t('projects.currency')">
            <UInput
              v-model="basics.currency"
              maxlength="3"
              class="w-full uppercase"
            />
          </UFormField>
          <UFormField
            :label="t('common.description')"
            class="sm:col-span-2"
          >
            <UInput
              v-model="basics.description"
              class="w-full"
            />
          </UFormField>
          <UFormField
            :label="t('projects.businessContext')"
            :help="t('projects.businessContextHelp')"
            class="sm:col-span-2"
          >
            <UTextarea
              v-model="basics.business_context"
              :rows="4"
              class="w-full"
            />
          </UFormField>
        </div>
      </UCard>

      <div
        v-show="step === 1"
        class="space-y-4"
      >
        <UCard>
          <template #header>
            <h3 class="font-medium">
              {{ t('nav.mlSupervised') }}
            </h3>
          </template>
          <AlgorithmPicker
            ref="supPicker"
            v-model:algorithm="ml.supervised.algorithm"
            v-model:params="ml.supervised.params"
            kind="supervised"
            :algorithms="algorithms"
          />
        </UCard>
        <UCard>
          <template #header>
            <h3 class="font-medium">
              {{ t('nav.mlUnsupervised') }}
            </h3>
          </template>
          <div class="grid lg:grid-cols-2 gap-6">
            <AlgorithmPicker
              ref="anomPicker"
              v-model:algorithm="ml.unsupervised.anomaly_algorithm"
              v-model:params="ml.unsupervised.anomaly_params"
              kind="anomaly"
              :label="t('ml.anomalyAlgorithm')"
              :algorithms="algorithms"
            />
            <AlgorithmPicker
              ref="clusPicker"
              v-model:algorithm="ml.unsupervised.clustering_algorithm"
              v-model:params="ml.unsupervised.clustering_params"
              kind="clustering"
              :label="t('ml.clusteringAlgorithm')"
              :algorithms="algorithms"
            />
          </div>
        </UCard>
        <UCard>
          <template #header>
            <h3 class="font-medium">
              {{ t('ml.featureSelection') }}
            </h3>
          </template>
          <div class="grid sm:grid-cols-3 gap-4">
            <UFormField :label="t('ml.include')">
              <UInputTags
                v-model="ml.features.include"
                class="w-full"
              />
            </UFormField>
            <UFormField :label="t('ml.exclude')">
              <UInputTags
                v-model="ml.features.exclude"
                class="w-full"
              />
            </UFormField>
            <UFormField
              :label="t('ml.extraSourceFields')"
              :help="t('ml.extraSourceFieldsHelp')"
            >
              <UInputTags
                v-model="ml.features.extra_source_fields"
                class="w-full"
                placeholder="source.order.item_count"
              />
            </UFormField>
          </div>
        </UCard>
      </div>

      <div
        v-show="step === 2"
        class="grid lg:grid-cols-2 gap-4"
      >
        <UCard>
          <template #header>
            <h3 class="font-medium">
              LLM
            </h3>
          </template>
          <div class="space-y-3">
            <UFormField
              :label="t('projects.chatModel')"
              :help="t('projects.chatModelHelp')"
            >
              <UInput
                :model-value="llm.chat_model ?? ''"
                class="w-full fp-mono"
                @update:model-value="(v: string | number) => (llm.chat_model = String(v) || null)"
              />
            </UFormField>
            <UFormField :label="t('projects.temperature')">
              <USlider
                v-model="llm.temperature"
                :min="0"
                :max="1"
                :step="0.05"
              />
              <span class="text-xs text-muted">{{ llm.temperature }}</span>
            </UFormField>
            <UFormField :label="t('common.language')">
              <USelect
                v-model="llm.language"
                :items="[{ label: 'Bahasa Indonesia', value: 'id' }, { label: 'English', value: 'en' }]"
                class="w-full"
              />
            </UFormField>
            <UFormField
              :label="t('projects.systemPromptExtra')"
              :help="t('projects.systemPromptExtraHelp')"
            >
              <UTextarea
                v-model="llm.system_prompt_extra"
                :rows="3"
                class="w-full"
              />
            </UFormField>
          </div>
        </UCard>
        <UCard>
          <template #header>
            <h3 class="font-medium">
              {{ t('nav.graph') }}
            </h3>
          </template>
          <div class="space-y-3">
            <UFormField :label="t('graph.linkKinds')">
              <USelectMenu
                v-model="graph.link_kinds"
                :items="LINK_KINDS"
                multiple
                class="w-full"
              />
            </UFormField>
            <USwitch
              v-model="graph.include_similar"
              :label="t('graph.includeSimilar')"
            />
            <div class="grid grid-cols-3 gap-3">
              <UFormField :label="t('graph.maxDepth')">
                <UInputNumber
                  v-model="graph.max_depth"
                  :min="1"
                  :max="4"
                />
              </UFormField>
              <UFormField :label="t('graph.supernodeCap')">
                <UInputNumber
                  v-model="graph.supernode_degree_cap"
                  :min="2"
                />
              </UFormField>
              <UFormField :label="t('graph.similarityThreshold')">
                <UInputNumber
                  v-model="graph.similarity_threshold"
                  :min="0.5"
                  :max="1"
                  :step="0.01"
                />
              </UFormField>
            </div>
          </div>
        </UCard>
      </div>

      <UCard v-show="step === 3">
        <p class="text-sm mb-3">
          {{ t('projects.reviewHint') }}
        </p>
        <pre class="fp-json bg-elevated rounded-md p-3 max-h-[28rem] overflow-auto">{{ JSON.stringify(payload, null, 2) }}</pre>
      </UCard>

      <div class="flex justify-between">
        <UButton
          :label="t('actions.back')"
          color="neutral"
          variant="ghost"
          icon="i-lucide-arrow-left"
          :disabled="step === 0"
          @click="step--"
        />
        <UButton
          v-if="step < 3"
          :label="t('actions.next')"
          trailing-icon="i-lucide-arrow-right"
          :disabled="!stepValid"
          @click="step++"
        />
        <UButton
          v-else
          :label="t('actions.create')"
          icon="i-lucide-check"
          :loading="busy"
          @click="create"
        />
      </div>
    </div>
  </PagePanel>
</template>
