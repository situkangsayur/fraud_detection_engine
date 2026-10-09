import type { Items, ProjectMember } from '#shared/types/api'

/** Project members (GET /members → {items}); used to resolve user ids (case assignees, approvers) to names. */
export function useMembers() {
  const { pid, apiBase } = useProject()
  const api = useApi()
  const { data, refresh } = useAsyncData<ProjectMember[]>(
    () => `members-${pid.value}`,
    () => api.get<Items<ProjectMember>>(`${apiBase.value}/members`, { silent: true }).then(r => r.items).catch(() => []),
    { default: () => [], dedupe: 'defer' },
  )
  const nameOf = (id: string | null | undefined) => (id ? data.value.find(m => m.user_id === id)?.full_name ?? shortId(id) : null)
  return { members: data, refresh, nameOf }
}
