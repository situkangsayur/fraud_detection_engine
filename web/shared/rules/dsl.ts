// Rule DSL v1 — TypeScript mirror of docs/technical/rule-dsl.md.
//
// The definition types are generic over the operand (O) and condition (C) representation, so the same shapes describe
// both the wire JSON (`Operand`/`Condition`) and the rule-builder UI model (`OperandModel`/`ConditionModel`, see
// model.ts), which adds stable ids for Vue list rendering.

export type RuleKind = 'simple' | 'velocity' | 'composite' | 'reference' | 'graph'
export type RuleStatus = 'draft' | 'pending_approval' | 'active' | 'shadow' | 'retired'
export type RuleAction = 'score' | 'force_review' | 'force_decline' | 'force_approve'
export type OnTrapped = 'ignore' | 'score' | 'review'

export const RULE_KINDS: RuleKind[] = ['simple', 'velocity', 'composite', 'reference', 'graph']
export const RULE_ACTIONS: RuleAction[] = ['score', 'force_review', 'force_decline', 'force_approve']
export const ON_TRAPPED: OnTrapped[] = ['ignore', 'score', 'review']

// ---------------------------------------------------------------- operands
export interface ConstOperand { type: 'const', value: unknown }
export interface FieldOperand { type: 'field', path: string }
export interface RefOperand { type: 'ref', path: string }
export interface HistOperand { type: 'hist', path: string }
export interface FormulaOperandT<O> { type: 'formula', expr: string, args: Record<string, O> }

export type OperandT<O> = ConstOperand | FieldOperand | RefOperand | HistOperand | FormulaOperandT<O>
// Declared via an interface so the recursive reference (formula args are operands) is legal.
export interface FormulaOperand { type: 'formula', expr: string, args: Record<string, Operand> }
export type Operand = ConstOperand | FieldOperand | RefOperand | HistOperand | FormulaOperand
export type OperandType = Operand['type']

// ---------------------------------------------------------------- conditions
export type Operator
  = | 'eq' | 'ne' | 'gt' | 'gte' | 'lt' | 'lte' | 'between' | 'in' | 'not_in'
    | 'contains' | 'not_contains' | 'starts_with' | 'ends_with' | 'regex'
    | 'is_null' | 'is_not_null' | 'similar'

export const OPERATORS: Operator[] = [
  'eq', 'ne', 'gt', 'gte', 'lt', 'lte', 'between', 'in', 'not_in',
  'contains', 'not_contains', 'starts_with', 'ends_with', 'regex', 'is_null', 'is_not_null', 'similar',
]

/** Operators that take no right-hand operand. */
export const UNARY_OPERATORS: Operator[] = ['is_null', 'is_not_null']

/** Operators allowed in composite.history_filter (compiled to SQL). */
export const SQL_OPERATORS: Operator[] = ['eq', 'ne', 'gt', 'gte', 'lt', 'lte', 'between', 'in', 'not_in', 'is_null', 'is_not_null', 'starts_with', 'contains']

export interface LeafConditionT<O> {
  left: O
  op: Operator
  right?: O
  weight?: number
  options?: Record<string, unknown>
}

export type ConditionT<O>
  = | LeafConditionT<O>
    | { all: ConditionT<O>[] }
    | { any: ConditionT<O>[] }
    | { not: ConditionT<O> }
    | { at_least: { n: number, of: ConditionT<O>[] } }

export type Condition = ConditionT<Operand>
export type LeafCondition = LeafConditionT<Operand>

// ---------------------------------------------------------------- velocity & statistics
export type AggregateFn = 'count' | 'sum' | 'avg' | 'min' | 'max' | 'distinct_count' | 'stddev' | 'median' | 'percentile'
export const AGGREGATE_FNS: AggregateFn[] = ['count', 'sum', 'avg', 'min', 'max', 'distinct_count', 'stddev', 'median', 'percentile']

export type StatisticFn = 'zscore' | 'gaussian_tail' | 'percentile_rank' | 'linear_trend' | 'poisson_tail'
export const STATISTIC_FNS: StatisticFn[] = ['zscore', 'gaussian_tail', 'percentile_rank', 'linear_trend', 'poisson_tail']

export interface StatisticT<O> {
  fn: StatisticFn
  of?: O
  tail?: 'upper' | 'lower' | 'two'
  bucket?: string
  output?: 'slope' | 'forecast' | 'residual_z'
  [extra: string]: unknown
}

export type VelocityWindow = { duration: string } | { last_n: number }

export interface CompareT<O> { op: Operator, right: O }

export interface VelocityBodyT<O> {
  history_event_types?: string[]
  group_by: string[]
  window: VelocityWindow
  aggregate: { fn: AggregateFn, field?: string, p?: number }
  statistic?: StatisticT<O> | null
  include_current?: boolean
  min_samples?: number
  compare: CompareT<O>
}

// ---------------------------------------------------------------- definitions per kind
// eslint-disable-next-line @typescript-eslint/no-unused-vars -- O kept so every definition shares the <O, C> signature
export interface SimpleDefinitionT<O, C> {
  kind: 'simple'
  when: C
  scoring?: 'binary' | 'weighted'
}

export interface VelocityDefinitionT<O> extends VelocityBodyT<O> {
  kind: 'velocity'
}

export interface CompositeDefinitionT<O, C> {
  kind: 'composite'
  gate?: C
  history_filter?: C
  velocity: VelocityBodyT<O>
}

export type ReferenceMode = 'exists' | 'not_exists' | 'attribute'

export interface ReferenceDefinitionT<O, C> {
  kind: 'reference'
  list: string
  key: O
  mode: ReferenceMode
  attribute_condition?: C
}

export type GraphMetric = 'distance_to_fraud' | 'fraud_neighbors' | 'shared_entity_count' | 'component_size' | 'degree' | 'community_fraud_rate'
export const GRAPH_METRICS: GraphMetric[] = ['distance_to_fraud', 'fraud_neighbors', 'shared_entity_count', 'component_size', 'degree', 'community_fraud_rate']

export interface GraphDefinitionT<O> {
  kind: 'graph'
  metric: GraphMetric
  link_kinds?: string[]
  include_similar?: boolean
  max_depth?: number
  compare: CompareT<O>
}

export type RuleDefinitionT<O, C>
  = | SimpleDefinitionT<O, C>
    | VelocityDefinitionT<O>
    | CompositeDefinitionT<O, C>
    | ReferenceDefinitionT<O, C>
    | GraphDefinitionT<O>

export type RuleDefinition = RuleDefinitionT<Operand, Condition>

export interface RuleEnvelopeT<D> {
  code: string
  name: string
  description?: string | null
  kind: RuleKind
  typologies: string[]
  event_types: string[]
  risk_score: number
  trapped_score?: number
  action?: RuleAction
  on_trapped?: OnTrapped
  missing_as_no_match?: boolean
  definition: D
}

export type RuleEnvelope = RuleEnvelopeT<RuleDefinition>

// ---------------------------------------------------------------- type guards
export function isLeaf<O>(c: ConditionT<O>): c is LeafConditionT<O> {
  return typeof c === 'object' && c !== null && 'op' in c && 'left' in c
}
export function isAll<O>(c: ConditionT<O>): c is { all: ConditionT<O>[] } {
  return typeof c === 'object' && c !== null && 'all' in c
}
export function isAny<O>(c: ConditionT<O>): c is { any: ConditionT<O>[] } {
  return typeof c === 'object' && c !== null && 'any' in c
}
export function isNot<O>(c: ConditionT<O>): c is { not: ConditionT<O> } {
  return typeof c === 'object' && c !== null && 'not' in c
}
export function isAtLeast<O>(c: ConditionT<O>): c is { at_least: { n: number, of: ConditionT<O>[] } } {
  return typeof c === 'object' && c !== null && 'at_least' in c
}
