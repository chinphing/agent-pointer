/**
 * The virtualizer's scroll offset, kept in step with the scroller's real
 * `scrollTop`.
 *
 * ## Why this exists
 *
 * `@tanstack/virtual-core` computes the rendered range from its **internal**
 * `scrollOffset`, and that number has exactly two writers:
 *
 * 1. a **scroll event** (`observeElementOffset`'s listener), and
 * 2. its own `applyScrollAdjustment` bookkeeping, which moves the offset by the
 *    delta it *assumed* the DOM accepted.
 *
 * Everything else that moves the scroller — the mount / stick-to-bottom
 * `el.scrollTop = …`, the viewport-shrink compensation `scrollTop += delta`, and
 * the `resizeItem` corrections written while the container has not grown yet —
 * changes the DOM **without** necessarily producing a scroll event: a write that
 * does not change the value (already at the bottom, clamped to the old max)
 * fires nothing. Nothing then re-reads the DOM, so the internal offset can sit
 * at a stale value indefinitely, and the range is computed for a position the
 * user is not at:
 *
 * - offset **behind** the DOM (e.g. an element that was already scrolled when
 *   the observers attached — the module comment in `chatMainPane.ts` records
 *   this as "inherited scrollTop blanks the virtualizer"): the rows inside the
 *   viewport are never mounted, while `getTotalSize()` still reserves their
 *   height → a blank region below the last mounted row;
 * - offset **ahead** of the DOM (a clamped `applyScrollAdjustment`): the
 *   viewport has no rendered rows at all.
 *
 * Both heal as soon as one real scroll event lands — which is why a *tiny* drag
 * renders a *large* blank: the event re-reads `el.scrollTop` from the DOM (a
 * full correction, not a delta).
 *
 * ## What this module adds
 *
 * Two things, both mirroring behaviour the core already has on the rect side:
 *
 * - **`observeElementOffset` publishes the element's current offset the moment
 *   it attaches.** `observeElementRect` reads the element immediately
 *   (`handler(getRect(element))`); the offset observer does not, so
 *   `scrollOffset` starts at `initialOffset` (0) and stays there until the user
 *   scrolls.
 * - **`sync()`** re-publishes the DOM's offset on demand, for the writes that
 *   cannot produce an event: the app calls it after every programmatic
 *   `scrollTop` write and after each measure-batch flush.
 *
 * This is deliberately an *independent invalidation signal*: it leaves the
 * virtualizer's options, the row-level `v-memo` vector and the per-turn spawn
 * plan identity untouched, and only re-states a fact the DOM already owns.
 */
import {
  observeElementOffset as defaultObserveElementOffset,
  type Virtualizer
} from '@tanstack/vue-virtual'

/** The core's `observeElementOffset` option shape (element generics stay open). */
export type VirtualOffsetObserver = (
  instance: Virtualizer<any, any>,
  cb: (offset: number, isScrolling: boolean) => void
) => (() => void) | undefined

export interface VirtualOffsetObservation {
  /**
   * Pass as the virtualizer's `observeElementOffset` option: attaches the
   * scroll listener exactly like the core does, then publishes the offset the
   * element already has.
   */
  observeElementOffset: VirtualOffsetObserver
  /**
   * Re-publish `scrollElement.scrollTop` when it differs from the virtualizer's
   * internal offset. A no-op (no callback, no notify) when the two agree, so it
   * is safe to call from hot paths. Does nothing before the observers attach.
   */
  sync(): void
}

function readOffset(instance: Virtualizer<any, any>, element: Element): number {
  return instance.options.horizontal ? element.scrollLeft : element.scrollTop
}

/**
 * @param observe — the observer to wrap; defaults to the core's. Injectable so
 *   a test can assert the attach-time read without a real scroll element.
 */
export function createVirtualOffsetObservation(
  observe: VirtualOffsetObserver = defaultObserveElementOffset as VirtualOffsetObserver
): VirtualOffsetObservation {
  let instance: Virtualizer<any, any> | null = null
  let publish: ((offset: number, isScrolling: boolean) => void) | null = null

  return {
    observeElementOffset(current, cb) {
      instance = current
      publish = cb
      const unsubscribe = observe(current, cb)
      const element = current.scrollElement
      // `observeElementRect` reads the element before observing it; without the
      // same read here an already-scrolled element keeps rendering the top of
      // the list (see the module comment).
      if (element) cb(readOffset(current, element), false)
      return unsubscribe
    },
    sync() {
      const current = instance
      const element = current?.scrollElement
      if (!current || !element || !publish) return
      const offset = readOffset(current, element)
      // Equal offsets need no publish: it would only clear `isScrolling` /
      // `scrollDirection` mid-scroll for no correction.
      if (offset === current.scrollOffset) return
      publish(offset, false)
    }
  }
}
