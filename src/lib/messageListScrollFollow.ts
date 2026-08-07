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
