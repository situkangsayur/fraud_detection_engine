import { describe, expect, it } from 'vitest'
import type { RuleEnvelope } from '#shared/rules/dsl'
import { RULE_KINDS } from '#shared/rules/dsl'
import { RULE_EXAMPLES, EXAMPLE_COMPOSITE, EXAMPLE_FORMULA, EXAMPLE_SIMPLE } from '#shared/rules/examples'
import {
  collectFieldPaths, conditionFromDsl, conditionToDsl, defaultEnvelope, definitionFromDsl, definitionToDsl,
  describeDefinition, envelopeFromDsl, envelopeToDsl, localValidate,
} from '#shared/rules/model'

describe('rule builder model ⇄ rule DSL', () => {
  it.each(RULE_EXAMPLES.map(e => [e.code, e] as const))('round-trips %s losslessly', (_code, example) => {
    const model = envelopeFromDsl(example)
    expect(envelopeToDsl(model)).toEqual(example)
  })

  it('covers all five rule kinds in the fixtures', () => {
    expect(new Set(RULE_EXAMPLES.map(e => e.kind))).toEqual(new Set(RULE_KINDS))
  })

  it('assigns unique uids to every node (keyed rendering)', () => {
    const model = envelopeFromDsl(EXAMPLE_FORMULA)
    const uids: string[] = []
    JSON.stringify(model, (k, v) => { if (k === 'uid') uids.push(v as string); return v })
    expect(uids.length).toBeGreaterThan(10)
    expect(new Set(uids).size).toBe(uids.length)
  })

  it('represents formula args as an ordered editable list and restores the object', () => {
    const model = envelopeFromDsl(EXAMPLE_FORMULA)
    const def = model.definition
    if (def.kind !== 'simple' || def.when.type !== 'any') throw new Error('unexpected shape')
    const leaf = def.when.children[0]!
    if (leaf.type !== 'leaf' || leaf.left.type !== 'formula') throw new Error('unexpected shape')
    expect(leaf.left.args.map(a => a.name)).toEqual(['x', 'y', 'z'])
    leaf.left.args.reverse()
    const back = envelopeToDsl(model)
    const any = (back.definition as { when: { any: { left: { args: Record<string, unknown> } }[] } }).when.any[0]!
    expect(Object.keys(any.left.args).sort()).toEqual(['x', 'y', 'z'])
  })

  it('preserves unknown keys from newer DSL versions', () => {
    const future = { ...EXAMPLE_SIMPLE, definition: { ...EXAMPLE_SIMPLE.definition, future_flag: true } } as unknown as RuleEnvelope
    expect(envelopeToDsl(envelopeFromDsl(future))).toEqual(future)
  })

  it('drops the right operand for unary operators', () => {
    const model = conditionFromDsl({ left: { type: 'field', path: 'event.promo_code' }, op: 'is_null' })
    if (model.type !== 'leaf') throw new Error('leaf expected')
    model.right = { uid: 'x', type: 'const', value: 1 }
    expect(conditionToDsl(model)).toEqual({ left: { type: 'field', path: 'event.promo_code' }, op: 'is_null' })
  })

  it('keeps composite gate / history filter optional', () => {
    const { gate: _g, ...noGate } = EXAMPLE_COMPOSITE.definition as Extract<RuleEnvelope['definition'], { kind: 'composite' }>
    expect(definitionToDsl(definitionFromDsl(noGate))).toEqual(noGate)
  })

  it.each(RULE_KINDS)('default %s rule serialises to a structurally valid definition', (kind) => {
    const env = envelopeToDsl({ ...defaultEnvelope(kind), code: 'RL-TEST-1', name: 'test' })
    expect(env.kind).toBe(kind)
    expect(env.definition.kind).toBe(kind)
    const issues = localValidate(env).filter(i => !i.path.startsWith('definition.list'))
    expect(issues).toEqual([])
  })

  it('describes definitions for list views', () => {
    expect(describeDefinition(EXAMPLE_SIMPLE.definition)).toBe('(event.amount > 5000000) AND (event.issuer_country ≠ event.geo_country)')
    expect(describeDefinition(RULE_EXAMPLES.find(e => e.kind === 'graph')!.definition)).toContain('graph.distance_to_fraud')
  })

  it('collects referenced field paths (including formula args)', () => {
    expect(collectFieldPaths(EXAMPLE_FORMULA.definition)).toEqual([
      'customer.full_name', 'event.channel', 'event.promo_code', 'features.amount_zscore_30d', 'features.cust_cnt_24h',
      'features.is_new_device', 'features.recent_credential_change_24h',
    ])
  })

  it('flags invalid envelopes locally before calling the server', () => {
    const bad = { ...EXAMPLE_SIMPLE, code: 'bad code', risk_score: 150, definition: { kind: 'simple', when: { all: [] } } } as RuleEnvelope
    const paths = localValidate(bad).map(i => i.path)
    expect(paths).toEqual(expect.arrayContaining(['code', 'risk_score', 'definition.when']))
  })
})
