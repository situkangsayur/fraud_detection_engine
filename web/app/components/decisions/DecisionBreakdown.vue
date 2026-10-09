<script setup lang="ts">
// Decision summary: final score/decision, per-engine scores (with degraded engines), top reasons.
import type { DecisionOut } from '#shared/types/api'

const props = defineProps<{ decision: DecisionOut, thresholds?: { review: number, decline: number } }>()
const { t } = useI18n()
const ENGINES = ['rules', 'supervised', 'unsupervised', 'graph'] as const
const engineIcon = { rules: 'i-lucide-list-checks', supervised: 'i-lucide-brain-circuit', unsupervised: 'i-lucide-scatter-chart', graph: 'i-lucide-share-2' }
const forced = computed(() => props.decision.rule_results.find(r => !r.shadow && r.outcome === 'match' && r.action !== 'score'))
</script>

<template>
  <div class="space-y-4">
    <div class="flex flex-wrap items-center gap-4">
      <div>
        <p class="text-xs text-muted uppercase">
          {{ t('common.decision') }}
        </p>
        <StatusBadge
          :value="decision.decision"
          size="md"
        />
      </div>
      <div class="flex-1 min-w-48">
        <p class="text-xs text-muted uppercase">
          {{ t('decisions.finalScore') }}
        </p>
        <ScoreBar
          :score="decision.final_score"
          :thresholds="thresholds"
        />
      </div>
      <div class="text-xs text-muted">
        {{ t('decisions.latency', { ms: decision.latency_ms }) }}
        <span v-if="!decision.persisted"> · {{ t('decisions.simulated') }}</span>
      </div>
    </div>

    <UAlert
      v-if="forced"
      color="warning"
      variant="subtle"
      icon="i-lucide-gavel"
      :title="t('decisions.forcedBy', { code: forced.rule_code, action: forced.action })"
    />
    <UAlert
      v-if="decision.degraded.length"
      color="warning"
      variant="subtle"
      icon="i-lucide-triangle-alert"
      :title="t('decisions.degraded')"
      :description="decision.degraded.map(e => t(`engines.${e}`)).join(', ')"
    />

    <div class="grid sm:grid-cols-2 gap-3">
      <div
        v-for="e in ENGINES"
        :key="e"
        class="rounded-md border border-default p-3"
        :class="decision.degraded.includes(e) ? 'opacity-50' : ''"
      >
        <div class="flex items-center gap-2 text-sm font-medium mb-1">
          <UIcon
            :name="engineIcon[e]"
            class="text-primary"
          />
          {{ t(`engines.${e}`) }}
          <UBadge
            v-if="decision.degraded.includes(e)"
            color="warning"
            variant="soft"
            size="xs"
          >
            {{ t('decisions.unavailable') }}
          </UBadge>
        </div>
        <ScoreBar
          :score="decision.engine_scores[e] ?? null"
          :thresholds="thresholds"
        />
        <p
          v-if="e === 'supervised' && decision.ml.supervised_model"
          class="text-xs text-muted mt-1"
        >
          P(fraud) {{ fmtPct(decision.ml.fraud_probability) }} · {{ decision.ml.supervised_model.algorithm }} v{{ decision.ml.supervised_model.version }}
        </p>
        <p
          v-if="e === 'unsupervised' && decision.ml.cluster_id !== undefined"
          class="text-xs text-muted mt-1"
        >
          {{ t('ml.cluster') }} {{ decision.ml.cluster_id }} · {{ t('ml.fraudRate') }} {{ fmtPct(decision.ml.cluster_fraud_rate) }}
        </p>
        <p
          v-if="e === 'graph'"
          class="text-xs text-muted mt-1"
        >
          {{ t('graph.distanceToFraud') }}: {{ decision.graph.distance_to_fraud ?? '∞' }} · {{ t('graph.componentSize') }}: {{ decision.graph.component_size ?? '—' }}
        </p>
      </div>
    </div>

    <div v-if="decision.reasons.length">
      <p class="text-xs text-muted uppercase mb-1">
        {{ t('decisions.reasons') }}
        <UTooltip :text="t('decisions.reasonsHelp')">
          <UIcon
            name="i-lucide-info"
            class="align-middle normal-case"
          />
        </UTooltip>
      </p>
      <ul class="space-y-1">
        <li
          v-for="r in decision.reasons"
          :key="r.code"
          class="flex items-center gap-2 text-sm"
        >
          <UBadge
            color="neutral"
            variant="outline"
            size="sm"
            class="fp-mono"
          >
            {{ r.code }}
          </UBadge>
          <span class="flex-1">{{ r.message }}</span>
          <span class="tabular-nums text-muted">+{{ r.contribution.toFixed(1) }}</span>
        </li>
      </ul>
    </div>
  </div>
</template>
