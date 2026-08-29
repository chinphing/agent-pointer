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

/** True while the user is typing in the footer / inline composer. */
export function isComposerDraftingTarget(target: EventTarget | null): boolean {
  if (!target || typeof (target as { closest?: unknown }).closest !== 'function') return false
  try {
    return !!(target as HTMLElement).closest('.composer-shell')
  } catch {
    return false
  }
}

/**
 * After the composer grows, keep skipping totalSize stick until the viewport
 * grows again (draft deleted) or the user is no longer drafting.
 * A short time window is not enough: WebKit often remasures rows hundreds of
 * ms later and the stick still pulls the last lines under the composer.
 */
export function shouldSkipTotalSizeStick(opts: {
  nowMs: number
  viewportShrinkAtMs: number
  windowMs: number
  skipUntilViewportGrows: boolean
  composerDrafting: boolean
}): boolean {
  if (opts.composerDrafting || opts.skipUntilViewportGrows) return true
  return shouldSkipTotalSizeStickAfterViewportShrink(
    opts.nowMs,
    opts.viewportShrinkAtMs,
    opts.windowMs
  )
}

export type SwitchConversationScrollPlan = 'locate' | 'jumpToLatest' | 'toBottom'

/**
 * Re-opening a conversation without a search hit must land on the real tail.
 * Pending focus keeps the around window; `toBottom` alone would pin a hole.
 */
export function switchConversationScrollPlan(input: {
  hasPendingFocus: boolean
  hasMoreNewer: boolean
}): SwitchConversationScrollPlan {
  if (input.hasPendingFocus) return 'locate'
  if (input.hasMoreNewer) return 'jumpToLatest'
  return 'toBottom'
}

/** Around-window bottom is not the transcript tail; do not hide jump-to-latest. */
export function toBottomFollowsOutput(hasMoreNewer: boolean): boolean {
  return !hasMoreNewer
}
