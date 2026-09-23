/**
 * Sentinel for "all / none" options in <USelect>: Reka UI's SelectItem rejects empty-string values.
 * useApi() drops query params equal to ALL, so filters can bind directly to it.
 */
export const ALL = '__all'

export const orAll = (v: unknown) => (v === undefined || v === null || v === '' ? ALL : String(v))
export const unlessAll = <T>(v: T) => (v === ALL || v === '' ? undefined : v)
