/**
 * Batched row measurements for the `MessageList` virtualizer.
 *
 * The per-row ref callback runs inside the patch that mounts or updates the
 * rendered window, so measuring there read layout once per row, interleaved
 * with the patch's own DOM writes: n rows cost n forced layouts, and every
 * virtualizer update written in between invalidated the next read.
 *
 * `createVirtualRowMeasureBatch` turns that into two passes:
 *
 * - `register` only queues the element — it touches no layout;
 * - one flush per registration burst reads every queued height first and then
 *   hands the whole batch to the virtualizer, so the batch forces a single
 *   layout and no read can observe a half-applied batch.
 *
 * The flush is queued as a microtask, not on the next animation frame: the
 * patch that registers the rows is synchronous, so a microtask still runs after
 * Vue's DOM update and before the browser paints. Waiting a frame let rows keep
 * their 180 px estimate for a frame after they mounted, and during fast
 * scrolling the stale offsets showed as gaps between rows.
 *
 * Registrations that no longer describe a live row are dropped before the
 * first read: a `null` node (row unmounted), a detached element, and an element
 * whose index attribute no longer matches the row it was queued for (the range
 * moved on and reused the element).
 */
import { measureElement as defaultMeasureElement, type Virtualizer } from '@tanstack/vue-virtual'

/**
 * Injectable scheduler for the coalesced flush, so the batching can be tested
 * without a real microtask queue.
 */
export interface VirtualRowMeasureFlushScheduler {
  request(callback: () => void): number
  cancel(handle: number): void
}

/**
 * Default scheduler: a microtask. `register` runs inside the patch that mounts
 * or updates the rows, so the callback lands once the patch (and Vue's DOM
 * update) has returned, but before the browser paints the frame.
 *
 * `queueMicrotask` has no cancel token, so `cancel` marks the handle; the
 * callback still runs but does nothing. Only handles cancelled before their
 * microtask ran are tracked, and each is dropped when the microtask fires.
 */
const microtaskScheduler: VirtualRowMeasureFlushScheduler = (() => {
  let nextHandle = 1
  const cancelled = new Set<number>()
  return {
    request(callback) {
      const handle = nextHandle
      nextHandle += 1
      queueMicrotask(() => {
        if (cancelled.delete(handle)) return
        callback()
      })
      return handle
    },
    cancel(handle) {
      cancelled.add(handle)
    }
  }
})()

/**
 * Attribute the row elements carry their virtual index on. Matches the
 * virtualizer's own `indexAttribute` default, which `indexFromElement` reads.
 */
const ROW_INDEX_ATTRIBUTE = 'data-index'

export interface VirtualRowMeasurement<TElement extends HTMLElement = HTMLElement> {
  index: number
  element: TElement
  /** Height read for this row in the flush's read pass. */
  size: number
}

export interface VirtualRowMeasureBatchOptions<TElement extends HTMLElement = HTMLElement> {
  /**
   * Apply every size of one flush, after the batch read all of them. Must not
   * read layout: `measureElementOption` returns the height the read pass took.
   */
  applySizes(measurements: readonly VirtualRowMeasurement<TElement>[]): void
  /** Forced-layout read for one row; defaults to `element.offsetHeight`. */
  readHeight?(element: TElement): number
  /** Optional middleware around one flush (the perf marker); must call `flush()` once. */
  wrapFlush?(flush: () => void): void
  /** Flush scheduler; defaults to a microtask queued from the registering patch. */
  host?: VirtualRowMeasureFlushScheduler
}

export interface VirtualRowMeasureBatch<TElement extends HTMLElement = HTMLElement> {
  /**
   * Queue one rendered row element. `null` (the row unmounted) and elements
   * without an index attribute are ignored.
   */
  register(element: TElement | null): void
  /** Drop a flush that has not run yet. */
  cancel(): void
  /**
   * Pass as the virtualizer's `measureElement` option: it reuses the height the
   * flush read for this element instead of forcing a second layout, and falls
   * back to the default measurement for every other caller.
   */
  measureElementOption(
    element: TElement,
    entry: ResizeObserverEntry | undefined,
    instance: Virtualizer<any, TElement>
  ): number
  readonly pending: boolean
}

export function createVirtualRowMeasureBatch<TElement extends HTMLElement = HTMLElement>(
  options: VirtualRowMeasureBatchOptions<TElement>
): VirtualRowMeasureBatch<TElement> {
  const host = options.host ?? microtaskScheduler
  const readHeight = options.readHeight ?? ((element: TElement) => element.offsetHeight)
  /** Heights of the running flush, consumed by `measureElementOption`. */
  const flushHeights = new Map<TElement, number>()
  /** Rows queued for the pending flush, by index: the newest element of a row wins. */
  let queued = new Map<number, TElement>()
  let handle: number | null = null

  function readRowIndex(element: TElement): number | null {
    const raw = element.getAttribute(ROW_INDEX_ATTRIBUTE)
    if (raw == null) return null
    const index = Number.parseInt(raw, 10)
    return Number.isNaN(index) ? null : index
  }

  function flush() {
    handle = null
    const pending = queued
    queued = new Map()
    if (pending.size === 0) return

    const run = () => {
      // Drop what stopped describing a live row before the first layout read.
      const rows: Array<{ index: number; element: TElement }> = []
      for (const [index, element] of pending) {
        if (!element.isConnected) continue
        // The range may have moved on and reused the element for another row.
        if (readRowIndex(element) !== index) continue
        rows.push({ index, element })
      }
      // Read pass: every height first, so the batch forces one layout.
      const measurements = rows.map(row => ({ ...row, size: readHeight(row.element) }))
      // Write pass: no read may run between the first and the last of these.
      for (const measurement of measurements) {
        flushHeights.set(measurement.element, measurement.size)
      }
      try {
        options.applySizes(measurements)
      } finally {
        for (const measurement of measurements) flushHeights.delete(measurement.element)
      }
    }

    if (options.wrapFlush) options.wrapFlush(run)
    else run()
  }

  return {
    register(element) {
      if (!element) return
      const index = readRowIndex(element)
      if (index === null) return
      queued.set(index, element)
      if (handle != null) return
      handle = host.request(flush)
    },
    cancel() {
      if (handle == null) return
      host.cancel(handle)
      handle = null
      queued = new Map()
    },
    measureElementOption(element, entry, instance) {
      if (!entry) {
        const size = flushHeights.get(element)
        if (size !== undefined) return size
      }
      return defaultMeasureElement(element, entry, instance)
    },
    get pending() {
      return handle != null
    }
  }
}
