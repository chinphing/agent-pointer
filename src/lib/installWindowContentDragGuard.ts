import { isTauriRuntime } from '../lib/runtime'

import { WINDOW_DRAG_INTERACTIVE_SELECTOR } from '../lib/windowDragRegions'

const INTERACTIVE_SELECTOR = WINDOW_DRAG_INTERACTIVE_SELECTOR

/** Block macOS background window-drag on content; allow only explicit chrome strips. */
export function installWindowContentDragGuard() {
  if (!isTauriRuntime()) return

  document.addEventListener(
    'mousedown',
    e => {
      if (e.button !== 0) return
      const target = e.target as HTMLElement | null
      if (!target) return
      if (target.closest(INTERACTIVE_SELECTOR)) return
      if (target.closest('.window-drag-region')) return
      e.preventDefault()
    },
    true
  )
}
