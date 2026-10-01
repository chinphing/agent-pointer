export const MESSAGE_VIRTUAL_ROW_ESTIMATE = 180
/**
 * How many of the most recently measured row heights the running estimate keeps.
 *
 * A window (rather than the mean of everything ever measured) is what bounds one
 * row's influence: a 1000 px turn folded into a window averaging 100 px moves the
 * estimate by ~3.5 px, so it cannot drag the whole list to its own height, while
 * the window still turns over within a screen or two of scrolling, so a
 * conversation whose turns are all short ends up with a short estimate.
 *
 * Fixed size (a ring), so folding a measure pass allocates nothing — this runs
 * inside the measure batch's flush, on the scrolling hot path.
 */
export const MESSAGE_VIRTUAL_ROW_ESTIMATE_WINDOW = 256
export const MESSAGE_VIRTUAL_OVERSCAN = 8
/**
 * Class on the virtual message row wrapper (MessageList.vue). Rows are placed by
 * an inline `translateY`, so each row's layout is self-contained; the rule in
 * `src/styles/globals.css` turns that into `contain: layout` (a measurement
 * batch stops invalidating the whole transcript). The rule deliberately carries
 * no `will-change: transform`: a composited layer per rendered row left blank
 * areas during fast scrolling, and the inline `translateY` already animates on
 * the compositor without the hint.
 */
export const MESSAGE_VIRTUAL_ROW_CLASS = 'chat-virtual-row'
export const MESSAGE_VIRTUAL_PADDING_START = 24
/** Extra space below the last turn so the bubble clears the composer edge. */
export const MESSAGE_VIRTUAL_PADDING_END = 56

export function messageVirtualizerBaseOptions(
  count: number,
  getItemKey: (index: number) => string,
  /**
   * Assumed height for a row that has no measured size yet. The virtualizer calls
   * this with the row index; the running estimate deliberately ignores it — it is
   * one average for the whole transcript, so the same index always gets the same
   * height until something is measured.
   */
  estimateSize: (index: number) => number = () => MESSAGE_VIRTUAL_ROW_ESTIMATE
) {
  return {
    count,
    estimateSize,
    getItemKey,
    overscan: MESSAGE_VIRTUAL_OVERSCAN,
    paddingStart: MESSAGE_VIRTUAL_PADDING_START,
    paddingEnd: MESSAGE_VIRTUAL_PADDING_END
  }
}

/**
 * Running estimate of how tall an unmeasured row is, fed by the heights the
 * measure batch already reads (`virtualRowMeasureBatch`).
 *
 * Why this exists: `estimateSize` is what places every row without a measured
 * height yet, and a fixed 180 px is wrong for most conversations (real rows run
 * from ~60 px to over 1000 px). Each time such a row *is* measured, every row
 * below it moves by the difference — that relocation is what re-lays-out and
 * repaints the window while scrolling. Starting unmeasured rows from the
 * conversation's own average removes most of that jump instead of relocating it.
 *
 * Invariants:
 *
 * - **Measured rows are untouched.** The value is only returned from
 *   `estimateSize`, which the virtualizer consults for rows that are *not* in its
 *   measured-size cache; a measured row keeps the exact height that was read.
 * - **Stable between measurements.** The estimate moves only when a measure pass
 *   is folded, and the virtualizer rebuilds its measurements on measurement — so
 *   the same index cannot change size between two frames with no measurement
 *   behind it.
 * - **Bounded.** See `MESSAGE_VIRTUAL_ROW_ESTIMATE_WINDOW`.
 * - **Reset with the transcript.** `reset()` returns the fallback, and
 *   `MessageList` creates one estimator per component instance — the component is
 *   keyed by conversation id and `useVirtualizer` keeps one virtualizer per
 *   instance, so a conversation switch or a recreated virtualizer starts from
 *   `MESSAGE_VIRTUAL_ROW_ESTIMATE` with no carry-over.
 */
export interface MessageRowHeightEstimator {
  /** Height to assume for a row that has not been measured yet, in px. */
  readonly estimate: number
  /**
   * Fold the sizes one measure pass read. Non-finite and non-positive sizes are
   * ignored, so a row that measured 0 cannot drag the estimate down.
   */
  sample(sizes: readonly { size: number }[]): void
  /** Drop every sample and go back to the fallback estimate. */
  reset(): void
}

export function createMessageRowHeightEstimator(options?: {
  /** Samples the running average keeps; defaults to `MESSAGE_VIRTUAL_ROW_ESTIMATE_WINDOW`. */
  windowSize?: number
  /** Value used before any sample; defaults to `MESSAGE_VIRTUAL_ROW_ESTIMATE`. */
  fallback?: number
}): MessageRowHeightEstimator {
  const windowSize = Math.max(
    1,
    Math.floor(options?.windowSize ?? MESSAGE_VIRTUAL_ROW_ESTIMATE_WINDOW)
  )
  const fallback = options?.fallback ?? MESSAGE_VIRTUAL_ROW_ESTIMATE
  /** Ring of the most recent samples: `count` filled slots, `write` the next one. */
  const heights = new Float64Array(windowSize)
  let count = 0
  let write = 0
  let sum = 0
  let estimate = fallback

  return {
    get estimate() {
      return estimate
    },
    sample(sizes) {
      let folded = false
      for (let i = 0; i < sizes.length; i += 1) {
        const size = sizes[i].size
        if (!Number.isFinite(size) || size <= 0) continue
        if (count < windowSize) {
          heights[count] = size
          count += 1
          sum += size
        } else {
          // Overwrite the oldest sample: the window slides, nothing grows.
          sum += size - heights[write]
          heights[write] = size
          write = (write + 1) % windowSize
        }
        folded = true
      }
      // Recompute once per pass, not per row: the estimate must not change
      // halfway through the rows it is about to place.
      if (folded) estimate = sum / count
    },
    reset() {
      count = 0
      write = 0
      sum = 0
      estimate = fallback
    }
  }
}

export function messageTurnSpacingPixels(
  turnIndex: number,
  previousTurnCollapsed = false
): number {
  if (turnIndex === 0 || previousTurnCollapsed) return 0
  return 28
}

export function messageRowSpacingPixels(spacingClass: string): number {
  if (spacingClass === 'mt-7') return 28
  if (spacingClass === 'mt-4') return 16
  if (spacingClass === 'mt-3.5') return 14
  if (spacingClass === 'mt-1.5') return 6
  if (spacingClass === 'mt-1') return 4
  if (spacingClass === 'mt-0.5') return 2
  return 0
}
