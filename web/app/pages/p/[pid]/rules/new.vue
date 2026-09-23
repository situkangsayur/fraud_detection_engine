<script setup lang="ts">
import type { RuleKind } from '#shared/rules/dsl'
import { RULE_EXAMPLES } from '#shared/rules/examples'
import type { RuleEnvelopeModel } from '#shared/rules/model'
import { defaultEnvelope, envelopeFromDsl } from '#shared/rules/model'
import type { Rule } from '#shared/types/api'
import type { RuleEditorExpose } from '~/types/ui'

const { t } = useI18n()
const api = useApi()
const route = useRoute()
const toast = useToast()
const { base, apiBase, can } = useProject()
useHead({ title: () => t('actions.newRule') })
if (!can('analyst')) await navigateTo(`${base.value}/rules`)

function initial(): RuleEnvelopeModel {
  const tpl = RULE_EXAMPLES.find(e => e.code === route.query.template)
  if (tpl) return envelopeFromDsl({ ...tpl, code: `${tpl.code}-COPY`.slice(0, 40) })
  return defaultEnvelope((route.query.kind as RuleKind) ?? 'simple')
}
const envelope = ref<RuleEnvelopeModel>(initial())
const editor = ref<RuleEditorExpose>()
const saving = ref(false)
const tab = ref('edit')

async function save() {
  if (!editor.value) return
  saving.value = true
  try {
    if (!(await editor.value.validate())) {
      toast.add({ title: t('rules.fixErrors'), color: 'warning' })
      return
    }
    const rule = await api.post<Rule>(`${apiBase.value}/rules`, editor.value.getEnvelope())
    toast.add({ title: t('rules.created'), description: rule.code, color: 'success' })
    await navigateTo(`${base.value}/rules/${rule.id}`)
  }
  catch { /* toast */ }
  finally { saving.value = false }
}
</script>

<template>
  <PagePanel :title="t('actions.newRule')">
    <template #actions>
      <UButton
        :to="`${base}/rules`"
        color="neutral"
        variant="ghost"
        :label="t('actions.cancel')"
      />
      <UButton
        :label="t('rules.saveDraft')"
        icon="i-lucide-save"
        :loading="saving"
        @click="save"
      />
    </template>
    <UTabs
      v-model="tab"
      :items="[{ label: t('actions.edit'), value: 'edit', icon: 'i-lucide-pencil' }, { label: t('actions.test'), value: 'test', icon: 'i-lucide-flask-conical' }, { label: t('actions.backtest'), value: 'backtest', icon: 'i-lucide-history' }]"
      variant="link"
      :content="false"
      class="mb-4"
    />
    <RuleEditor
      v-show="tab === 'edit'"
      ref="editor"
      v-model="envelope"
      is-new
    />
    <RuleTestPanel
      v-if="tab === 'test' && editor"
      :get-envelope="editor.getEnvelope"
    />
    <BacktestPanel
      v-if="tab === 'backtest' && editor"
      :endpoint="`${apiBase}/rules/backtest`"
      :body="() => ({ rule: editor!.getEnvelope() })"
    />
  </PagePanel>
</template>
