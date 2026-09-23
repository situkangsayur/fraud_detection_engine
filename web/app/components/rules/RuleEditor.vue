<script setup lang="ts">
// Rule envelope + kind-specific definition editor with a Form ⇄ JSON toggle and validation (local + server).
import type { RuleEnvelope, RuleKind } from '#shared/rules/dsl'
import { ON_TRAPPED, RULE_ACTIONS, RULE_KINDS, SQL_OPERATORS } from '#shared/rules/dsl'
import type { RuleEnvelopeModel } from '#shared/rules/model'
import { defaultDefinition, describeDefinition, envelopeFromDsl, envelopeToDsl, localValidate, newGroup } from '#shared/rules/model'
import type { Typology, ValidationResult } from '#shared/types/api'
import { ApiError } from '~/composables/useApi'

const model = defineModel<RuleEnvelopeModel>({ required: true })
const props = defineProps<{ isNew?: boolean, disabled?: boolean }>()
const { t } = useI18n()
const api = useApi()
const { apiBase } = useProject()

const TYPOLOGIES: Typology[] = ['carding', 'account_takeover', 'bank_account_takeover', 'system_breach', 'promo_abuse', 'refund_abuse', 'money_mule', 'other']
const EVENT_TYPES = ['transaction', 'login', 'account_change', 'promo_redemption', 'payout', 'registration', 'refund']

const kind = computed({
  get: () => model.value.definition.kind,
  set: (k: RuleKind) => {
    model.value.kind = k
    model.value.definition = defaultDefinition(k)
  },
})

function toggleGate(on: boolean) {
  const d = model.value.definition
  if (d.kind !== 'composite') return
  if (on) d.gate = newGroup('all')
  else delete d.gate
}

// ---- JSON mode
const mode = ref<'form' | 'json'>('form')
const jsonText = ref('')
const jsonError = ref('')
watch(mode, (m) => {
  if (m === 'json') { jsonText.value = JSON.stringify(envelopeToDsl(model.value), null, 2); jsonError.value = '' }
})
function applyJson() {
  try {
    const parsed = JSON.parse(jsonText.value) as RuleEnvelope
    if (!parsed.definition?.kind) throw new Error('definition.kind is required')
    if (!props.isNew && parsed.code !== model.value.code) throw new Error(t('rules.codeImmutable'))
    model.value = envelopeFromDsl(parsed)
    jsonError.value = ''
    mode.value = 'form'
  }
  catch (err) {
    jsonError.value = err instanceof Error ? err.message : String(err)
  }
}

// ---- validation
const dsl = computed(() => envelopeToDsl(model.value))
const summary = computed(() => {
  try { return describeDefinition(dsl.value.definition) }
  catch { return '' }
})
const localIssues = computed(() => localValidate(dsl.value))
const server = ref<ValidationResult | null>(null)
const validating = ref(false)
async function validate(): Promise<boolean> {
  validating.value = true
  try {
    server.value = await api.post<ValidationResult>(`${apiBase.value}/rules/validate`, dsl.value, { silent: true })
    return server.value.valid && localIssues.value.length === 0
  }
  catch (err) {
    // 422 {valid:false, errors:[{path,message}]} → ApiError with field errors
    const fieldErrors = err instanceof ApiError ? err.problem.errors.map(e => ({ path: e.field, message: e.message })) : []
    server.value = { valid: false, errors: fieldErrors.length ? fieldErrors : [{ path: '', message: err instanceof Error ? err.message : String(err) }], referenced_fields: [], referenced_lists: [] }
    return false
  }
  finally { validating.value = false }
}
watch(dsl, () => { server.value = null }, { deep: true })

defineExpose({ validate, getEnvelope: () => envelopeToDsl(model.value) })
</script>

<template>
  <div class="grid xl:grid-cols-[22rem_1fr] gap-4">
    <!-- envelope -->
    <UCard>
      <div class="space-y-3">
        <UFormField
          :label="t('common.code')"
          required
          :help="isNew ? t('rules.codeHelp') : t('rules.codeImmutable')"
        >
          <UInput
            v-model="model.code"
            class="w-full fp-mono uppercase"
            :disabled="disabled || !isNew"
            placeholder="RL-CARD-010"
          />
        </UFormField>
        <UFormField
          :label="t('common.name')"
          required
        >
          <UInput
            v-model="model.name"
            class="w-full"
            :disabled="disabled"
          />
        </UFormField>
        <UFormField :label="t('common.description')">
          <UTextarea
            :model-value="model.description ?? ''"
            :rows="2"
            class="w-full"
            :disabled="disabled"
            @update:model-value="(v: string) => (model.description = v)"
          />
        </UFormField>
        <UFormField
          :label="t('common.kind')"
          required
        >
          <USelect
            v-model="kind"
            :items="RULE_KINDS.map(k => ({ label: t(`kinds.${k}`), value: k }))"
            class="w-full"
            :disabled="disabled || !isNew"
          />
        </UFormField>
        <UFormField :label="t('common.typologies')">
          <USelectMenu
            v-model="model.typologies"
            :items="TYPOLOGIES.map(x => ({ label: t(`typologies.${x}`), value: x as string }))"
            value-key="value"
            multiple
            class="w-full"
            :disabled="disabled"
          />
        </UFormField>
        <UFormField
          :label="t('common.eventTypes')"
          :help="t('rules.eventTypesHelp')"
        >
          <USelectMenu
            v-model="model.event_types"
            :items="EVENT_TYPES"
            multiple
            create-item
            class="w-full"
            :disabled="disabled"
            @create="(v: string) => model.event_types.push(v)"
          />
        </UFormField>
        <div class="grid grid-cols-2 gap-3">
          <UFormField
            :label="t('rules.riskScore')"
            required
          >
            <UInputNumber
              v-model="model.risk_score"
              :min="0"
              :max="100"
              :disabled="disabled"
            />
          </UFormField>
          <UFormField :label="t('rules.trappedScore')">
            <UInputNumber
              v-model="model.trapped_score"
              :min="0"
              :max="100"
              :disabled="disabled"
            />
          </UFormField>
        </div>
        <UFormField
          :label="t('rules.action')"
          :help="t(`rules.actions.help.${model.action ?? 'score'}`)"
        >
          <USelect
            v-model="model.action"
            :items="RULE_ACTIONS.map(a => ({ label: t(`rules.actions.${a}`), value: a }))"
            class="w-full"
            :disabled="disabled"
          />
        </UFormField>
        <UFormField
          :label="t('rules.onTrapped')"
          :help="t('rules.onTrappedHelp')"
        >
          <USelect
            v-model="model.on_trapped"
            :items="ON_TRAPPED.map(a => ({ label: t(`rules.trapped.${a}`), value: a }))"
            class="w-full"
            :disabled="disabled"
          />
        </UFormField>
        <UCheckbox
          v-model="model.missing_as_no_match"
          :label="t('rules.missingAsNoMatch')"
          :description="t('rules.missingAsNoMatchHelp')"
          :disabled="disabled"
        />
      </div>
    </UCard>

    <!-- definition -->
    <div class="space-y-4 min-w-0">
      <UCard>
        <template #header>
          <div class="flex flex-wrap items-center gap-2">
            <h3 class="font-medium">
              {{ t(`kinds.${kind}`) }}
            </h3>
            <span class="text-xs text-muted">{{ t(`kinds.help.${kind}`) }}</span>
            <UTabs
              v-model="mode"
              :items="[{ label: t('actions.viewForm'), value: 'form', icon: 'i-lucide-pencil-ruler' }, { label: 'JSON', value: 'json', icon: 'i-lucide-braces' }]"
              :content="false"
              size="xs"
              class="ml-auto"
            />
          </div>
        </template>

        <div
          v-if="mode === 'form'"
          class="space-y-4"
        >
          <template v-if="model.definition.kind === 'simple'">
            <UFormField
              :label="t('rules.scoring')"
              :help="t(`rules.scoringHelp.${model.definition.scoring ?? 'binary'}`)"
            >
              <URadioGroup
                v-model="model.definition.scoring"
                orientation="horizontal"
                :items="[{ label: t('rules.scoringBinary'), value: 'binary' }, { label: t('rules.scoringWeighted'), value: 'weighted' }]"
                :disabled="disabled"
              />
            </UFormField>
            <ConditionNode
              v-model="model.definition.when"
              :show-weight="model.definition.scoring === 'weighted'"
              :disabled="disabled"
            />
          </template>
          <VelocityEditor
            v-else-if="model.definition.kind === 'velocity'"
            v-model="model.definition"
            :disabled="disabled"
          />
          <template v-else-if="model.definition.kind === 'composite'">
            <div>
              <div class="flex items-center gap-2 mb-2">
                <h4 class="text-sm font-medium">
                  {{ t('rules.composite.gate') }}
                </h4>
                <span class="text-xs text-muted">{{ t('rules.composite.gateHelp') }}</span>
                <USwitch
                  :model-value="!!model.definition.gate"
                  size="sm"
                  class="ml-auto"
                  :disabled="disabled"
                  @update:model-value="toggleGate"
                />
              </div>
              <ConditionNode
                v-if="model.definition.gate"
                v-model="model.definition.gate"
                :disabled="disabled"
              />
            </div>
            <div>
              <div class="flex items-center gap-2 mb-2">
                <h4 class="text-sm font-medium">
                  {{ t('rules.composite.historyFilter') }}
                </h4>
                <span class="text-xs text-muted">{{ t('rules.composite.historyFilterHelp') }}</span>
              </div>
              <ConditionNode
                v-if="model.definition.history_filter"
                v-model="model.definition.history_filter"
                :left-types="['hist']"
                :right-types="['const', 'field', 'formula']"
                :operators="SQL_OPERATORS"
                :disabled="disabled"
              />
            </div>
            <USeparator :label="t('rules.composite.velocity')" />
            <VelocityEditor
              v-model="model.definition.velocity"
              :disabled="disabled"
            />
          </template>
          <ReferenceEditor
            v-else-if="model.definition.kind === 'reference'"
            v-model="model.definition"
            :disabled="disabled"
          />
          <GraphRuleEditor
            v-else-if="model.definition.kind === 'graph'"
            v-model="model.definition"
            :disabled="disabled"
          />
        </div>

        <div
          v-else
          class="space-y-2"
        >
          <CodeEditor
            v-model="jsonText"
            :readonly="disabled"
            height="460px"
            :aria-label="t('rules.jsonEditor')"
          />
          <UAlert
            v-if="jsonError"
            color="error"
            variant="subtle"
            :description="jsonError"
            icon="i-lucide-circle-alert"
          />
          <UButton
            v-if="!disabled"
            :label="t('actions.apply')"
            icon="i-lucide-check"
            size="sm"
            @click="applyJson"
          />
        </div>
      </UCard>

      <UCard :ui="{ body: 'p-3 sm:p-3' }">
        <div class="flex flex-wrap items-start gap-3">
          <div class="flex-1 min-w-0">
            <p class="text-xs text-muted uppercase">
              {{ t('rules.summary') }}
            </p>
            <p class="fp-mono text-xs break-words">
              {{ summary || '—' }}
            </p>
          </div>
          <UButton
            :label="t('actions.validate')"
            icon="i-lucide-shield-check"
            size="sm"
            color="neutral"
            variant="outline"
            :loading="validating"
            @click="validate"
          />
        </div>
        <ul
          v-if="localIssues.length || server?.errors.length"
          class="mt-2 space-y-1 text-xs"
        >
          <li
            v-for="(e, i) in [...localIssues, ...(server?.errors ?? [])]"
            :key="i"
            class="text-error"
          >
            <UIcon
              name="i-lucide-circle-x"
              class="align-middle"
            /> <span class="fp-mono">{{ e.path }}</span> {{ e.message }}
          </li>
        </ul>
        <p
          v-else-if="server?.valid"
          class="mt-2 text-xs text-success"
        >
          <UIcon
            name="i-lucide-circle-check"
            class="align-middle"
          /> {{ t('rules.valid') }}
          <span
            v-if="server.referenced_fields.length"
            class="text-muted"
          > · {{ server.referenced_fields.join(', ') }}</span>
        </p>
      </UCard>
    </div>
  </div>
</template>
