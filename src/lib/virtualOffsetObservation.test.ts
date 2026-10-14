import { readFileSync } from 'node:fs'
import { describe, expect, it, vi } from 'vitest'
import { Virtualizer, observeElementOffset as defaultObserveElementOffset } from '@tanstack/vue-virtual'
import { createVirtualOffsetObservation, type VirtualOffsetObserver } from './virtualOffsetObservation'

/**
 * Minimal scroll element: real enough for the core's `observeElementOffset`
 * (a scroll listener + `scrollTop` / `scrollHeight` / `clientHeight`) and for
 * `elementScroll` (a `scrollTo` that clamps like a browser).
 */
function fakeScroller(options: {
  clientHeight?: number
  contentHeight?: number
  scrollTop?: number
} = {}) {
  const listeners = new Set<() => void>()
  const frameWindow = {
    setTimeout: (fn: () => void, ms?: number) => setTimeout(fn, ms) as unknown as number,
    clearTimeout: (id: number) => clearTimeout(id as unknown as ReturnType<typeof setTimeout>),
    requestAnimationFrame: (fn: () => void) => setTimeout(() => fn(), 0) as unknown as number,
    cancelAnimationFrame: (id: number) => clearTimeout(id as unknown as ReturnType<typeof setTimeout>)
  }
  const el = {
    scrollTop: options.scrollTop ?? 0,
    scrollLeft: 0,
    clientHeight: options.clientHeight ?? 600,
    clientWidth: 800,
    scrollHeight: options.contentHeight ?? 0,
    offsetHeight: options.clientHeight ?? 600,
    offsetWidth: 800,
    ownerDocument: { defaultView: frameWindow },
    isConnected: true,
    addEventListener(type: string, fn: () => void) {
      if (type === 'scroll') listeners.add(fn)
    },
    removeEventListener(type: string, fn: () => void) {
      if (type === 'scroll') listeners.delete(fn)
    },
    /** Browser-like write: clamps to the current content box, no event on a no-op. */
    scrollTo({ top }: { top: number }) {
      const max = Math.max(0, el.scrollHeight - el.clientHeight)
      const next = Math.max(0, Math.min(max, top))
      if (next === el.scrollTop) return
      el.scrollTop = next
      for (const fn of [...listeners]) fn()
    },
    dispatchScroll() {
      for (const fn of [...listeners]) fn()
    }
  }
  return el
}

function mount(
  el: ReturnType<typeof fakeScroller>,
  keys: string[],
  observeElementOffset: VirtualOffsetObserver,
  estimateSize: () => number = () => 862
) {
  const v = new Virtualizer<HTMLElement, HTMLElement>({
    count: keys.length,
    estimateSize,
    getItemKey: index => keys[index]!,
    overscan: 8,
    paddingStart: 24,
    paddingEnd: 56,
    getScrollElement: () => el as unknown as HTMLElement,
    observeElementRect: (_instance, cb) => {
      cb({ width: 800, height: el.clientHeight })
      return () => {}
    },
    observeElementOffset,
    // The core's `elementScroll`: the DOM receives `offset + adjustments`.
    scrollToFn: (offset, options) =>
      el.scrollTo({ top: offset + (options?.adjustments ?? 0) }),
    measureElement: node => (node as unknown as { offsetHeight: number }).offsetHeight
  })
  v._willUpdate()
  return v
}

const rows = (v: Virtualizer<HTMLElement, HTMLElement>) => v.getVirtualItems()
const lastIndex = (v: Virtualizer<HTMLElement, HTMLElement>) => {
  const items = rows(v)
  return items.length ? items[items.length - 1]!.index : -1
}

describe('createVirtualOffsetObservation', () => {
  it('publishes the element offset when the observers attach', () => {
    const inner = vi.fn((_instance, _cb) => () => {})
    const observation = createVirtualOffsetObservation(inner as unknown as VirtualOffsetObserver)
    const publish = vi.fn()
    const element = { scrollTop: 4321, scrollLeft: 0 }
    const instance = {
      scrollElement: element,
      options: { horizontal: false }
    } as unknown as Virtualizer<HTMLElement, HTMLElement>

    observation.observeElementOffset(instance, publish)

    expect(inner).toHaveBeenCalledTimes(1)
    expect(publish).toHaveBeenCalledWith(4321, false)
  })

  it('sync publishes only when the DOM offset differs, and is a no-op before attach', () => {
    const observation = createVirtualOffsetObservation(vi.fn(() => () => {}) as unknown as VirtualOffsetObserver)
    const publish = vi.fn()
    const element = { scrollTop: 900, scrollLeft: 0 }
    const instance = {
      scrollElement: element,
      scrollOffset: 900,
      options: { horizontal: false }
    } as unknown as Virtualizer<HTMLElement, HTMLElement>

    // Before attach: nothing to publish, nothing to throw.
    observation.sync()
    expect(publish).not.toHaveBeenCalled()

    observation.observeElementOffset(instance, publish)
    publish.mockClear()

    observation.sync()
    expect(publish).not.toHaveBeenCalled()

    element.scrollTop = 1500
    observation.sync()
    expect(publish).toHaveBeenCalledWith(1500, false)
  })

  it('reads scrollLeft when the virtualizer is horizontal', () => {
    const observation = createVirtualOffsetObservation(vi.fn(() => () => {}) as unknown as VirtualOffsetObserver)
    const publish = vi.fn()
    const instance = {
      scrollElement: { scrollTop: 100, scrollLeft: 777 },
      options: { horizontal: true }
    } as unknown as Virtualizer<HTMLElement, HTMLElement>

    observation.observeElementOffset(instance, publish)

    expect(publish).toHaveBeenCalledWith(777, false)
  })
})

describe('a scroller that is already scrolled when the observers attach', () => {
  const keys = Array.from({ length: 20 }, (_, index) => `turn-${index + 1}`)

  it('the core observer throws the position away and renders the top of the list', () => {
    const el = fakeScroller({ clientHeight: 600, contentHeight: 20 * 862 + 80, scrollTop: 12000 })
    const v = mount(el, keys, defaultObserveElementOffset as unknown as VirtualOffsetObserver)

    // `_willUpdate` pushes its own `scrollOffset` (still `initialOffset` = 0, the
    // observers having never read the element) into the DOM, so the element the
    // user had scrolled is reset and the range covers the first rows only.
    expect(el.scrollTop).toBe(0)
    expect(v.scrollOffset).toBe(0)
    expect(lastIndex(v)).toBeLessThan(keys.length - 1)
  })

  it('keeps the position and renders the rows the user is looking at with the attach-time read', () => {
    const el = fakeScroller({ clientHeight: 600, contentHeight: 20 * 862 + 80, scrollTop: 12000 })
    const v = mount(el, keys, createVirtualOffsetObservation().observeElementOffset)

    expect(el.scrollTop).toBe(12000)
    expect(v.scrollOffset).toBe(12000)
    expect(lastIndex(v)).toBe(keys.length - 1)
  })
})

describe('offset corrections that the DOM clamps away', () => {
  const keys = Array.from({ length: 13 }, (_, index) => `turn-${index + 1}`)

  /** Long enough for the core's `isScrollingResetDelay` (150 ms) to settle. */
  const settle = () => new Promise(resolve => setTimeout(resolve, 220))

  it('sync repairs an offset that ran ahead while the container was still small', async () => {
    const el = fakeScroller({ clientHeight: 600, contentHeight: 13 * 180 + 80 })
    const observation = createVirtualOffsetObservation()
    const v = mount(el, keys, observation.observeElementOffset, () => 180)

    // The app sticks to the bottom of the (still estimated) content: the write
    // changes the element, so the scroll event lands and both sides agree.
    el.scrollTo({ top: el.scrollHeight - el.clientHeight })
    await settle()
    expect(v.scrollOffset).toBe(el.scrollTop)

    // Measure batch: every rendered row reports its real (much larger) height.
    // The container's height is written by the framework on the *next* render,
    // so each scroll adjustment is clamped to the old max — and a clamped write
    // that does not change the value fires no scroll event, leaving the
    // virtualizer's offset ahead of the element it is rendering for.
    for (const item of [...rows(v)]) {
      v.measureElement({
        getAttribute: () => String(item.index),
        offsetHeight: 862,
        isConnected: true
      } as unknown as HTMLElement)
    }

    expect(v.scrollOffset).toBeGreaterThan(el.scrollTop)

    // The container catches up; the app re-states the DOM's offset.
    el.scrollHeight = v.getTotalSize() + 24
    observation.sync()

    expect(v.scrollOffset).toBe(el.scrollTop)
  })
})

describe('sync cannot re-enter itself', () => {
  const keys = Array.from({ length: 13 }, (_, index) => `turn-${index + 1}`)

  it('publishes once per sync even when the publish path calls sync back', () => {
    const el = fakeScroller({ clientHeight: 600, contentHeight: 13 * 180 + 80 })
    const observation = createVirtualOffsetObservation()
    const v = mount(el, keys, observation.observeElementOffset, () => 180)

    // The wiring the re-entrancy worry is about: the callback `publish` lands in writes
    // the offset back onto the virtualizer (what the core's own listener does) and calls
    // `sync()` again before returning. The equal-offset guard has to absorb that, so one
    // `sync()` stays one publish instead of driving a loop.
    let publishes = 0
    observation.observeElementOffset(v, (offset: number) => {
      publishes += 1
      v.scrollOffset = offset
      observation.sync()
    })
    publishes = 0

    // The case the module exists for: the internal offset ran ahead of a write the DOM
    // clamped away, and the write fired no scroll event.
    el.scrollTop = el.scrollHeight - el.clientHeight
    v.scrollOffset = (v.scrollOffset ?? 0) + 5000

    observation.sync()

    expect(publishes).toBe(1)
    expect(v.scrollOffset).toBe(el.scrollTop)
  })
})

// The virtualizer and the scroller only meet inside MessageList.vue (no
// component fixture), so this guards the wiring itself: the observer must be
// handed to `useVirtualizer`, and every programmatic scroll write must be
// followed by a re-state of the element's offset.
describe('MessageList wiring', () => {  const source = readFileSync(
    new URL('../components/chat/MessageList.vue', import.meta.url),
    'utf8'
  )

  function region(startMarker: string, endMarker: string): string {
    const start = source.indexOf(startMarker)
    expect(start, `expected ${startMarker} in MessageList.vue`).toBeGreaterThan(-1)
    const end = source.indexOf(endMarker, start + startMarker.length)
    expect(end, `expected ${endMarker} after ${startMarker}`).toBeGreaterThan(start)
    return source.slice(start, end)
  }

  it('hands the offset observation to the virtualizer', () => {
    expect(source).toMatch(/observeElementOffset:\s*offsetObservation\.observeElementOffset/)
  })

  it('re-states the offset after every programmatic scroll write', () => {
    expect(region('function stickScrollerToBottom()', 'function scheduleToBottom()')).toMatch(
      /offsetObservation\.sync\(\)/
    )
    expect(region('function applyScrollerViewportResize()', 'function ')).toMatch(
      /offsetObservation\.sync\(\)/
    )
  })

  it('re-states the offset after the measure batch applied its sizes', () => {
    expect(region('applySizes(measurements) {', 'wrapFlush(flush) {')).toMatch(
      /offsetObservation\.sync\(\)/
    )
  })
})
