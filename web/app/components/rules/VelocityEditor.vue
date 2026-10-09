<script setup lang="ts">
// Velocity body (rule-dsl §6.2): group_by, window, aggregate, optional statistic, compare.
import type { StatisticFn, VelocityBodyT } from '#shared/rules/dsl'
import { AGGREGATE_FNS, OPERATORS, STATISTIC_FNS } from '#shared/rules/dsl'
import type { OperandModel } from '#shared/rules/model'
import { newOperand } from '#shared/rules/model'

const model = defineModel<VelocityBodyT<OperandModel>>({ required: true })
defineProps<{ disabled?: boolean }>()
const { t } = useI18n()
const { velocityFields } = useFieldCatalog()
const EVENT_TYPES = ['transaction', 'login', 'account_change', 'promo_redemption', 'payout', 'registration', 'refund']

const windowMode = computed({
  get: () => ('last_n' in model.value.window ? 'last_n' : 'duration'),
  set: (m: 'duration' | 'last_n') => { model.value.window = m === 'duration' ? { duration: '24h' } : { last_n: 50 } },
})
const duration = computed({
  get: () => ('duration' in model.value.window ? model.value.window.duration : ''),
  set: (v: string) => { model.value.window = { duration: v } },
})
const lastN = computed({
  get: () => ('last_n' in model.value.window ? model.value.window.last_n : 50),
  set: (v: number) => { model.value.window = { last_n: v } },
})
const statFn = computed({
  get: () => model.value.statistic?.fn ?? ALL,
  set: (fn: StatisticFn | typeof ALL) => {
    if (fn === ALL) { model.value.statistic = null; return }
    const base: NonNullable<VelocityBodyT<OperandModel>['statistic']> = { fn }
    if (fn === 'zscore' || fn === 'gaussian_tail' || fn === 'percentile_rank') base.of = { ...newOperand('field'), path: `event.${model.value.aggregate.field ?? 'amount'}` } as OperandModel
    if (fn === 'gaussian_tail') base.tail = 'upper'
    if (fn === 'linear_trend') { base.bucket = '1d'; base.output = 'residual_z' }
    if (fn === 'poisson_tail') base.bucket = '1h'
    model.value.statistic = base
    model.value.include_current = false
    model.value.min_samples = Math.max(model.value.min_samples ?? 0, 5)
  },
})
const statHelp = computed(() => (model.value.statistic ? t(`rules.stat.help.${model.value.statistic.fn}`) : t('rules.stat.help.none')))
const fieldItems = computed(() => ['customer_id', ...velocityFields.value.filter(f => f !== 'customer_id')])
</script>

<template>
  <div class="grid lg:grid-cols-2 gap-4">
    <UFormField
      :label="t('rules.velocity.historyEventTypes')"
      :help="t('rules.velocity.historyEventTypesHelp')"
    >
      <USelectMenu
        v-model="model.history_event_types"
        :items="EVENT_TYPES"
        multiple
        create-item
        class="w-full"
        :disabled="disabled"
        @create="(v: string) => model.history_event_types = [...(model.history_event_types ?? []), v]"
      />
    </UFormField>
    <UFormField
      :label="t('rules.velocity.groupBy')"
      :help="t('rules.velocity.groupByHelp')"
      required
    >
      <USelectMenu
        v-model="model.group_by"
        :items="fieldItems"
        multiple
        create-item
        class="w-full fp-mono"
        :disabled="disabled"
        @create="(v: string) => model.group_by.push(v)"
      />
    </UFormField>

    <UFormField
      :label="t('rules.velocity.window')"
      required
    >
      <div class="flex gap-2">
        <USelect
          v-model="windowMode"
          :items="[{ label: t('rules.velocity.duration'), value: 'duration' }, { label: t('rules.velocity.lastN'), value: 'last_n' }]"
          class="w-40"
          :disabled="disabled"
        />
        <UInput
          v-if="windowMode === 'duration'"
          v-model="duration"
          placeholder="24h, 7d, 30m"
          class="w-28 fp-mono"
          :disabled="disabled"
        />
        <UInputNumber
          v-else
          v-model="lastN"
          :min="1"
          :max="10000"
          class="w-32"
          :disabled="disabled"
        />
      </div>
    </UFormField>
    <UFormField
      :label="t('rules.velocity.aggregate')"
      required
    >
      <div class="flex flex-wrap gap-2">
        <USelect
          v-model="model.aggregate.fn"
          :items="AGGREGATE_FNS"
          class="w-40"
          :disabled="disabled"
        />
        <USelectMenu
          v-if="model.aggregate.fn !== 'count'"
          v-model="model.aggregate.field"
          :items="fieldItems"
          create-item
          class="w-48 fp-mono"
          :placeholder="t('rules.velocity.field')"
          :disabled="disabled"
          @create="(v: string) => (model.aggregate.field = v)"
        />
        <UInputNumber
          v-if="model.aggregate.fn === 'percentile'"
          v-model="model.aggregate.p"
          :min="0.01"
          :max="0.99"
          :step="0.01"
          class="w-24"
          placeholder="0.95"
          :disabled="disabled"
        />
      </div>
    </UFormField>

    <UFormField
      :label="t('rules.stat.title')"
      :help="statHelp"
      class="lg:col-span-2"
    >
      <div class="flex flex-wrap items-start gap-2">
        <USelect
          v-model="statFn"
          :items="[{ label: t('rules.stat.none'), value: ALL }, ...STATISTIC_FNS.map(f => ({ label: t(`rules.stat.fn.${f}`), value: f }))]"
          class="w-56"
          :disabled="disabled"
        />
        <template v-if="model.statistic">
          <div
            v-if="model.statistic.of"
            class="flex items-center gap-1"
          >
            <span class="text-sm text-muted">x =</span>
            <OperandEditor
              v-model="model.statistic.of"
              :allowed="['field', 'const', 'formula']"
              :disabled="disabled"
            />
          </div>
          <USelect
            v-if="model.statistic.fn === 'gaussian_tail'"
            v-model="model.statistic.tail"
            :items="['upper', 'lower', 'two']"
            class="w-28"
            :disabled="disabled"
          />
          <UInput
            v-if="model.statistic.fn === 'linear_trend' || model.statistic.fn === 'poisson_tail'"
            v-model="model.statistic.bucket"
            class="w-24 fp-mono"
            placeholder="1d"
            :disabled="disabled"
          />
          <USelect
            v-if="model.statistic.fn === 'linear_trend'"
            v-model="model.statistic.output"
            :items="['slope', 'forecast', 'residual_z']"
            class="w-36"
            :disabled="disabled"
          />
        </template>
      </div>
    </UFormField>

    <div class="flex flex-wrap gap-4 items-end">
      <USwitch
        v-model="model.include_current"
        :label="t('rules.velocity.includeCurrent')"
        :disabled="disabled"
      />
      <UFormField :label="t('rules.velocity.minSamples')">
        <UInputNumber
          v-model="model.min_samples"
          :min="0"
          class="w-28"
          :disabled="disabled"
        />
      </UFormField>
    </div>
    <UFormField
      :label="t('rules.compare')"
      required
    >
      <div class="flex flex-wrap items-start gap-2">
        <span class="text-sm text-muted pt-1">{{ model.statistic ? t(`rules.stat.fn.${model.statistic.fn}`) : `${model.aggregate.fn}(${model.aggregate.field ?? '*'})` }}</span>
        <USelect
          v-model="model.compare.op"
          :items="OPERATORS.filter(o => !['is_null', 'is_not_null', 'regex', 'similar', 'contains', 'not_contains', 'starts_with', 'ends_with'].includes(o)).map(o => ({ label: t(`rules.ops.${o}`), value: o }))"
          class="w-32"
          size="sm"
          :disabled="disabled"
        />
        <OperandEditor
          v-model="model.compare.right"
          :allowed="['const', 'field', 'formula']"
          :disabled="disabled"
        />
      </div>
    </UFormField>
  </div>
</template>
