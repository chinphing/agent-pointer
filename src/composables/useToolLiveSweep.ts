import { nextTick, onBeforeUnmount, ref, watch, type Ref } from 'vue'
import { toolLiveSweepDuration } from '../lib/toolLiveSweep'

/**
 * Bind `--tool-live-sweep` from the visible text box width (cap 6s).
 *
 * The watcher keys on the element and the active flag only — **not** on the text.
 * Streaming text used to re-key it, so every chunk tore down the `ResizeObserver`,
 * waited a tick and built a new one just to re-measure a width that had not changed.
 * Width is measured once on mount and then owned by the observer callback.
 */
export function useToolLiveSweep(
  el: Ref<HTMLElement | null | undefined>,
  active: () => boolean
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
    [el, active],
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
