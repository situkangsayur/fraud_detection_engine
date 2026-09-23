<script setup lang="ts">
// Full rule evaluation trace (rule-dsl §7), including trapped reasons and shadow results.
import type { RuleResult } from '#shared/types/api'

const props = defineProps<{ results: RuleResult[], pid: string }>()
const { t } = useI18n()
const showShadow = ref(true)
const rows = computed(() => props.results.filter(r => showShadow.value || !r.shadow))
const expanded = ref<Record<string, boolean>>({})
</script>

<template>
  <div>
    <div class="flex justify-end mb-2">
      <USwitch
        v-model="showShadow"
        :label="t('rules.showShadow')"
        size="sm"
      />
    </div>
    <UEmpty
      v-if="!rows.length"
      icon="i-lucide-list-x"
      :title="t('rules.noResults')"
      size="sm"
    />
    <div
      v-else
      class="divide-y divide-default rounded-md border border-default"
    >
      <div
        v-for="r in rows"
        :key="`${r.rule_id}-${r.ruleset_code}`"
        class="p-2 text-sm"
      >
        <div class="flex flex-wrap items-center gap-2">
          <NuxtLink
            :to="`/p/${pid}/rules/${r.rule_id}`"
            class="fp-mono text-xs font-medium hover:text-primary"
          >
            {{ r.rule_code }}<span class="text-muted">@v{{ r.version }}</span>
          </NuxtLink>
          <UBadge
            color="neutral"
            variant="soft"
            size="xs"
          >
            {{ t(`kinds.${r.kind}`) }}
          </UBadge>
          <UBadge
            v-if="r.ruleset_code"
            color="neutral"
            variant="outline"
            size="xs"
          >
            {{ r.ruleset_code }}
          </UBadge>
          <StatusBadge
            :value="r.outcome"
            size="xs"
          />
          <UBadge
            v-if="r.shadow"
            color="info"
            variant="soft"
            size="xs"
          >
            {{ t('status.shadow') }}
          </UBadge>
          <UBadge
            v-if="r.action !== 'score'"
            color="warning"
            variant="soft"
            size="xs"
          >
            {{ r.action }}
          </UBadge>
          <span class="ml-auto tabular-nums">+{{ r.contribution.toFixed(1) }}</span>
          <span
            v-if="r.duration_us"
            class="text-xs text-muted tabular-nums w-16 text-right"
          >{{ r.duration_us }} µs</span>
          <UButton
            size="xs"
            color="neutral"
            variant="ghost"
            :icon="expanded[r.rule_id] ? 'i-lucide-chevron-up' : 'i-lucide-chevron-down'"
            :aria-label="t('actions.details')"
            @click="expanded[r.rule_id] = !expanded[r.rule_id]"
          />
        </div>
        <p
          v-if="r.trapped_reason"
          class="text-xs text-warning mt-1"
        >
          <UIcon
            name="i-lucide-triangle-alert"
            class="align-middle"
          /> {{ r.trapped_reason }}
        </p>
        <JsonTree
          v-if="expanded[r.rule_id]"
          :value="r.trace"
          name="trace"
          class="mt-1"
          :open="true"
        />
      </div>
    </div>
  </div>
</template>
