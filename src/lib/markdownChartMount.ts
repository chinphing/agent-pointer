/** Shared Chart.js mount scheduling — keep conversation switches from freezing. */

export const CHART_VIEWPORT_MARGIN_PX = 240
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

export function closestChatScroller(el: Element): Element | null {
  const scroller = el.closest('.chat-scroll-area')
  return scroller instanceof Element ? scroller : null
}

export function isNearViewport(el: Element, marginPx = CHART_VIEWPORT_MARGIN_PX): boolean {
  if (typeof window === 'undefined') return true
  const rect = el.getBoundingClientRect()
  // Unlaid-out hosts (0×0) stay on the size-waiter path, not IntersectionObserver.
  if (rect.width <= 0 && rect.height <= 0) return true
  const top = -marginPx
  const bottom = (window.innerHeight || 0) + marginPx
  return rect.bottom >= top && rect.top <= bottom
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
      root: closestChatScroller(el),
      rootMargin: `${marginPx}px 0px`,
      threshold: 0,
    },
  )
  observer.observe(el)
  return observer
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
