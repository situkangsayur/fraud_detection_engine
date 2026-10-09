import { describe, expect, it } from 'vitest'
import { dayEndIso, dayStartIso } from '../../app/utils/dates'

describe('day range helpers', () => {
  it('turns days into RFC 3339 bounds', () => {
    expect(dayStartIso('2026-09-23')).toBe('2026-09-23T00:00:00Z')
    expect(dayEndIso('2026-09-30')).toBe('2026-10-01T00:00:00Z')
    expect(dayEndIso('2026-12-31')).toBe('2027-01-01T00:00:00Z')
  })
})
