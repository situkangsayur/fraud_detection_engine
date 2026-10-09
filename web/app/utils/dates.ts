// The APIs take RFC 3339 date-times; date inputs and range pickers work with plain YYYY-MM-DD days (UTC).

/** `2026-09-23` → `2026-09-23T00:00:00Z` (start of the day). */
export function dayStartIso(day: string): string {
  return `${day}T00:00:00Z`
}

/** `2026-09-23` → `2026-09-24T00:00:00Z` (end of the day, exclusive), so the whole day is included. */
export function dayEndIso(day: string): string {
  const d = new Date(`${day}T00:00:00Z`)
  d.setUTCDate(d.getUTCDate() + 1)
  return d.toISOString().replace('.000Z', 'Z')
}
