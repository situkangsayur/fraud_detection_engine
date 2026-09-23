/** Debounced mirror of a ref (search boxes → API queries). */
export function useDebounced<T>(source: Ref<T>, delayMs = 300): Ref<T> {
  const out = ref(source.value) as Ref<T>
  let timer: ReturnType<typeof setTimeout> | undefined
  watch(source, (v) => {
    clearTimeout(timer)
    timer = setTimeout(() => { out.value = v }, delayMs)
  })
  onScopeDispose(() => clearTimeout(timer))
  return out
}
