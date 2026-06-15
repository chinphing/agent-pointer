import type { WindowDragRegionPolicy } from '../lib/windowDragRegions'
import { WINDOW_DRAG_INTERACTIVE_SELECTOR } from '../lib/windowDragRegions'
import { isTauriRuntime } from '../lib/runtime'
import { useWindowChrome } from './useWindowChrome'

const INTERACTIVE_SELECTOR = WINDOW_DRAG_INTERACTIVE_SELECTOR

export function useWindowDragRegion() {
  const { enabled, startDrag, toggleMaximize } = useWindowChrome()

  function onMouseDown(e: MouseEvent, policy: WindowDragRegionPolicy) {
    if (!enabled || !isTauriRuntime()) return
    if (!policy.draggable) return
    if (e.button !== 0) return
    const target = e.target as HTMLElement | null
    if (target?.closest(INTERACTIVE_SELECTOR)) return
    startDrag()
  }

  function onDoubleClick(e: MouseEvent, policy: WindowDragRegionPolicy) {
    if (!enabled || !policy.doubleClickMaximize) return
    const target = e.target as HTMLElement | null
    if (target?.closest(INTERACTIVE_SELECTOR)) return
    void toggleMaximize()
  }

  return { onMouseDown, onDoubleClick }
}
