import type { H3Event } from 'h3'
import { mockFetch } from '../mock'

export function isMockMode(): boolean {
  const config = useRuntimeConfig()
  const flag = String(config.mockApi || process.env.MOCK_API || '').toLowerCase()
  return flag === '1' || flag === 'true'
}

export interface UpstreamInit {
  method: string
  headers: Record<string, string>
  body?: BodyInit | null
  /** Long-lived responses (SSE, large downloads) are exempt from the request timeout. */
  streaming?: boolean
}

/** Single place that talks to the gateway (or the in-process mock). */
export async function upstreamFetch(_event: H3Event | null, path: string, init: UpstreamInit): Promise<Response> {
  if (isMockMode()) return mockFetch(path, init)
  const config = useRuntimeConfig()
  const base = String(config.apiBaseUrl).replace(/\/+$/, '')
  const requestInit: RequestInit & { duplex?: 'half' } = {
    method: init.method,
    headers: init.headers,
    body: init.body ?? undefined,
    redirect: 'manual',
  }
  if (init.body instanceof ReadableStream) requestInit.duplex = 'half'
  if (!init.streaming) requestInit.signal = AbortSignal.timeout(Number(config.apiTimeoutMs) || 30_000)
  try {
    return await fetch(`${base}${path}`, requestInit)
  }
  catch (err) {
    const timeout = err instanceof Error && (err.name === 'TimeoutError' || err.name === 'AbortError')
    const status = timeout ? 504 : 502
    return new Response(JSON.stringify({
      type: 'about:blank',
      title: timeout ? 'Gateway timed out' : 'Gateway unreachable',
      status,
      detail: timeout ? 'The platform did not answer in time.' : 'Could not reach the platform gateway.',
    }), { status, headers: { 'content-type': 'application/problem+json' } })
  }
}
