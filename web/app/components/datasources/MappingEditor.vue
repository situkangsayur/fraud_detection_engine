<script setup lang="ts">
// Source field → canonical field mapping editor (docs/technical/data-sources.md §3) with confidence badges.
import type { FieldMapping, InferredField, MappingSpec, TransformStep } from '#shared/types/api'
import { CANONICAL_CUSTOMER_FIELDS, CANONICAL_EVENT_FIELDS, EVENT_TYPES, REQUIRED_EVENT_FIELDS } from '#shared/utils/canonical'

const model = defineModel<MappingSpec>({ required: true })
const props = defineProps<{ fields: InferredField[], confidence?: Record<string, number>, disabled?: boolean }>()
const { t } = useI18n()

const sourcePaths = computed(() => props.fields.map(f => f.path))
const piiOf = (path?: string | string[]) => (typeof path === 'string' ? props.fields.find(f => f.path === path)?.pii : undefined)

type Section = 'event' | 'customer'
const showAll = ref(false)
const eventRows = computed(() => CANONICAL_EVENT_FIELDS.filter(f => showAll.value || REQUIRED_EVENT_FIELDS.includes(f as never) || model.value.event[f]))
const customerRows = computed(() => CANONICAL_CUSTOMER_FIELDS.filter(f => showAll.value || model.value.customer?.[f]))

function get(section: Section, target: string): FieldMapping | undefined {
  const s = section === 'event' ? model.value.event : model.value.customer
  return s?.[target] as FieldMapping | undefined
}
function ensure(section: Section, target: string): FieldMapping {
  if (section === 'customer' && !model.value.customer) model.value.customer = {}
  const s = (section === 'event' ? model.value.event : model.value.customer)! as Record<string, FieldMapping>
  if (!s[target]) s[target] = {}
  return s[target]
}
function setFrom(section: Section, target: string, from: string | undefined) {
  if (!from) { clear(section, target); return }
  const m = ensure(section, target)
  delete m.const
  m.from = from
  const pii = piiOf(from)
  if (!m.transform?.length) {
    if (pii === 'pan' && target === 'instrument_fingerprint') m.transform = [{ fn: 'hash_pan' }]
    else if (pii === 'pan' && target === 'card_bin') m.transform = [{ fn: 'pan_bin' }]
    else if (pii === 'pan' && target === 'card_last4') m.transform = [{ fn: 'pan_last4' }]
    else if (pii === 'phone') m.transform = [{ fn: 'normalize_phone', default_country: 'ID' }]
    else if (pii === 'email') m.transform = [{ fn: 'normalize_email' }]
    const f = props.fields.find(x => x.path === from)
    if (f?.inferred_type === 'datetime' && !m.transform?.length) m.transform = [{ fn: 'parse_datetime', format: f.datetime_format ?? 'rfc3339', timezone: 'Asia/Jakarta' }]
  }
  if (pii === 'pan' && !(model.value.drop_fields ?? []).includes(from)) model.value.drop_fields = [...(model.value.drop_fields ?? []), from]
}
function setConst(section: Section, target: string, value: string) {
  const m = ensure(section, target)
  delete m.from
  m.const = value
}
function without<T extends object>(obj: T, key: string): T {
  return Object.fromEntries(Object.entries(obj).filter(([k]) => k !== key)) as T
}
function clear(section: Section, target: string) {
  if (section === 'event') model.value.event = without(model.value.event, target)
  else if (model.value.customer) model.value.customer = without(model.value.customer, target)
}
function conf(key: string) { return props.confidence?.[key] }

const etMode = computed({
  get: () => (model.value.event_type?.from ? 'from' : 'const'),
  set: (m: 'from' | 'const') => { model.value.event_type = m === 'from' ? { from: sourcePaths.value[0], value_map: {}, default: 'transaction' } : { const: 'transaction' } },
})
const valueMapText = ref(JSON.stringify(model.value.event_type?.value_map ?? {}, null, 0))
watch(valueMapText, (v) => {
  try { if (model.value.event_type) model.value.event_type.value_map = JSON.parse(v) }
  catch { /* keep typing */ }
})

const labelOn = computed({
  get: () => !!model.value.label,
  set: (on: boolean) => { model.value.label = on ? { from: props.fields.find(f => /fraud|label|chargeback|penipuan/i.test(f.path))?.path ?? sourcePaths.value[0] ?? '', fraud_values: [1, '1', 'true'] } : null },
})
const fraudValuesText = computed({
  get: () => (model.value.label?.fraud_values ?? []).join(', '),
  set: (v: string) => { if (model.value.label) model.value.label.fraud_values = v.split(',').map(s => s.trim()).filter(Boolean).map(s => (s !== '' && !Number.isNaN(Number(s)) ? Number(s) : s)) },
})
</script>

<template>
  <div class="space-y-6">
    <!-- event type -->
    <section>
      <h4 class="font-medium text-sm mb-2">
        {{ t('datasources.eventType') }}
        <UBadge
          v-if="conf('event_type') !== undefined"
          :color="(conf('event_type') ?? 0) > 0.8 ? 'success' : 'warning'"
          variant="soft"
          size="xs"
        >
          {{ fmtPct(conf('event_type'), 0) }}
        </UBadge>
      </h4>
      <div class="flex flex-wrap items-center gap-2">
        <USelect
          v-model="etMode"
          :items="[{ label: t('datasources.constant'), value: 'const' }, { label: t('datasources.fromField'), value: 'from' }]"
          class="w-36"
          size="sm"
          :disabled="disabled"
        />
        <USelect
          v-if="etMode === 'const' && model.event_type"
          v-model="(model.event_type.const as string)"
          :items="[...EVENT_TYPES]"
          class="w-48"
          size="sm"
          :disabled="disabled"
        />
        <template v-else-if="model.event_type">
          <USelect
            v-model="(model.event_type.from as string)"
            :items="sourcePaths"
            class="w-48 fp-mono"
            size="sm"
            :disabled="disabled"
          />
          <UInput
            v-model="valueMapText"
            class="w-72 fp-mono"
            size="sm"
            placeholder="{&quot;PURCHASE&quot;:&quot;transaction&quot;}"
            :disabled="disabled"
          />
          <USelect
            v-model="(model.event_type.default as string)"
            :items="[...EVENT_TYPES]"
            class="w-40"
            size="sm"
            :disabled="disabled"
          />
        </template>
      </div>
    </section>

    <!-- event fields -->
    <section>
      <div class="flex items-center justify-between mb-2">
        <h4 class="font-medium text-sm">
          {{ t('datasources.eventFields') }}
        </h4>
        <USwitch
          v-model="showAll"
          size="xs"
          :label="t('datasources.showAllFields')"
        />
      </div>
      <div class="divide-y divide-default rounded-md border border-default">
        <div
          v-for="target in eventRows"
          :key="target"
          class="p-2 grid lg:grid-cols-[14rem_16rem_1fr] gap-2 items-start"
        >
          <div class="flex items-center gap-1.5">
            <span class="fp-mono text-xs">event.{{ target }}</span>
            <UBadge
              v-if="REQUIRED_EVENT_FIELDS.includes(target as never)"
              color="error"
              variant="soft"
              size="xs"
            >
              {{ t('common.required') }}
            </UBadge>
            <UBadge
              v-if="conf(`event.${target}`) !== undefined"
              :color="(conf(`event.${target}`) ?? 0) > 0.8 ? 'success' : 'warning'"
              variant="soft"
              size="xs"
            >
              {{ fmtPct(conf(`event.${target}`), 0) }}
            </UBadge>
          </div>
          <div class="flex gap-1">
            <USelectMenu
              :model-value="(get('event', target)?.from as string) ?? undefined"
              :items="sourcePaths"
              :placeholder="get('event', target)?.const !== undefined ? `= ${get('event', target)?.const}` : t('datasources.pickSourceField')"
              class="flex-1 fp-mono"
              size="sm"
              :disabled="disabled"
              @update:model-value="(v: string) => setFrom('event', target, v)"
            />
            <UPopover v-if="!disabled">
              <UButton
                icon="i-lucide-equal"
                size="xs"
                color="neutral"
                variant="ghost"
                :aria-label="t('datasources.constant')"
              />
              <template #content>
                <div class="p-2 w-56">
                  <UInput
                    :model-value="String(get('event', target)?.const ?? '')"
                    size="sm"
                    :placeholder="t('datasources.constant')"
                    @update:model-value="(v: string | number) => setConst('event', target, String(v))"
                  />
                </div>
              </template>
            </UPopover>
            <UButton
              v-if="get('event', target) && !disabled"
              icon="i-lucide-x"
              size="xs"
              color="neutral"
              variant="ghost"
              :aria-label="t('actions.clear')"
              @click="clear('event', target)"
            />
          </div>
          <TransformEditor
            v-if="get('event', target)"
            :model-value="get('event', target)!.transform ?? []"
            :disabled="disabled"
            @update:model-value="(v: TransformStep[]) => (ensure('event', target).transform = v)"
          />
        </div>
      </div>
    </section>

    <!-- customer fields -->
    <section>
      <h4 class="font-medium text-sm mb-2">
        {{ t('datasources.customerFields') }}
      </h4>
      <div class="divide-y divide-default rounded-md border border-default">
        <div
          v-for="target in customerRows"
          :key="target"
          class="p-2 grid lg:grid-cols-[14rem_16rem_1fr] gap-2 items-start"
        >
          <div class="flex items-center gap-1.5">
            <span class="fp-mono text-xs">customer.{{ target }}</span>
            <UBadge
              v-if="conf(`customer.${target}`) !== undefined"
              :color="(conf(`customer.${target}`) ?? 0) > 0.8 ? 'success' : 'warning'"
              variant="soft"
              size="xs"
            >
              {{ fmtPct(conf(`customer.${target}`), 0) }}
            </UBadge>
          </div>
          <USelectMenu
            :model-value="(get('customer', target)?.from as string) ?? undefined"
            :items="sourcePaths"
            :placeholder="t('datasources.pickSourceField')"
            class="fp-mono"
            size="sm"
            :disabled="disabled"
            @update:model-value="(v: string) => setFrom('customer', target, v)"
          />
          <TransformEditor
            v-if="get('customer', target)"
            :model-value="get('customer', target)!.transform ?? []"
            :disabled="disabled"
            @update:model-value="(v: TransformStep[]) => (ensure('customer', target).transform = v)"
          />
        </div>
        <p
          v-if="!customerRows.length"
          class="p-2 text-xs text-muted"
        >
          {{ t('datasources.noCustomerFields') }}
        </p>
      </div>
    </section>

    <!-- label & drop -->
    <section class="grid lg:grid-cols-2 gap-4">
      <div class="rounded-md border border-default p-3 space-y-2">
        <USwitch
          v-model="labelOn"
          :label="t('datasources.labelSection')"
          :description="t('datasources.labelHelp')"
          :disabled="disabled"
        />
        <div
          v-if="model.label"
          class="flex flex-wrap gap-2"
        >
          <USelect
            v-model="model.label.from"
            :items="sourcePaths"
            class="w-48 fp-mono"
            size="sm"
            :disabled="disabled"
          />
          <UInput
            v-model="fraudValuesText"
            class="w-48 fp-mono"
            size="sm"
            :placeholder="t('datasources.fraudValues')"
            :disabled="disabled"
          />
        </div>
      </div>
      <div class="rounded-md border border-default p-3 space-y-2">
        <p class="text-sm font-medium">
          {{ t('datasources.dropFields') }}
        </p>
        <p class="text-xs text-muted">
          {{ t('datasources.dropFieldsHelp') }}
        </p>
        <USelectMenu
          v-model="model.drop_fields"
          :items="sourcePaths"
          multiple
          class="w-full fp-mono"
          size="sm"
          :disabled="disabled"
        />
      </div>
    </section>
  </div>
</template>
