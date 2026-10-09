<script setup lang="ts">
// Formula playground (rule-dsl §3.1): evaluates on the server (rule-service) — the exact same engine as rules.
import type { FormulaResult } from '#shared/types/api'
import { ApiError } from '~/composables/useApi'

const { t } = useI18n()
const api = useApi()
const { apiBase } = useProject()
useHead({ title: () => t('nav.formulas') })

const EXAMPLES = [
  { expr: 'F(x,y,z) = 2x + 2^y / z^2', vars: { x: 10, y: 3, z: 2 } },
  { expr: 'F(amount, avg) = (amount - avg) / max(avg, 1)', vars: { amount: 5_000_000, avg: 750_000 } },
  { expr: 'F(x, mu, s) = 1 - normcdf(x, mu, s)', vars: { x: 9, mu: 3, s: 2 } },
  { expr: 'F(d, a) = if(d, sigmoid(a / 1000000), 0)', vars: { d: 1, a: 3_000_000 } },
]
const expr = ref(EXAMPLES[0]!.expr)
const vars = ref<{ name: string, value: string }[]>(Object.entries(EXAMPLES[0]!.vars).map(([name, value]) => ({ name, value: String(value) })))
const result = ref<FormulaResult | null>(null)
const parseError = ref<{ message: string, position: number | null } | null>(null)
const busy = ref(false)

function detectVars() {
  const header = /^\s*[A-Za-z_]\w*\s*\(([^)]*)\)\s*=/.exec(expr.value)
  const names = header ? header[1]!.split(',').map(s => s.trim()).filter(Boolean) : []
  vars.value = names.map(n => vars.value.find(v => v.name === n) ?? { name: n, value: '0' })
}
function loadExample(i: number) {
  const ex = EXAMPLES[i]!
  expr.value = ex.expr
  vars.value = Object.entries(ex.vars).map(([name, value]) => ({ name, value: String(value) }))
  result.value = null
  parseError.value = null
}
async function evaluate() {
  busy.value = true
  parseError.value = null
  result.value = null
  try {
    const variables = Object.fromEntries(vars.value.map(v => [v.name, v.value === '' || v.value === 'null' ? null : v.value === 'true' ? true : v.value === 'false' ? false : Number(v.value)]))
    result.value = await api.post<FormulaResult>(`${apiBase.value}/formulas/evaluate`, { expr: expr.value, variables }, { silent: true })
  }
  catch (err) {
    if (err instanceof ApiError) {
      const e = err.problem.errors.find(x => x.field === 'expr')
      const pos = /position (\d+)/.exec(e?.message ?? err.problem.detail ?? '')
      const position = typeof err.problem.extra?.position === 'number' ? err.problem.extra.position : pos ? Number(pos[1]) : null
      parseError.value = { message: String(err.problem.extra?.message ?? err.problem.detail ?? e?.message ?? err.message), position }
    }
  }
  finally { busy.value = false }
}
</script>

<template>
  <PagePanel
    :title="t('nav.formulas')"
    :description="t('formulas.description')"
  >
    <div class="grid xl:grid-cols-[1fr_22rem] gap-4">
      <UCard>
        <div class="space-y-4">
          <UFormField
            :label="t('formulas.expression')"
            :help="t('formulas.syntaxHelp')"
          >
            <UInput
              v-model="expr"
              class="w-full fp-mono"
              size="lg"
              @blur="detectVars"
              @keydown.enter="evaluate"
            />
          </UFormField>
          <div
            v-if="parseError"
            class="rounded-md border border-error/40 bg-error/5 p-3"
          >
            <p class="text-sm text-error">
              {{ parseError.message }}
            </p>
            <pre
              v-if="parseError.position !== null"
              class="fp-mono text-xs mt-1"
            >{{ expr }}
            {{ ' '.repeat(parseError.position) }}^</pre>
          </div>
          <div>
            <p class="text-sm font-medium mb-2">
              {{ t('formulas.variables') }}
            </p>
            <div class="grid sm:grid-cols-2 lg:grid-cols-3 gap-2">
              <div
                v-for="(v, i) in vars"
                :key="i"
                class="flex items-center gap-2"
              >
                <UInput
                  v-model="v.name"
                  class="w-20 fp-mono"
                  size="sm"
                />
                <span>=</span>
                <UInput
                  v-model="v.value"
                  class="flex-1 fp-mono"
                  size="sm"
                />
                <UButton
                  icon="i-lucide-x"
                  size="xs"
                  color="neutral"
                  variant="ghost"
                  :aria-label="t('actions.remove')"
                  @click="vars.splice(i, 1)"
                />
              </div>
            </div>
            <UButton
              size="xs"
              variant="link"
              icon="i-lucide-plus"
              :label="t('formulas.addVariable')"
              @click="vars.push({ name: `v${vars.length + 1}`, value: '0' })"
            />
          </div>
          <UButton
            :label="t('formulas.evaluate')"
            icon="i-lucide-play"
            :loading="busy"
            @click="evaluate"
          />
          <div
            v-if="result"
            class="rounded-md p-4"
            :class="!result.trapped ? 'bg-success/10' : 'bg-warning/10'"
          >
            <p
              v-if="!result.trapped"
              class="text-2xl font-semibold tabular-nums fp-mono"
            >
              = {{ result.value }}
            </p>
            <template v-else>
              <StatusBadge value="trapped" />
              <p class="text-sm mt-1">
                {{ result.reason }}
              </p>
            </template>
          </div>
        </div>
      </UCard>
      <UCard>
        <template #header>
          <h3 class="font-medium">
            {{ t('formulas.examples') }}
          </h3>
        </template>
        <ul class="space-y-2">
          <li
            v-for="(ex, i) in EXAMPLES"
            :key="i"
          >
            <UButton
              variant="ghost"
              color="neutral"
              class="fp-mono text-xs text-left w-full"
              @click="loadExample(i)"
            >
              {{ ex.expr }}
            </UButton>
          </li>
        </ul>
        <USeparator class="my-3" />
        <p class="text-xs text-muted">
          {{ t('formulas.functions') }}: abs, sqrt, ln, log10, log(x,b), exp, pow, min, max, floor, ceil, round, clamp, sigmoid, gauss, normcdf, if · pi, e
        </p>
      </UCard>
    </div>
  </PagePanel>
</template>
