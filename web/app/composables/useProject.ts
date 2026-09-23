import type { Project, ProjectRole } from '#shared/types/api'

/** Project-scoped pages (/p/[pid]/...): current project id, data, base path and role checks. */
export function useProject() {
  const route = useRoute()
  const session = useSessionStore()
  const pid = computed(() => String(route.params.pid ?? ''))
  const base = computed(() => `/p/${pid.value}`)
  const api = useApi()

  const { data: project, refresh, status } = useAsyncData(
    () => `project-${pid.value}`,
    () => (pid.value ? api.get<Project>(`/projects/${pid.value}`) : Promise.resolve(null)),
    { watch: [pid], dedupe: 'defer' },
  )

  const role = computed(() => session.roleIn(pid.value))
  const can = (min: ProjectRole) => session.can(pid.value, min)

  return { pid, base, project, refresh, status, role, can, apiBase: computed(() => `/projects/${pid.value}`) }
}
