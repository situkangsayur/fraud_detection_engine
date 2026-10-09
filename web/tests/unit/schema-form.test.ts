import { describe, expect, it } from 'vitest'
import type { JsonSchema } from '#shared/types/api'
import { defaultsFromSchema, getAt, mergeWithDefaults, parseNumberList, schemaToFields, setAt, validateParams } from '#shared/schema-form'

const MLP: JsonSchema = {
  type: 'object',
  additionalProperties: false,
  required: ['epochs'],
  properties: {
    hidden_layers: { type: 'array', items: { type: 'integer', minimum: 1, maximum: 1024 }, minItems: 1, maxItems: 6, default: [64, 32] },
    activation: { type: 'string', enum: ['relu', 'gelu', 'tanh'], default: 'relu' },
    dropout: { type: 'number', minimum: 0, maximum: 0.9, default: 0.2 },
    batch_norm: { type: 'boolean', default: true },
    lr: { type: 'number', title: 'Learning rate', exclusiveMinimum: 0, maximum: 1, default: 0.001 },
    epochs: { type: 'integer', minimum: 1, maximum: 1000, default: 50 },
    pos_weight: { type: ['number', 'null'], minimum: 0, default: null },
    optimizer: { type: 'object', properties: { name: { type: 'string', enum: ['adam', 'sgd'], default: 'adam' }, momentum: { type: 'number', default: 0.9 } } },
    tags: { type: 'array', items: { type: 'string' } },
    raw: { description: 'no type → JSON editor' },
  },
}

describe('JSON-schema form engine (ML plugin params)', () => {
  it('maps schema properties to widgets', () => {
    const f = Object.fromEntries(schemaToFields(MLP).map(x => [x.key, x]))
    expect(f.hidden_layers).toMatchObject({ widget: 'number-list', step: 1, min: 1, max: 1024 })
    expect(f.activation).toMatchObject({ widget: 'select', options: [{ label: 'relu', value: 'relu' }, { label: 'gelu', value: 'gelu' }, { label: 'tanh', value: 'tanh' }] })
    expect(f.dropout).toMatchObject({ widget: 'number', min: 0, max: 0.9, step: 'any' })
    expect(f.batch_norm).toMatchObject({ widget: 'switch' })
    expect(f.lr).toMatchObject({ widget: 'number', label: 'Learning rate' })
    expect(f.lr!.min).toBeGreaterThan(0)
    expect(f.epochs).toMatchObject({ widget: 'number', step: 1, required: true })
    expect(f.pos_weight).toMatchObject({ widget: 'number', nullable: true })
    expect(f.optimizer?.widget).toBe('group')
    expect(f.optimizer?.children?.map(c => c.path.join('.'))).toEqual(['optimizer.name', 'optimizer.momentum'])
    expect(f.tags?.widget).toBe('string-list')
    expect(f.raw?.widget).toBe('json')
  })

  it('builds nested defaults', () => {
    expect(defaultsFromSchema(MLP)).toEqual({
      hidden_layers: [64, 32], activation: 'relu', dropout: 0.2, batch_norm: true, lr: 0.001, epochs: 50, pos_weight: null,
      optimizer: { name: 'adam', momentum: 0.9 },
    })
  })

  it('merges user params over defaults and drops unknown keys when additionalProperties=false', () => {
    const merged = mergeWithDefaults(MLP, { epochs: 10, bogus: 1, optimizer: { name: 'sgd' } })
    expect(merged.epochs).toBe(10)
    expect(merged).not.toHaveProperty('bogus')
    expect(merged.optimizer).toEqual({ name: 'sgd', momentum: 0.9 })
  })

  it('validates ranges, enums, nullability and array items', () => {
    const issues = validateParams({ hidden_layers: [64, 0], activation: 'swish', dropout: 1.5, lr: 0, epochs: 1.5, batch_norm: null, bogus: 1 }, MLP)
    const byPath = Object.fromEntries(issues.map(i => [i.path, i.message]))
    expect(byPath['hidden_layers[1]']).toMatch(/≥ 1/)
    expect(byPath.activation).toMatch(/one of/)
    expect(byPath.dropout).toMatch(/≤ 0.9/)
    expect(byPath.lr).toMatch(/> 0/)
    expect(byPath.epochs).toMatch(/integer/)
    expect(byPath.batch_norm).toMatch(/null/)
    expect(byPath.bogus).toMatch(/unknown/)
    expect(validateParams({ ...defaultsFromSchema(MLP) }, MLP)).toEqual([])
    expect(validateParams({ ...defaultsFromSchema(MLP), epochs: undefined }, MLP)).toEqual([{ path: 'epochs', message: 'required' }])
  })

  it('parses comma separated number lists', () => {
    expect(parseNumberList('64, 32 16')).toEqual([64, 32, 16])
    expect(parseNumberList('1.5', true)).toBeNull()
    expect(parseNumberList('a,1')).toBeNull()
  })

  it('sets nested values immutably', () => {
    const root = { optimizer: { name: 'adam' }, epochs: 5 }
    const next = setAt(root, ['optimizer', 'momentum'], 0.5)
    expect(next).toEqual({ optimizer: { name: 'adam', momentum: 0.5 }, epochs: 5 })
    expect(root.optimizer).toEqual({ name: 'adam' })
    expect(getAt(next, ['optimizer', 'momentum'])).toBe(0.5)
    expect(setAt(next, ['epochs'], undefined)).not.toHaveProperty('epochs')
  })
})
