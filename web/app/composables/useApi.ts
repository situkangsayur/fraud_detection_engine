// Typed client for the BFF (/api/** → gateway /api/v1/**).
// - Adds the anti-CSRF header the BFF requires on mutating requests.
// - On the server (SSR) uses useRequestFetch() so the session cookie is forwarded.
// - Normalises every error into ApiError(problem) and, unless `silent`, shows a toast.
import type { ApiProblem } from '#shared/utils/problem'
import { parseProblem, problemMessage } from '#shared/utils/problem'

export class ApiError extends Error {
  constructor(public problem: ApiProblem) {
    super(problemMessage(problem))
    this.name = 'ApiError'
  }

  get status() { return this.problem.status }
}

export interface ApiOptions {
  query?: Record<string, unknown>
  silent?: boolean
  signal?: AbortSignal
}

type Method = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'

function cleanQuery(q?: Record<string, unknown>) {
  if (!q) return undefined
  const out: Record<string, string> = {}
  for (const [k, v] of Object.entries(q)) {
    if (v === undefined || v === null || v === '' || v === ALL) continue
    out[k] = Array.isArray(v) ? v.join(',') : String(v)
  }
  return out
}

export function useApi() {
  // Must work outside component setup too (Pinia actions, middleware) → no useI18n() here.
  const nuxtApp = useNuxtApp()
  const requestFetch = import.meta.server ? useRequestFetch() : $fetch
  const toast = import.meta.client ? useToast() : null
  const t = (key: string) => String(nuxtApp.$i18n.t(key))

  function handleError(err: unknown, silent?: boolean): never {
    const e = err as { status?: number, statusCode?: number, data?: unknown, response?: { headers?: Headers, _data?: unknown } }
    const status = e.status ?? e.statusCode ?? 0
    const problem = status
      ? parseProblem(status, e.data ?? e.response?._data, e.response?.headers?.get('x-request-id') ?? undefined)
      : { status: 0, title: t('errors.network'), errors: [] }
    if (import.meta.client && status === 401) {
      const route = useRoute()
      if (route.path !== '/login') navigateTo({ path: '/login', query: { next: route.fullPath } })
    }
    else if (!silent && toast) {
      toast.add({ title: problem.title, description: problemMessage(problem), color: 'error', icon: 'i-lucide-circle-alert' })
    }
    throw new ApiError(problem)
  }

  async function request<T>(method: Method, path: string, body?: unknown, opts: ApiOptions = {}): Promise<T> {
    try {
      return await requestFetch<T>(`/api${path}`, {
        method,
        body: body as Record<string, unknown> | FormData | undefined,
        query: cleanQuery(opts.query),
        headers: { 'x-requested-with': 'fraud-web', 'accept': 'application/json' },
        signal: opts.signal,
      }) as T
    }
    catch (err) {
      return handleError(err, opts.silent)
    }
  }

  /** POST to an SSE endpoint and dispatch each `event:`/`data:` frame. Resolves when the stream ends. */
  async function stream(path: string, body: unknown, onEvent: (event: string, data: unknown) => void, signal?: AbortSignal): Promise<void> {
    const res = await fetch(`/api${path}`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'accept': 'text/event-stream', 'x-requested-with': 'fraud-web' },
      body: JSON.stringify(body),
      signal,
    })
    if (!res.ok || !res.body) {
      const text = await res.text().catch(() => '')
      handleError({ status: res.status, data: text, response: { headers: res.headers } })
    }
    const reader = res.body!.pipeThrough(new TextDecoderStream()).getReader()
    let buffer = ''
    for (;;) {
      const { value, done } = await reader.read()
      if (done) break
      buffer += value
      let idx: number
      while ((idx = buffer.indexOf('\n\n')) >= 0) {
        const frame = buffer.slice(0, idx)
        buffer = buffer.slice(idx + 2)
        let event = 'message'
        const data: string[] = []
        for (const line of frame.split('\n')) {
          if (line.startsWith('event:')) event = line.slice(6).trim()
          else if (line.startsWith('data:')) data.push(line.slice(5).trimStart())
        }
        const raw = data.join('\n')
        let parsed: unknown = raw
        try { parsed = JSON.parse(raw) }
        catch { /* plain text frame */ }
        onEvent(event, parsed)
      }
    }
  }

  return {
    get: <T>(path: string, opts?: ApiOptions) => request<T>('GET', path, undefined, opts),
    post: <T>(path: string, body?: unknown, opts?: ApiOptions) => request<T>('POST', path, body ?? {}, opts),
    put: <T>(path: string, body?: unknown, opts?: ApiOptions) => request<T>('PUT', path, body ?? {}, opts),
    patch: <T>(path: string, body?: unknown, opts?: ApiOptions) => request<T>('PATCH', path, body ?? {}, opts),
    del: <T = void>(path: string, opts?: ApiOptions) => request<T>('DELETE', path, undefined, opts),
    upload: <T>(path: string, form: FormData, opts?: ApiOptions) => request<T>('POST', path, form, opts),
    stream,
  }
}
