import { describe, expect, it } from 'vitest'
import { diffLines, stableJson } from '#shared/utils/diff'

describe('line diff', () => {
  it('marks added and removed lines around common ones', () => {
    const d = diffLines('a\nb\nc', 'a\nx\nc\nd')
    expect(d.map(l => `${l.type}:${l.text}`)).toEqual(['same:a', 'del:b', 'add:x', 'same:c', 'add:d'])
  })

  it('stableJson sorts keys so key order does not create noise', () => {
    expect(stableJson({ b: 1, a: { d: 2, c: 3 } })).toBe(stableJson({ a: { c: 3, d: 2 }, b: 1 }))
    expect(diffLines(stableJson({ b: 1, a: 2 }), stableJson({ a: 2, b: 1 })).every(l => l.type === 'same')).toBe(true)
  })
})
