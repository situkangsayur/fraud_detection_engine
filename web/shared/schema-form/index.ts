// JSON Schema → form field specs, defaults and validation for ML plugin `param_schema`s (docs/technical/ml-plugins.md).
// Plugins can be added on the fly, so the UI never hard-codes algorithm parameters: it renders whatever the plugin
// declares. Supports the subset plugins realistically use: number/integer (min/max/exclusive/multipleOf), boolean,
// string (+enum), arrays of numbers/strings, nested objects, nullable unions. Anything else falls back to a JSON editor.

import type { JsonSchema } from '../types/api'

export type Widget = 'number' | 'switch' | 'select' | 'text' | 'number-list' | 'string-list' | 'multiselect' | 'group' | 'json'

export interface FieldSpec {
  key: string
  path: string[]
  label: string
  description?: string
  widget: Widget
  valueType: 'number' | 'integer' | 'boolean' | 'string' | 'array' | 'object' | 'unknown'
  required: boolean
  nullable: boolean
  min?: number
  max?: number
  step?: number | 'any'
  options?: { label: string, value: unknown }[]
  default?: unknown
  children?: FieldSpec[]
}

export interface SchemaIssue {
  path: string
  message: string
}

function primaryType(schema: JsonSchema): { type: string | undefined, nullable: boolean } {
  const t = schema.type
  if (Array.isArray(t)) {
    const nonNull = t.filter(x => x !== 'null')
    return { type: nonNull[0], nullable: t.includes('null') }
  }
  return { type: t ?? (schema.properties ? 'object' : schema.enum ? 'string' : undefined), nullable: false }
}

export function humanize(key: string): string {
  const s = key.replace(/[_-]+/g, ' ').trim()
  return s.charAt(0).toUpperCase() + s.slice(1)
}

export function schemaToFields(schema: JsonSchema, parentPath: string[] = []): FieldSpec[] {
  const props = schema.properties ?? {}
  const required = new Set(schema.required ?? [])
  return Object.entries(props).map(([key, prop]) => fieldFor(key, prop, [...parentPath, key], required.has(key)))
}

function fieldFor(key: string, prop: JsonSchema, path: string[], required: boolean): FieldSpec {
  const { type, nullable } = primaryType(prop)
  const base = {
    key,
    path,
    label: prop.title ?? humanize(key),
    description: prop.description,
    required,
    nullable,
    default: prop.default,
  }

  if (prop.enum && prop.enum.length) {
    return {
      ...base,
      widget: 'select',
      valueType: type === 'integer' ? 'integer' : type === 'number' ? 'number' : type === 'boolean' ? 'boolean' : 'string',
      options: prop.enum.map(v => ({ label: String(v), value: v })),
    }
  }

  switch (type) {
    case 'integer':
    case 'number': {
      const min = prop.minimum ?? (prop.exclusiveMinimum !== undefined ? prop.exclusiveMinimum + (type === 'integer' ? 1 : Number.EPSILON) : undefined)
      const max = prop.maximum ?? (prop.exclusiveMaximum !== undefined ? prop.exclusiveMaximum - (type === 'integer' ? 1 : Number.EPSILON) : undefined)
      return { ...base, widget: 'number', valueType: type, min, max, step: prop.multipleOf ?? (type === 'integer' ? 1 : 'any') }
    }
    case 'boolean':
      return { ...base, widget: 'switch', valueType: 'boolean' }
    case 'string':
      return { ...base, widget: 'text', valueType: 'string' }
    case 'array': {
      const items = prop.items ?? {}
      const itemType = primaryType(items).type
      if (items.enum?.length) {
        return { ...base, widget: 'multiselect', valueType: 'array', options: items.enum.map(v => ({ label: String(v), value: v })) }
      }
      if (itemType === 'integer' || itemType === 'number') {
        return { ...base, widget: 'number-list', valueType: 'array', min: items.minimum, max: items.maximum, step: itemType === 'integer' ? 1 : 'any' }
      }
      if (itemType === 'string') return { ...base, widget: 'string-list', valueType: 'array' }
      return { ...base, widget: 'json', valueType: 'array' }
    }
    case 'object':
      if (prop.properties) return { ...base, widget: 'group', valueType: 'object', children: schemaToFields(prop, path) }
      return { ...base, widget: 'json', valueType: 'object' }
    default:
      return { ...base, widget: 'json', valueType: 'unknown' }
  }
}

/** Default parameter object (recursively) from `default` keywords. Keys without defaults are omitted. */
export function defaultsFromSchema(schema: JsonSchema): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const [key, prop] of Object.entries(schema.properties ?? {})) {
    if (prop.default !== undefined) out[key] = JSON.parse(JSON.stringify(prop.default))
    else if (primaryType(prop).type === 'object' && prop.properties) {
      const nested = defaultsFromSchema(prop)
      if (Object.keys(nested).length) out[key] = nested
    }
  }
  return out
}

/** Merge user params over defaults (user wins), dropping keys that are not in the schema. */
export function mergeWithDefaults(schema: JsonSchema, params: Record<string, unknown> | undefined): Record<string, unknown> {
  const defaults = defaultsFromSchema(schema)
  const out: Record<string, unknown> = { ...defaults }
  for (const [k, v] of Object.entries(params ?? {})) {
    const prop = schema.properties?.[k]
    if (!prop && schema.additionalProperties === false) continue
    if (prop && primaryType(prop).type === 'object' && prop.properties && typeof v === 'object' && v !== null && !Array.isArray(v))
      out[k] = mergeWithDefaults(prop, v as Record<string, unknown>)
    else out[k] = v
  }
  return out
}

export function parseNumberList(text: string, integer = false): number[] | null {
  const parts = text.split(/[,\s]+/).map(s => s.trim()).filter(Boolean)
  const nums = parts.map(Number)
  if (nums.some(n => Number.isNaN(n))) return null
  if (integer && nums.some(n => !Number.isInteger(n))) return null
  return nums
}

export function formatNumberList(v: unknown): string {
  return Array.isArray(v) ? v.join(', ') : ''
}

function typeOk(value: unknown, type: string | undefined): boolean {
  switch (type) {
    case 'integer': return typeof value === 'number' && Number.isInteger(value)
    case 'number': return typeof value === 'number' && Number.isFinite(value)
    case 'boolean': return typeof value === 'boolean'
    case 'string': return typeof value === 'string'
    case 'array': return Array.isArray(value)
    case 'object': return typeof value === 'object' && value !== null && !Array.isArray(value)
    default: return true
  }
}

/** Client-side validation (ml-service re-validates authoritatively with a real JSON Schema validator). */
export function validateParams(values: Record<string, unknown>, schema: JsonSchema, prefix = ''): SchemaIssue[] {
  const issues: SchemaIssue[] = []
  const props = schema.properties ?? {}
  for (const req of schema.required ?? []) {
    if (values[req] === undefined || values[req] === '') issues.push({ path: prefix + req, message: 'required' })
  }
  if (schema.additionalProperties === false) {
    for (const k of Object.keys(values)) if (!(k in props)) issues.push({ path: prefix + k, message: 'unknown parameter' })
  }
  for (const [key, prop] of Object.entries(props)) {
    const value = values[key]
    if (value === undefined) continue
    const path = prefix + key
    const { type, nullable } = primaryType(prop)
    if (value === null) {
      if (!nullable) issues.push({ path, message: 'must not be null' })
      continue
    }
    if (!typeOk(value, type)) {
      issues.push({ path, message: `must be ${type}` })
      continue
    }
    if (prop.enum && !prop.enum.some(e => JSON.stringify(e) === JSON.stringify(value))) issues.push({ path, message: `must be one of ${prop.enum.join(', ')}` })
    if (typeof value === 'number') {
      if (prop.minimum !== undefined && value < prop.minimum) issues.push({ path, message: `must be ≥ ${prop.minimum}` })
      if (prop.maximum !== undefined && value > prop.maximum) issues.push({ path, message: `must be ≤ ${prop.maximum}` })
      if (prop.exclusiveMinimum !== undefined && value <= prop.exclusiveMinimum) issues.push({ path, message: `must be > ${prop.exclusiveMinimum}` })
      if (prop.exclusiveMaximum !== undefined && value >= prop.exclusiveMaximum) issues.push({ path, message: `must be < ${prop.exclusiveMaximum}` })
    }
    if (typeof value === 'string') {
      if (prop.minLength !== undefined && value.length < prop.minLength) issues.push({ path, message: `must have at least ${prop.minLength} characters` })
      if (prop.maxLength !== undefined && value.length > prop.maxLength) issues.push({ path, message: `must have at most ${prop.maxLength} characters` })
    }
    if (Array.isArray(value)) {
      if (prop.minItems !== undefined && value.length < prop.minItems) issues.push({ path, message: `needs at least ${prop.minItems} items` })
      if (prop.maxItems !== undefined && value.length > prop.maxItems) issues.push({ path, message: `allows at most ${prop.maxItems} items` })
      const items = prop.items
      if (items) {
        const itemType = primaryType(items).type
        value.forEach((item, i) => {
          if (!typeOk(item, itemType)) issues.push({ path: `${path}[${i}]`, message: `must be ${itemType}` })
          else if (typeof item === 'number') {
            if (items.minimum !== undefined && item < items.minimum) issues.push({ path: `${path}[${i}]`, message: `must be ≥ ${items.minimum}` })
            if (items.maximum !== undefined && item > items.maximum) issues.push({ path: `${path}[${i}]`, message: `must be ≤ ${items.maximum}` })
          }
        })
      }
    }
    if (type === 'object' && prop.properties) issues.push(...validateParams(value as Record<string, unknown>, prop, `${path}.`))
  }
  return issues
}

export function getAt(obj: Record<string, unknown>, path: string[]): unknown {
  let cur: unknown = obj
  for (const p of path) {
    if (typeof cur !== 'object' || cur === null) return undefined
    cur = (cur as Record<string, unknown>)[p]
  }
  return cur
}

/** Immutable set at a nested path (returns a new root object). */
export function setAt(obj: Record<string, unknown>, path: string[], value: unknown): Record<string, unknown> {
  if (!path.length) return obj
  const [head, ...tail] = path as [string, ...string[]]
  const copy: Record<string, unknown> = { ...obj }
  if (!tail.length) {
    if (value === undefined) return Object.fromEntries(Object.entries(copy).filter(([k]) => k !== head))
    copy[head] = value
    return copy
  }
  const child = typeof copy[head] === 'object' && copy[head] !== null && !Array.isArray(copy[head]) ? copy[head] as Record<string, unknown> : {}
  copy[head] = setAt(child, tail, value)
  return copy
}
