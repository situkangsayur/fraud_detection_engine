import type { FieldCatalogEntry, Items, Page, ReferenceList } from '#shared/types/api'

/** Field catalog of the current project (built-ins + source.* fields), cached per project. */
export function useFieldCatalog() {
  const { pid, apiBase } = useProject()
  const api = useApi()
  const { data, refresh, status } = useAsyncData(
    () => `catalog-${pid.value}`,
    () => api.get<Items<FieldCatalogEntry>>(`${apiBase.value}/field-catalog`, { query: { page_size: 500 }, silent: true }).then(p => p.items).catch(() => [] as FieldCatalogEntry[]),
    { default: () => [] as FieldCatalogEntry[], dedupe: 'defer' },
  )
  const paths = computed(() => data.value.map(f => f.path))
  /** Velocity group/aggregate fields use bare canonical names (e.g. `device_id`) or `source.*` paths. */
  const velocityFields = computed(() => data.value
    .filter(f => f.velocity_enabled && (f.entity === 'event' || f.entity === 'source'))
    .map(f => f.velocity_column ?? (f.entity === 'event' ? f.path.replace(/^event\./, '') : f.path)))
  return { catalog: data, paths, velocityFields, refresh, status }
}

/** Reference lists visible in the project (project + tenant-wide). */
export function useReferenceLists() {
  const { pid, apiBase } = useProject()
  const api = useApi()
  return useAsyncData(
    () => `reflists-${pid.value}`,
    () => api.get<ReferenceList[] | Page<ReferenceList>>(`${apiBase.value}/reference-lists`, { silent: true }).then(r => (Array.isArray(r) ? r : r.items)).catch(() => [] as ReferenceList[]),
    { default: () => [] as ReferenceList[], dedupe: 'defer' },
  )
}
