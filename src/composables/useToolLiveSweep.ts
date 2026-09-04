import { nextTick, onBeforeUnmount, ref, watch, type Ref } from 'vue'
import { toolLiveSweepDuration } from '../lib/toolLiveSweep'

/** Bind `--tool-live-sweep` from the visible text box width (cap 6s). */
export function useToolLiveSweep(
  el: Ref<HTMLElement | null | undefined>,
  active: () => boolean,
  textKey: () => string
) {
  const duration = ref('6s')
  const boxPx = ref(0)
  let ro: ResizeObserver | null = null

  function measure() {
    const node = el.value
    if (!node || !active()) {
      duration.value = '6s'
      boxPx.value = 0
      return
    }
    const w = node.clientWidth || node.scrollWidth
    duration.value = toolLiveSweepDuration(w)
    boxPx.value = w
  }

  function disconnect() {
    ro?.disconnect()
    ro = null
  }

  watch(
    [el, active, textKey],
    async ([node, on]) => {
      disconnect()
      if (!on || !node) {
        duration.value = '6s'
        boxPx.value = 0
        return
      }
      await nextTick()
      measure()
      if (typeof ResizeObserver === 'undefined') return
      ro = new ResizeObserver(measure)
      ro.observe(node)
    },
    { immediate: true }
  )

  onBeforeUnmount(disconnect)
  return { duration, boxPx }
}
