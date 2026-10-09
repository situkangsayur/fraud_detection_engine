// Formatting helpers (auto-imported). APIs use UTC ISO strings.
// The locale always comes from the active i18n locale (never the runtime default), so server-rendered and
// hydrated output are identical — otherwise Node (en-US) and the browser (id-ID) produce hydration mismatches.

const LANG: Record<string, string> = { id: 'id-ID', en: 'en-US' }
const TZ = 'Asia/Jakarta'

function loc(): string {
  const code = (tryUseNuxtApp()?.$i18n?.locale as { value?: string } | undefined)?.value ?? 'id'
  return LANG[code] ?? code
}

export function fmtNumber(v: number | null | undefined, digits = 0): string {
  if (v === null || v === undefined || Number.isNaN(v)) return '—'
  return new Intl.NumberFormat(loc(), { maximumFractionDigits: digits, minimumFractionDigits: 0 }).format(v)
}

export function fmtCompact(v: number | null | undefined): string {
  if (v === null || v === undefined) return '—'
  return new Intl.NumberFormat(loc(), { notation: 'compact', maximumFractionDigits: 1 }).format(v)
}

export function fmtMoney(v: number | null | undefined, currency = 'IDR'): string {
  if (v === null || v === undefined) return '—'
  return new Intl.NumberFormat(loc(), { style: 'currency', currency, maximumFractionDigits: currency === 'IDR' ? 0 : 2 }).format(v)
}

export function fmtPct(v: number | null | undefined, digits = 1): string {
  if (v === null || v === undefined || Number.isNaN(v)) return '—'
  return `${(v * 100).toFixed(digits)}%`
}

export function fmtDate(v: string | null | undefined): string {
  if (!v) return '—'
  const d = new Date(v)
  return Number.isNaN(d.getTime()) ? v : d.toLocaleString(loc(), { dateStyle: 'medium', timeStyle: 'short', timeZone: TZ })
}

export function fmtRelative(v: string | null | undefined): string {
  if (!v) return '—'
  const diff = (new Date(v).getTime() - Date.now()) / 1000
  const rtf = new Intl.RelativeTimeFormat(loc(), { numeric: 'auto' })
  const abs = Math.abs(diff)
  if (abs < 60) return rtf.format(Math.round(diff), 'second')
  if (abs < 3600) return rtf.format(Math.round(diff / 60), 'minute')
  if (abs < 86400) return rtf.format(Math.round(diff / 3600), 'hour')
  return rtf.format(Math.round(diff / 86400), 'day')
}

export function shortId(id: string | null | undefined): string {
  return id ? id.slice(0, 8) : '—'
}

export type BadgeColor = 'primary' | 'secondary' | 'success' | 'info' | 'warning' | 'error' | 'neutral'

const STATUS_COLORS: Record<string, BadgeColor> = {
  approve: 'success', review: 'warning', decline: 'error',
  match: 'error', no_match: 'neutral', trapped: 'warning',
  active: 'success', shadow: 'info', draft: 'neutral', pending_approval: 'warning', retired: 'neutral',
  training: 'info', ready: 'primary', archived: 'neutral', failed: 'error',
  pending: 'warning', approved: 'success', rejected: 'error', applied: 'success',
  open: 'warning', in_review: 'info', resolved_fraud: 'error', resolved_legit: 'success',
  running: 'info', done: 'success', queued: 'neutral', cancelled: 'neutral',
  indexed: 'success', processing: 'info', superseded: 'neutral',
  available: 'success', invalid: 'error', disabled: 'neutral',
  fraud: 'error', legit: 'success', unknown: 'neutral',
  stable: 'success', moderate: 'warning', significant: 'error',
  suspended: 'error',
}

export function statusColor(status: string | null | undefined): BadgeColor {
  return (status && STATUS_COLORS[status]) || 'neutral'
}

export function scoreColor(score: number | null | undefined, thresholds = { review: 50, decline: 80 }): BadgeColor {
  if (score === null || score === undefined) return 'neutral'
  if (score >= thresholds.decline) return 'error'
  if (score >= thresholds.review) return 'warning'
  return 'success'
}

export function downloadJson(filename: string, data: unknown) {
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  a.click()
  URL.revokeObjectURL(url)
}
