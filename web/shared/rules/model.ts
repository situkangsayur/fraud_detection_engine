// Rule-builder UI model ⇄ rule DSL JSON.
//
// The editor works on a tree with stable `uid`s (needed for keyed v-for rendering and drag/drop) and formula args as an
// ordered array. `definitionFromDsl` / `definitionToDsl` convert losslessly: every key the UI does not model
// explicitly is copied through untouched, so rules written by the API, the LLM or a newer DSL version survive an
// edit round-trip. Unit-tested in tests/unit/rule-model.test.ts against the examples in rule-dsl.md.

import type {
  CompareT,
  Condition,
  ConditionT,
  Operand,
  Operator,
  RuleDefinition,
  RuleDefinitionT,
  RuleEnvelope,
  RuleEnvelopeT,
  RuleKind,
  StatisticT,
  VelocityBodyT,
} from './dsl'
import { isAll, isAny, isAtLeast, isLeaf, isNot, UNARY_OPERATORS } from './dsl'

let uidCounter = 0
export function uid(prefix = 'n'): string {
  uidCounter += 1
  return `${prefix}${uidCounter.toString(36)}`
}

// ---------------------------------------------------------------- UI model types
export interface FormulaArgModel { uid: string, name: string, operand: OperandModel }

export type OperandModel
  = | { uid: string, type: 'const', value: unknown }
    | { uid: string, type: 'field', path: string }
    | { uid: string, type: 'ref', path: string }
    | { uid: string, type: 'hist', path: string }
    | { uid: string, type: 'formula', expr: string, args: FormulaArgModel[] }

export interface LeafModel {
  uid: string
  type: 'leaf'
  left: OperandModel
  op: Operator
  right?: OperandModel
  weight?: number
  options?: Record<string, unknown>
}

export interface GroupModel {
  uid: string
  type: 'all' | 'any'
  children: ConditionModel[]
}

export interface NotModel {
  uid: string
  type: 'not'
  child: ConditionModel
}

export interface AtLeastModel {
  uid: string
  type: 'at_least'
  n: number
  children: ConditionModel[]
}

export type ConditionModel = LeafModel | GroupModel | NotModel | AtLeastModel

export type RuleDefinitionModel = RuleDefinitionT<OperandModel, ConditionModel>
export type RuleEnvelopeModel = RuleEnvelopeT<RuleDefinitionModel>

// ---------------------------------------------------------------- helpers
function clone<T>(v: T): T {
  return v === undefined ? v : JSON.parse(JSON.stringify(v)) as T
}

// ---------------------------------------------------------------- operands
export function operandFromDsl(o: Operand): OperandModel {
  switch (o.type) {
    case 'const':
      return { uid: uid('o'), type: 'const', value: clone(o.value) }
    case 'field':
    case 'ref':
    case 'hist':
      return { uid: uid('o'), type: o.type, path: o.path }
    case 'formula':
      return {
        uid: uid('o'),
        type: 'formula',
        expr: o.expr,
        args: Object.entries(o.args ?? {}).map(([name, operand]) => ({ uid: uid('a'), name, operand: operandFromDsl(operand) })),
      }
  }
}

export function operandToDsl(o: OperandModel): Operand {
  switch (o.type) {
    case 'const':
      return { type: 'const', value: clone(o.value) }
    case 'field':
    case 'ref':
    case 'hist':
      return { type: o.type, path: o.path }
    case 'formula': {
      const args: Record<string, Operand> = {}
      for (const a of o.args) args[a.name] = operandToDsl(a.operand)
      return { type: 'formula', expr: o.expr, args }
    }
  }
}

export function newOperand(type: OperandModel['type'] = 'field'): OperandModel {
  switch (type) {
    case 'const': return { uid: uid('o'), type, value: 0 }
    case 'formula': return { uid: uid('o'), type, expr: 'F(x) = x', args: [{ uid: uid('a'), name: 'x', operand: newOperand('field') }] }
    default: return { uid: uid('o'), type, path: '' }
  }
}

// ---------------------------------------------------------------- conditions
export function conditionFromDsl(c: Condition): ConditionModel {
  if (isLeaf(c)) {
    const leaf: LeafModel = { uid: uid('c'), type: 'leaf', left: operandFromDsl(c.left), op: c.op }
    if (c.right !== undefined) leaf.right = operandFromDsl(c.right)
    if (c.weight !== undefined) leaf.weight = c.weight
    if (c.options !== undefined) leaf.options = clone(c.options)
    return leaf
  }
  if (isAll(c)) return { uid: uid('c'), type: 'all', children: c.all.map(conditionFromDsl) }
  if (isAny(c)) return { uid: uid('c'), type: 'any', children: c.any.map(conditionFromDsl) }
  if (isNot(c)) return { uid: uid('c'), type: 'not', child: conditionFromDsl(c.not) }
  if (isAtLeast(c)) return { uid: uid('c'), type: 'at_least', n: c.at_least.n, children: c.at_least.of.map(conditionFromDsl) }
  throw new Error(`Unknown condition shape: ${JSON.stringify(c)}`)
}

export function conditionToDsl(c: ConditionModel): Condition {
  switch (c.type) {
    case 'leaf': {
      const out: ConditionT<Operand> = { left: operandToDsl(c.left), op: c.op }
      if (c.right !== undefined && !UNARY_OPERATORS.includes(c.op)) out.right = operandToDsl(c.right)
      if (c.weight !== undefined) out.weight = c.weight
      if (c.options !== undefined) out.options = clone(c.options)
      return out
    }
    case 'all': return { all: c.children.map(conditionToDsl) }
    case 'any': return { any: c.children.map(conditionToDsl) }
    case 'not': return { not: conditionToDsl(c.child) }
    case 'at_least': return { at_least: { n: c.n, of: c.children.map(conditionToDsl) } }
  }
}

export function newLeaf(): LeafModel {
  return {
    uid: uid('c'),
    type: 'leaf',
    left: { uid: uid('o'), type: 'field', path: 'event.amount' },
    op: 'gt',
    right: { uid: uid('o'), type: 'const', value: 0 },
  }
}

export function newGroup(type: GroupModel['type'] = 'all'): GroupModel {
  return { uid: uid('c'), type, children: [newLeaf()] }
}

// ---------------------------------------------------------------- velocity pieces
function compareFromDsl(c: CompareT<Operand>): CompareT<OperandModel> {
  return { ...clone(c), right: operandFromDsl(c.right) } as CompareT<OperandModel>
}
function compareToDsl(c: CompareT<OperandModel>): CompareT<Operand> {
  return { ...clone({ ...c, right: undefined }), op: c.op, right: operandToDsl(c.right) }
}

function statisticFromDsl(s: StatisticT<Operand>): StatisticT<OperandModel> {
  const out = clone({ ...s, of: undefined }) as StatisticT<OperandModel>
  delete out.of
  if (s.of !== undefined) out.of = operandFromDsl(s.of)
  return out
}
function statisticToDsl(s: StatisticT<OperandModel>): StatisticT<Operand> {
  const out = clone({ ...s, of: undefined }) as StatisticT<Operand>
  delete out.of
  if (s.of !== undefined) out.of = operandToDsl(s.of)
  return out
}

function velocityFromDsl(v: VelocityBodyT<Operand>): VelocityBodyT<OperandModel> {
  const out = clone({ ...v, compare: undefined, statistic: undefined }) as unknown as VelocityBodyT<OperandModel>
  out.compare = compareFromDsl(v.compare)
  if ('statistic' in v) out.statistic = v.statistic ? statisticFromDsl(v.statistic) : v.statistic
  else delete out.statistic
  return out
}
function velocityToDsl(v: VelocityBodyT<OperandModel>): VelocityBodyT<Operand> {
  const out = clone({ ...v, compare: undefined, statistic: undefined }) as unknown as VelocityBodyT<Operand>
  out.compare = compareToDsl(v.compare)
  if ('statistic' in v) out.statistic = v.statistic ? statisticToDsl(v.statistic) : v.statistic
  else delete out.statistic
  return out
}

/** Copies every key except the ones listed (which the caller converts), preserving unknown keys. */
function rest<T extends object>(obj: T, omit: string[]): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(obj)) if (!omit.includes(k)) out[k] = clone(v)
  return out
}

// ---------------------------------------------------------------- definitions
export function definitionFromDsl(d: RuleDefinition): RuleDefinitionModel {
  switch (d.kind) {
    case 'simple':
      return { ...rest(d, ['when']), kind: 'simple', when: conditionFromDsl(d.when) } as RuleDefinitionModel
    case 'velocity': {
      const { kind: _kind, ...body } = d
      return { ...velocityFromDsl(body), kind: 'velocity' }
    }
    case 'composite': {
      const out = { ...rest(d, ['gate', 'history_filter', 'velocity']), kind: 'composite', velocity: velocityFromDsl(d.velocity) } as Extract<RuleDefinitionModel, { kind: 'composite' }>
      if (d.gate !== undefined) out.gate = conditionFromDsl(d.gate)
      if (d.history_filter !== undefined) out.history_filter = conditionFromDsl(d.history_filter)
      return out
    }
    case 'reference': {
      const out = { ...rest(d, ['key', 'attribute_condition']), kind: 'reference', key: operandFromDsl(d.key) } as Extract<RuleDefinitionModel, { kind: 'reference' }>
      if (d.attribute_condition !== undefined) out.attribute_condition = conditionFromDsl(d.attribute_condition)
      return out
    }
    case 'graph':
      return { ...rest(d, ['compare']), kind: 'graph', compare: compareFromDsl(d.compare) } as RuleDefinitionModel
  }
}

export function definitionToDsl(d: RuleDefinitionModel): RuleDefinition {
  switch (d.kind) {
    case 'simple':
      return { ...rest(d, ['when']), kind: 'simple', when: conditionToDsl(d.when) } as RuleDefinition
    case 'velocity': {
      const { kind: _kind, ...body } = d
      return { ...velocityToDsl(body), kind: 'velocity' }
    }
    case 'composite': {
      const out = { ...rest(d, ['gate', 'history_filter', 'velocity']), kind: 'composite', velocity: velocityToDsl(d.velocity) } as Extract<RuleDefinition, { kind: 'composite' }>
      if (d.gate !== undefined) out.gate = conditionToDsl(d.gate)
      if (d.history_filter !== undefined) out.history_filter = conditionToDsl(d.history_filter)
      return out
    }
    case 'reference': {
      const out = { ...rest(d, ['key', 'attribute_condition']), kind: 'reference', key: operandToDsl(d.key) } as Extract<RuleDefinition, { kind: 'reference' }>
      if (d.attribute_condition !== undefined && d.mode === 'attribute') out.attribute_condition = conditionToDsl(d.attribute_condition)
      return out
    }
    case 'graph':
      return { ...rest(d, ['compare']), kind: 'graph', compare: compareToDsl(d.compare) } as RuleDefinition
  }
}

export function envelopeFromDsl(e: RuleEnvelope): RuleEnvelopeModel {
  return { ...clone({ ...e, definition: undefined }), definition: definitionFromDsl(e.definition) } as RuleEnvelopeModel
}

export function envelopeToDsl(e: RuleEnvelopeModel): RuleEnvelope {
  const out = { ...clone({ ...e, definition: undefined }), definition: definitionToDsl(e.definition) } as RuleEnvelope
  out.kind = out.definition.kind
  return out
}

// ---------------------------------------------------------------- defaults for "new rule"
function defaultVelocity(): VelocityBodyT<OperandModel> {
  return {
    history_event_types: ['transaction'],
    group_by: ['customer_id'],
    window: { duration: '24h' },
    aggregate: { fn: 'count' },
    statistic: null,
    include_current: true,
    min_samples: 1,
    compare: { op: 'gte', right: { uid: uid('o'), type: 'const', value: 5 } },
  }
}

export function defaultDefinition(kind: RuleKind): RuleDefinitionModel {
  switch (kind) {
    case 'simple':
      return { kind, when: newGroup('all'), scoring: 'binary' }
    case 'velocity':
      return { kind, ...defaultVelocity() }
    case 'composite':
      return {
        kind,
        gate: newGroup('all'),
        history_filter: {
          uid: uid('c'),
          type: 'all',
          children: [{
            uid: uid('c'),
            type: 'leaf',
            left: { uid: uid('o'), type: 'hist', path: 'amount' },
            op: 'gt',
            right: { uid: uid('o'), type: 'const', value: 0 },
          }],
        },
        velocity: defaultVelocity(),
      }
    case 'reference':
      return { kind, list: '', key: { uid: uid('o'), type: 'field', path: 'event.instrument_fingerprint' }, mode: 'exists' }
    case 'graph':
      return {
        kind,
        metric: 'distance_to_fraud',
        link_kinds: ['phone', 'card', 'device', 'address', 'email', 'bank_account', 'ref_transaction'],
        include_similar: true,
        max_depth: 3,
        compare: { op: 'lte', right: { uid: uid('o'), type: 'const', value: 2 } },
      }
  }
}

export function defaultEnvelope(kind: RuleKind = 'simple'): RuleEnvelopeModel {
  return {
    code: '',
    name: '',
    description: '',
    kind,
    typologies: [],
    event_types: ['transaction'],
    risk_score: 30,
    trapped_score: 0,
    action: 'score',
    on_trapped: 'ignore',
    missing_as_no_match: false,
    definition: defaultDefinition(kind),
  }
}

// ---------------------------------------------------------------- summaries (list/detail views)
const OP_SYMBOL: Partial<Record<Operator, string>> = { eq: '=', ne: '≠', gt: '>', gte: '≥', lt: '<', lte: '≤' }

export function describeOperand(o: Operand | undefined): string {
  if (!o) return ''
  switch (o.type) {
    case 'const': return typeof o.value === 'string' ? `"${o.value}"` : JSON.stringify(o.value)
    case 'field': return o.path
    case 'ref': return `ref.${o.path}`
    case 'hist': return `hist.${o.path}`
    case 'formula': return o.expr
  }
}

export function describeCondition(c: Condition): string {
  if (isLeaf(c)) {
    const op = OP_SYMBOL[c.op] ?? c.op
    return UNARY_OPERATORS.includes(c.op) ? `${describeOperand(c.left)} ${op}` : `${describeOperand(c.left)} ${op} ${describeOperand(c.right)}`
  }
  if (isAll(c)) return c.all.map(describeCondition).map(s => `(${s})`).join(' AND ')
  if (isAny(c)) return c.any.map(describeCondition).map(s => `(${s})`).join(' OR ')
  if (isNot(c)) return `NOT (${describeCondition(c.not)})`
  if (isAtLeast(c)) return `AT LEAST ${c.at_least.n} OF [${c.at_least.of.map(describeCondition).join('; ')}]`
  return '?'
}

export function describeDefinition(d: RuleDefinition): string {
  switch (d.kind) {
    case 'simple': return describeCondition(d.when)
    case 'velocity': {
      const w = 'duration' in d.window ? d.window.duration : `last ${d.window.last_n}`
      const agg = `${d.aggregate.fn}(${d.aggregate.field ?? '*'})`
      const stat = d.statistic ? `${d.statistic.fn}(${agg})` : agg
      return `${stat} by ${d.group_by.join(', ')} over ${w} ${OP_SYMBOL[d.compare.op] ?? d.compare.op} ${describeOperand(d.compare.right)}`
    }
    case 'composite': {
      const v = d.velocity
      const w = 'duration' in v.window ? v.window.duration : `last ${v.window.last_n}`
      const filter = d.history_filter ? ` where ${describeCondition(d.history_filter)}` : ''
      return `${v.aggregate.fn}(${v.aggregate.field ?? '*'}) by ${v.group_by.join(', ')} over ${w}${filter} ${OP_SYMBOL[v.compare.op] ?? v.compare.op} ${describeOperand(v.compare.right)}`
    }
    case 'reference':
      return `${describeOperand(d.key)} ${d.mode === 'not_exists' ? '∉' : '∈'} ${d.list}${d.mode === 'attribute' && d.attribute_condition ? ` where ${describeCondition(d.attribute_condition)}` : ''}`
    case 'graph':
      return `graph.${d.metric} (depth ${d.max_depth ?? 3}) ${OP_SYMBOL[d.compare.op] ?? d.compare.op} ${describeOperand(d.compare.right)}`
  }
}

/** All `field` paths referenced anywhere in a definition (for highlighting / catalog checks). */
export function collectFieldPaths(d: RuleDefinition): string[] {
  const paths = new Set<string>()
  const visitOperand = (o: Operand | undefined) => {
    if (!o) return
    if (o.type === 'field') paths.add(o.path)
    if (o.type === 'formula') Object.values(o.args).forEach(visitOperand)
  }
  const visitCondition = (c: Condition | undefined) => {
    if (!c) return
    if (isLeaf(c)) { visitOperand(c.left); visitOperand(c.right) }
    else if (isAll(c)) c.all.forEach(visitCondition)
    else if (isAny(c)) c.any.forEach(visitCondition)
    else if (isNot(c)) visitCondition(c.not)
    else if (isAtLeast(c)) c.at_least.of.forEach(visitCondition)
  }
  const visitVelocity = (v: VelocityBodyT<Operand>) => { visitOperand(v.compare.right); visitOperand(v.statistic?.of) }
  switch (d.kind) {
    case 'simple': visitCondition(d.when); break
    case 'velocity': visitVelocity(d); break
    case 'composite': visitCondition(d.gate); visitCondition(d.history_filter); visitVelocity(d.velocity); break
    case 'reference': visitOperand(d.key); visitCondition(d.attribute_condition); break
    case 'graph': visitOperand(d.compare.right); break
  }
  return [...paths].sort()
}

// ---------------------------------------------------------------- client-side checks (server validation is authoritative)
export interface LocalIssue { path: string, message: string }

export function localValidate(e: RuleEnvelope): LocalIssue[] {
  const issues: LocalIssue[] = []
  if (!/^[A-Z0-9-]{3,40}$/.test(e.code)) issues.push({ path: 'code', message: 'code must match ^[A-Z0-9-]{3,40}$' })
  if (!e.name.trim()) issues.push({ path: 'name', message: 'name is required' })
  if (!(e.risk_score >= 0 && e.risk_score <= 100)) issues.push({ path: 'risk_score', message: 'risk_score must be 0–100' })
  if (e.trapped_score !== undefined && !(e.trapped_score >= 0 && e.trapped_score <= 100)) issues.push({ path: 'trapped_score', message: 'trapped_score must be 0–100' })
  const d = e.definition
  const checkCondition = (c: Condition, path: string) => {
    if (isLeaf(c)) {
      if (!UNARY_OPERATORS.includes(c.op) && c.right === undefined) issues.push({ path, message: `operator ${c.op} needs a right operand` })
      if (c.left.type === 'field' && !c.left.path) issues.push({ path: `${path}.left`, message: 'field path is empty' })
      if (c.right?.type === 'field' && !c.right.path) issues.push({ path: `${path}.right`, message: 'field path is empty' })
    }
    else if (isAll(c)) { if (!c.all.length) issues.push({ path, message: 'empty ALL group' }); c.all.forEach((x, i) => checkCondition(x, `${path}.all[${i}]`)) }
    else if (isAny(c)) { if (!c.any.length) issues.push({ path, message: 'empty ANY group' }); c.any.forEach((x, i) => checkCondition(x, `${path}.any[${i}]`)) }
    else if (isNot(c)) checkCondition(c.not, `${path}.not`)
    else if (isAtLeast(c)) {
      if (c.at_least.n < 1 || c.at_least.n > c.at_least.of.length) issues.push({ path, message: 'at_least.n must be between 1 and the number of conditions' })
      c.at_least.of.forEach((x, i) => checkCondition(x, `${path}.at_least.of[${i}]`))
    }
  }
  const checkVelocity = (v: VelocityBodyT<Operand>, path: string) => {
    if (!v.group_by.length) issues.push({ path: `${path}.group_by`, message: 'group_by needs at least one field' })
    if ('duration' in v.window && !/^\d+(s|m|h|d|w)$/.test(v.window.duration)) issues.push({ path: `${path}.window`, message: 'duration must look like 30m, 24h, 7d' })
    if (v.aggregate.fn !== 'count' && !v.aggregate.field) issues.push({ path: `${path}.aggregate.field`, message: `${v.aggregate.fn} needs a field` })
  }
  switch (d.kind) {
    case 'simple': checkCondition(d.when, 'definition.when'); break
    case 'velocity': checkVelocity(d, 'definition'); break
    case 'composite':
      if (d.gate) checkCondition(d.gate, 'definition.gate')
      if (d.history_filter) checkCondition(d.history_filter, 'definition.history_filter')
      checkVelocity(d.velocity, 'definition.velocity')
      break
    case 'reference':
      if (!d.list) issues.push({ path: 'definition.list', message: 'choose a reference list' })
      if (d.mode === 'attribute' && !d.attribute_condition) issues.push({ path: 'definition.attribute_condition', message: 'attribute mode needs a condition' })
      break
    case 'graph':
      if (d.max_depth !== undefined && (d.max_depth < 1 || d.max_depth > 4)) issues.push({ path: 'definition.max_depth', message: 'max_depth must be 1–4' })
      break
  }
  return issues
}
