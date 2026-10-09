import { defineStore } from 'pinia'
import type { Me, ProjectMembership, ProjectRole } from '#shared/types/api'

const RANK: Record<ProjectRole, number> = { viewer: 1, analyst: 2, approver: 3, project_admin: 4 }

/** Current user, tenant and project memberships (from GET /me) + role helpers used for role-aware UI. */
export const useSessionStore = defineStore('session', () => {
  const me = ref<Me | null>(null)
  const loading = ref(false)

  const user = computed(() => me.value?.user ?? null)
  const tenant = computed(() => me.value?.tenant ?? null)
  const projects = computed<ProjectMembership[]>(() => me.value?.projects ?? [])
  const isPlatformAdmin = computed(() => !!user.value?.is_platform_admin)
  const isTenantAdmin = computed(() => user.value?.tenant_role === 'tenant_admin')

  function roleIn(pid: string | undefined | null): ProjectRole | null {
    if (!pid) return null
    if (isTenantAdmin.value) return 'project_admin'
    return projects.value.find(p => p.id === pid)?.role ?? null
  }

  /** True when the user's role in `pid` is at least `min` (project_admin > approver > analyst > viewer). */
  function can(pid: string | undefined | null, min: ProjectRole): boolean {
    const role = roleIn(pid)
    return !!role && RANK[role] >= RANK[min]
  }

  async function load(force = false) {
    if (me.value && !force) return me.value
    loading.value = true
    try {
      me.value = await useApi().get<Me>('/me', { silent: true })
    }
    finally {
      loading.value = false
    }
    return me.value
  }

  function set(value: Me | null) {
    me.value = value
  }

  return { me, loading, user, tenant, projects, isPlatformAdmin, isTenantAdmin, roleIn, can, load, set }
})
