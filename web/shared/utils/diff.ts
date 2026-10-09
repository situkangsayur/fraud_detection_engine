// Line diff (LCS) for rule version / proposal comparisons. Inputs are small (pretty-printed rule JSON), so the
// O(n·m) table is fine; inputs above MAX_LINES fall back to a naive whole-block replace.

export interface DiffLine {
  type: 'same' | 'add' | 'del'
  text: string
  oldNo?: number
  newNo?: number
}

const MAX_LINES = 2000

export function diffLines(oldText: string, newText: string): DiffLine[] {
  const a = oldText.split('\n')
  const b = newText.split('\n')
  if (a.length > MAX_LINES || b.length > MAX_LINES) {
    return [...a.map((text, i) => ({ type: 'del' as const, text, oldNo: i + 1 })), ...b.map((text, i) => ({ type: 'add' as const, text, newNo: i + 1 }))]
  }
  const n = a.length
  const m = b.length
  const lcs: number[][] = Array.from({ length: n + 1 }, () => Array<number>(m + 1).fill(0))
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i]![j] = a[i] === b[j] ? lcs[i + 1]![j + 1]! + 1 : Math.max(lcs[i + 1]![j]!, lcs[i]![j + 1]!)
    }
  }
  const out: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < n && j < m) {
    if (a[i] === b[j]) { out.push({ type: 'same', text: a[i]!, oldNo: i + 1, newNo: j + 1 }); i++; j++ }
    else if (lcs[i + 1]![j]! >= lcs[i]![j + 1]!) { out.push({ type: 'del', text: a[i]!, oldNo: i + 1 }); i++ }
    else { out.push({ type: 'add', text: b[j]!, newNo: j + 1 }); j++ }
  }
  while (i < n) { out.push({ type: 'del', text: a[i]!, oldNo: i + 1 }); i++ }
  while (j < m) { out.push({ type: 'add', text: b[j]!, newNo: j + 1 }); j++ }
  return out
}

/** Stable pretty JSON (sorted keys) so diffs show semantic changes, not key-order noise. */
export function stableJson(value: unknown): string {
  const sort = (v: unknown): unknown => {
    if (Array.isArray(v)) return v.map(sort)
    if (v && typeof v === 'object') return Object.fromEntries(Object.keys(v as object).sort().map(k => [k, sort((v as Record<string, unknown>)[k])]))
    return v
  }
  return JSON.stringify(sort(value), null, 2)
}
