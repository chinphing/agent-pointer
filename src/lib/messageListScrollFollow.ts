/**
 * Sticky follow for streaming chat lists.
 *
 * Wheel / touch already unpin immediately. Scrollbar drag only fires `scroll`,
 * so without a scroll-up check the list can stay in "follow" while the thumb
 * is pulled up — `scheduleToBottom` then fights the drag every stream token.
 */

export type FollowOutputScrollSample = {
  followOutput: boolean
  distanceFromBottom: number
  /** True when scrollTop moved toward older content (scrollbar / wheel / touch). */
  scrollingUp: boolean
  attachPx: number
  detachPx: number
}

/**
 * Next follow flag after a user-driven scroll sample (not programmatic sticks).
 *
 * - scrollingUp → detach immediately
 * - distance ≤ attach → re-attach
 * - distance > detach → detach by position
 * - otherwise keep prior (hysteresis between attach and detach)
 */
export function nextFollowOutputAfterScroll(sample: FollowOutputScrollSample): boolean {
  if (sample.scrollingUp) return false
  if (sample.distanceFromBottom <= sample.attachPx) return true
  if (sample.distanceFromBottom > sample.detachPx) return false
  return sample.followOutput
}

/**
 * Pixels to add to `scrollTop` when the scroller viewport shrinks (composer /
 * chrome grew). Grow / equal → 0 so deleting draft lines does not pull content.
 * `previousHeight <= 0` is treated as uninitialized, not a shrink from zero.
 */
export function scrollerViewportShrinkDelta(
  previousHeight: number,
  nextHeight: number
): number {
  if (
    !Number.isFinite(previousHeight) ||
    !Number.isFinite(nextHeight) ||
    previousHeight <= 0 ||
    nextHeight <= 0
  ) {
    return 0
  }
  if (nextHeight >= previousHeight) return 0
  return previousHeight - nextHeight
}

/**
 * Composer layout can flush virtualizer measurements in the same tick as a
 * viewport shrink. `getTotalSize` stick-to-bottom then jumps to a stale short
 * row and paints the last lines under the composer. Skip that stick briefly.
 */
export function shouldSkipTotalSizeStickAfterViewportShrink(
  nowMs: number,
  viewportShrinkAtMs: number,
  windowMs: number
): boolean {
  if (viewportShrinkAtMs <= 0 || windowMs <= 0) return false
  const elapsed = nowMs - viewportShrinkAtMs
  return elapsed >= 0 && elapsed < windowMs
}
