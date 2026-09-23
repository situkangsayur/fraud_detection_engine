import type { H3Event } from 'h3'
import type { ApiProblem } from '#shared/utils/problem'

/** Responds with RFC 7807 problem+json (same shape the backends use, so the client has one error path). */
export function sendProblem(event: H3Event, status: number, title: string, detail?: string): ApiProblem {
  setResponseStatus(event, status)
  setResponseHeader(event, 'content-type', 'application/problem+json')
  return { status, title, detail, errors: [] }
}
