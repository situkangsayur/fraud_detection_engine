// Normalises every error shape the backends can return into one `ApiProblem`:
//  - RFC 7807 problem+json from the Rust services {type,title,status,detail,errors:[{field|path,message}]}
//  - FastAPI validation errors {detail:[{loc:[...], msg, type}]} and {detail:"..."} from the Python services
//  - rule validation bodies {valid:false, errors:[{path,message}]}
//  - plain text / HTML / empty bodies from proxies (Traefik 502/504)

export interface ApiFieldError {
  field: string
  message: string
}

export interface ApiProblem {
  status: number
  title: string
  detail?: string
  type?: string
  errors: ApiFieldError[]
  requestId?: string
  /** non-standard extension members (e.g. formula parse `position`) */
  extra?: Record<string, unknown>
}

const STATUS_TITLES: Record<number, string> = {
  400: 'Bad request',
  401: 'Not signed in',
  403: 'Forbidden',
  404: 'Not found',
  409: 'Conflict',
  413: 'Payload too large',
  422: 'Validation failed',
  429: 'Too many requests',
  500: 'Server error',
  502: 'Service unavailable',
  503: 'Service unavailable',
  504: 'Service timed out',
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
}

function fieldErrorsFrom(v: unknown): ApiFieldError[] {
  if (!Array.isArray(v)) return []
  return v.flatMap((e): ApiFieldError[] => {
    if (typeof e === 'string') return [{ field: '', message: e }]
    if (!isRecord(e)) return []
    // FastAPI: {loc: ["body","params","lr"], msg}
    if (Array.isArray(e.loc)) {
      const loc = (e.loc as unknown[]).filter(p => p !== 'body').map(String).join('.')
      return [{ field: loc, message: String(e.msg ?? e.message ?? 'invalid') }]
    }
    const field = String(e.field ?? e.path ?? e.pointer ?? '')
    return [{ field, message: String(e.message ?? e.msg ?? e.detail ?? 'invalid') }]
  })
}

export function parseProblem(status: number, body: unknown, requestId?: string): ApiProblem {
  const fallbackTitle = STATUS_TITLES[status] ?? (status >= 500 ? 'Server error' : 'Request failed')
  let parsed: unknown = body
  if (typeof body === 'string') {
    const trimmed = body.trim()
    if (trimmed.startsWith('{') || trimmed.startsWith('[')) {
      try { parsed = JSON.parse(trimmed) }
      catch { parsed = trimmed }
    }
  }

  if (isRecord(parsed)) {
    // nuxt/h3 error wrapper {statusCode, statusMessage, data: <problem>}
    if (isRecord(parsed.data) && ('statusCode' in parsed || 'statusMessage' in parsed))
      return parseProblem(Number(parsed.statusCode ?? status), parsed.data, requestId)

    const errors = [...fieldErrorsFrom(parsed.errors)]
    let detail: string | undefined
    if (typeof parsed.detail === 'string') detail = parsed.detail
    else if (Array.isArray(parsed.detail)) errors.push(...fieldErrorsFrom(parsed.detail))
    else if (typeof parsed.message === 'string') detail = parsed.message

    const known = new Set(['type', 'title', 'status', 'detail', 'errors', 'message', 'instance'])
    const extraEntries = Object.entries(parsed).filter(([k]) => !known.has(k))
    if (typeof parsed.message === 'string' && typeof parsed.detail === 'string') extraEntries.push(['message', parsed.message])
    return {
      ...(extraEntries.length ? { extra: Object.fromEntries(extraEntries) } : {}),
      status: typeof parsed.status === 'number' ? parsed.status : status,
      title: typeof parsed.title === 'string' && parsed.title ? parsed.title : fallbackTitle,
      detail,
      type: typeof parsed.type === 'string' ? parsed.type : undefined,
      errors,
      requestId,
    }
  }

  const text = typeof parsed === 'string' && parsed && !parsed.trimStart().startsWith('<') ? parsed.slice(0, 500) : undefined
  return { status, title: fallbackTitle, detail: text, errors: [], requestId }
}

/** One-line message for toasts. */
export function problemMessage(p: ApiProblem): string {
  const parts = [p.detail ?? '']
  if (p.errors.length) parts.push(p.errors.slice(0, 3).map(e => (e.field ? `${e.field}: ${e.message}` : e.message)).join('; '))
  const msg = parts.filter(Boolean).join(' — ')
  return msg || p.title
}

/** Maps field errors to a lookup usable by forms: `errorsByField(p)['definition.list']`. */
export function errorsByField(p: ApiProblem): Record<string, string> {
  const out: Record<string, string> = {}
  for (const e of p.errors) if (e.field && !(e.field in out)) out[e.field] = e.message
  return out
}
