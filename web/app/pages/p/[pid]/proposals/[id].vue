<script setup lang="ts">
import type { Proposal, RuleDetail } from '#shared/types/api'
import { describeDefinition } from '#shared/rules/model'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const toast = useToast()
const { base, apiBase, can } = useProject()
const id = computed(() => String(route.params.id))
const { data: p, refresh } = await useAsyncData(`proposal-${id.value}`, () => api.get<Proposal>(`${apiBase.value}/proposals/${id.value}`))
const { data: target } = await useAsyncData(`proposal-target-${id.value}`, () => (p.value?.target_rule_id ? api.get<RuleDetail>(`${apiBase.value}/rules/${p.value.target_rule_id}`, { silent: true }).catch(() => null) : Promise.resolve(null)))
useHead({ title: () => t('nav.proposals') })

async function approve() {
  const res = await api.post<Proposal>(`${apiBase.value}/proposals/${id.value}/approve`)
  toast.add({ title: t('proposals.approved'), description: t('proposals.approvedHelp'), color: 'success' })
  await refresh()
  if (res.applied_rule_id) await navigateTo(`${base.value}/rules/${res.applied_rule_id}`)
}
async function reject(comment: string) {
  await api.post(`${apiBase.value}/proposals/${id.value}/reject`, { comment })
  await refresh()
}
</script>

<template>
  <PagePanel :title="p ? `${t(`proposals.types.${p.proposal_type}`)} · ${p.definition?.code ?? p.target_rule_code ?? ''}` : t('nav.proposals')">
    <template #actions>
      <StatusBadge
        v-if="p"
        :value="p.status"
      />
      <template v-if="p?.status === 'pending' && can('approver')">
        <ConfirmAction
          :label="t('actions.approve')"
          color="success"
          icon="i-lucide-badge-check"
          :description="t('proposals.approveHelp')"
          :action="approve"
        />
        <ConfirmAction
          :label="t('actions.reject')"
          color="error"
          icon="i-lucide-x"
          with-comment
          comment-required
          :action="reject"
        />
      </template>
    </template>
    <div
      v-if="p"
      class="grid xl:grid-cols-3 gap-4"
    >
      <div class="xl:col-span-2 space-y-4">
        <UCard>
          <template #header>
            <h3 class="font-medium">
              {{ t('proposals.rationale') }}
            </h3>
          </template>
          <MarkdownView :source="p.rationale" />
          <p
            v-if="p.review_comment"
            class="text-sm mt-3"
          >
            <b>{{ t('proposals.reviewComment') }}:</b> {{ p.review_comment }}
          </p>
        </UCard>
        <UCard v-if="p.definition">
          <template #header>
            <div class="flex items-center gap-2">
              <h3 class="font-medium">
                {{ t('proposals.proposedRule') }}
              </h3>
              <UBadge
                color="neutral"
                variant="soft"
                size="sm"
              >
                {{ t(`kinds.${p.definition.kind}`) }}
              </UBadge>
            </div>
          </template>
          <p class="fp-mono text-xs mb-3 break-words">
            {{ describeDefinition(p.definition.definition) }}
          </p>
          <JsonDiff
            v-if="target"
            :before="target.envelope"
            :after="p.definition"
            :before-label="`${target.code} v${target.current_version}`"
            :after-label="t('proposals.proposed')"
          />
          <pre
            v-else
            class="fp-json bg-elevated rounded-md p-3 max-h-96 overflow-auto"
          >{{ JSON.stringify(p.definition, null, 2) }}</pre>
        </UCard>
        <UCard v-if="p.backtest">
          <template #header>
            <h3 class="font-medium">
              {{ t('proposals.backtestEvidence') }}
            </h3>
          </template>
          <div class="grid grid-cols-2 lg:grid-cols-4 gap-3">
            <StatCard
              :label="t('rules.backtest.matched')"
              :value="fmtNumber(p.backtest.matched)"
            />
            <StatCard
              :label="t('rules.backtest.hitRate')"
              :value="fmtPct(p.backtest.hit_rate, 2)"
            />
            <StatCard
              :label="t('rules.backtest.precision')"
              :value="fmtPct(p.backtest.precision)"
            />
            <StatCard
              :label="t('rules.backtest.recall')"
              :value="fmtPct(p.backtest.recall)"
            />
          </div>
        </UCard>
      </div>
      <div class="space-y-4">
        <UCard>
          <KeyValue
            :items="[
              { label: t('common.source'), value: p.source === 'llm' ? `LLM (${p.llm_model})` : t('proposals.analyst') },
              { label: t('common.created'), value: fmtDate(p.created_at) },
              { label: t('proposals.reviewedAt'), value: fmtDate(p.reviewed_at) },
              { key: 'report', label: t('proposals.report'), value: p.report_id },
              { key: 'applied', label: t('proposals.appliedRule'), value: p.applied_rule_id },
            ]"
          >
            <template #report="{ value }">
              <NuxtLink
                v-if="value"
                :to="`${base}/llm/reports/${value}`"
                class="text-primary text-xs"
              >
                {{ t('proposals.openReport') }}
              </NuxtLink>
              <span v-else>—</span>
            </template>
            <template #applied="{ value }">
              <NuxtLink
                v-if="value"
                :to="`${base}/rules/${value}`"
                class="text-primary text-xs"
              >
                {{ t('proposals.openRule') }}
              </NuxtLink>
              <span v-else>—</span>
            </template>
          </KeyValue>
        </UCard>
        <UCard>
          <template #header>
            <h3 class="font-medium">
              {{ t('proposals.validation') }}
            </h3>
          </template>
          <p
            v-if="p.validation.valid"
            class="text-sm text-success"
          >
            <UIcon
              name="i-lucide-circle-check"
              class="align-middle"
            /> {{ t('rules.valid') }}
          </p>
          <ul
            v-else-if="p.validation.errors?.length"
            class="text-xs text-error space-y-1"
          >
            <li
              v-for="(e, i) in p.validation.errors"
              :key="i"
            >
              <span class="fp-mono">{{ e.path }}</span> {{ e.message }}
            </li>
          </ul>
          <p
            v-else
            class="text-sm text-muted"
          >
            —
          </p>
        </UCard>
        <UCard v-if="p.citations.length">
          <template #header>
            <h3 class="font-medium">
              {{ t('llm.citations') }}
            </h3>
          </template>
          <ul class="space-y-3">
            <li
              v-for="(c, i) in p.citations"
              :key="i"
              class="text-sm"
            >
              <p class="font-medium">
                {{ c.code }} · {{ c.section }}
              </p>
              <blockquote class="border-l-2 border-primary pl-2 text-muted italic">
                {{ c.excerpt }}
              </blockquote>
            </li>
          </ul>
        </UCard>
        <UCard v-if="Object.keys(p.evidence).length">
          <template #header>
            <h3 class="font-medium">
              {{ t('proposals.evidence') }}
            </h3>
          </template>
          <JsonTree
            :value="p.evidence"
            :open="true"
          />
        </UCard>
      </div>
    </div>
  </PagePanel>
</template>
