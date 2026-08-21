/** Shared Chart.js mount scheduling — keep conversation switches from freezing. */

export const CHART_VIEWPORT_MARGIN_PX = 240
export const CHART_MIN_LAYOUT_PX = 8
const CHARTS_PER_FRAME = 2

type QueueJob = () => void

const constructQueue: QueueJob[] = []
let queuedRaf = 0
let remainingThisFrame = 0

function pumpConstructQueue(): void {
  if (queuedRaf) return
  queuedRaf = requestAnimationFrame(() => {
    queuedRaf = 0
    remainingThisFrame = CHARTS_PER_FRAME
    while (remainingThisFrame > 0 && constructQueue.length > 0) {
      remainingThisFrame -= 1
      constructQueue.shift()?.()
    }
    if (constructQueue.length > 0) {
      pumpConstructQueue()
    }
  })
}

/** Run Chart constructors a few per frame so the UI can paint between them. */
export function scheduleChartConstruct(): Promise<void> {
  if (typeof requestAnimationFrame === 'undefined') {
    return Promise.resolve()
  }
  return new Promise(resolve => {
    constructQueue.push(resolve)
    pumpConstructQueue()
  })
}

export const SCROLL_ROOT_SELECTOR =
  '.chat-scroll-area, .workspace-scroll-area, .file-preview-scroll'

/** Nearest chat / workspace / file-preview scroller (not the window). */
export function closestScrollRoot(el: Element): Element | null {
  const named = el.closest(SCROLL_ROOT_SELECTOR)
  return named instanceof Element ? named : null
}

/** @deprecated Use {@link closestScrollRoot}. */
export function closestChatScroller(el: Element): Element | null {
  return closestScrollRoot(el)
}

/** False until the host has a real box. 0×0 must not be treated as in-viewport. */
export function isChartHostLaidOut(el: Element, minPx = CHART_MIN_LAYOUT_PX): boolean {
  if (typeof window === 'undefined') return true
  const rect = el.getBoundingClientRect()
  return rect.width >= minPx && rect.height >= minPx
}

export function isNearViewport(el: Element, marginPx = CHART_VIEWPORT_MARGIN_PX): boolean {
  if (typeof window === 'undefined') return true
  if (!isChartHostLaidOut(el)) return false
  const rect = el.getBoundingClientRect()
  const root = closestScrollRoot(el)
  if (root) {
    const rootRect = root.getBoundingClientRect()
    const top = rootRect.top - marginPx
    const bottom = rootRect.bottom + marginPx
    return rect.bottom >= top && rect.top <= bottom
  }
  const top = -marginPx
  const bottom = (window.innerHeight || 0) + marginPx
  return rect.bottom >= top && rect.top <= bottom
}

export function isInViewForLazyMount(el: Element): boolean {
  return isChartHostLaidOut(el) && isNearViewport(el)
}

export function observeUntilLaidOut(
  el: Element,
  onLaidOut: () => void,
  minPx = CHART_MIN_LAYOUT_PX,
): ResizeObserver | null {
    if (typeof ResizeObserver === 'undefined') {
      console.warn('[markdownCharts] ResizeObserver unavailable; cannot wait for chart layout')
      return null
    }
  const observer = new ResizeObserver(() => {
    if (!isChartHostLaidOut(el, minPx)) return
    observer.disconnect()
    onLaidOut()
  })
  observer.observe(el)
  return observer
}

export function observeUntilNearViewport(
  el: Element,
  onVisible: () => void,
  marginPx = CHART_VIEWPORT_MARGIN_PX,
): IntersectionObserver | null {
  if (typeof IntersectionObserver === 'undefined') {
    onVisible()
    return null
  }
  const observer = new IntersectionObserver(
    entries => {
      if (!entries.some(entry => entry.isIntersecting)) return
      observer.disconnect()
      onVisible()
    },
    {
      root: closestScrollRoot(el),
      rootMargin: `${marginPx}px 0px`,
      threshold: 0,
    },
  )
  observer.observe(el)
  return observer
}

export type ViewportDeferral = {
  resizeObserver: ResizeObserver | null
  visibilityObserver: IntersectionObserver | null
  disconnect: () => void
}

function disconnectObserver(observer: { disconnect: () => void } | null, label: string) {
  if (!observer) return
  try {
    observer.disconnect()
  } catch (err) {
    console.warn(`[markdownCharts] ${label} disconnect failed`, err)
  }
}

/**
 * Wait until `el` has a layout box and intersects the nearest scroll root
 * (hidden `v-show` tabs are 0×0 until shown).
 */
export function deferUntilInView(el: Element, onReady: () => void): ViewportDeferral {
  const handles: ViewportDeferral = {
    resizeObserver: null,
    visibilityObserver: null,
    disconnect() {
      disconnectObserver(handles.resizeObserver, 'viewport resizeObserver')
      disconnectObserver(handles.visibilityObserver, 'viewport visibilityObserver')
      handles.resizeObserver = null
      handles.visibilityObserver = null
    },
  }

  const fire = () => {
    handles.disconnect()
    onReady()
  }

  if (!isChartHostLaidOut(el)) {
    handles.resizeObserver = observeUntilLaidOut(el, () => {
      handles.resizeObserver = null
      if (isInViewForLazyMount(el)) {
        fire()
        return
      }
      handles.visibilityObserver = observeUntilNearViewport(el, fire)
    })
  } else {
    handles.visibilityObserver = observeUntilNearViewport(el, fire)
  }
  return handles
}

/**
 * Only remount when a `.md-chart` host is added/removed.
 * Mutations inside an existing host (canvas rebuild, toolbar) must not retrigger attach.
 */
export function mutationTouchesChartHost(mutations: MutationRecord[]): boolean {
  for (const mutation of mutations) {
    for (const node of mutation.addedNodes) {
      if (node instanceof Element && (node.classList.contains('md-chart') || node.querySelector('.md-chart'))) {
        return true
      }
    }
    for (const node of mutation.removedNodes) {
      if (node instanceof Element && (node.classList.contains('md-chart') || node.querySelector('.md-chart'))) {
        return true
      }
    }
  }
  return false
}
