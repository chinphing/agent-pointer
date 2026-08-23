/**
 * When to fetch older turns vs show the "no earlier messages" pull hint.
 *
 * Auto-prefetch on `scroll` needs `scrollTop` to decrease. At the real top
 * (`scrollTop === 0`) further wheel-up / pull-down does not move `scrollTop`,
 * so user intent must be read from wheel/touch instead.
 */

/** Prefetch older turns when within this distance of the top. */
export const LOAD_OLDER_TOP_PX = 300
/**
 * After an older page lands, require the user to leave the top band once
 * before auto-prefetch can fire again. Prevents a restore-shortfall cascade.
 */
export const LOAD_OLDER_LEAVE_TOP_PX = 400
/** Finger must move this far down at the top before it counts as a pull. */
export const OLDER_TOUCH_PULL_PX = 12

/**
 * `true` / `false` from paging state; `null` when the conversation has no
 * baseline (trim dropped it) — `loadOlderMessages` self-heals.
 */
export type HasMoreOlderFlag = boolean | null

export function shouldRequestOlderFromWheel(input: {
  deltaY: number
  scrollTop: number
  hasMoreOlder: HasMoreOlderFlag
}): boolean {
  if (input.deltaY >= 0) return false
  if (input.scrollTop > LOAD_OLDER_TOP_PX) return false
  return input.hasMoreOlder !== false
}

export function shouldRequestOlderFromTouchPull(input: {
  pullPx: number
  scrollTop: number
  hasMoreOlder: HasMoreOlderFlag
}): boolean {
  if (input.pullPx < OLDER_TOUCH_PULL_PX) return false
  if (input.scrollTop > 1) return false
  return input.hasMoreOlder !== false
}

export function canShowNoOlderPullHint(input: {
  hasMoreOlder: HasMoreOlderFlag
  scrollTop: number
  programmatic: boolean
}): boolean {
  return input.hasMoreOlder === false && input.scrollTop <= 1 && !input.programmatic
}

export function shouldRearmOlderPrefetch(scrollTop: number): boolean {
  return scrollTop > LOAD_OLDER_LEAVE_TOP_PX
}

export function shouldAutoPrefetchOlderOnScroll(input: {
  atTop: boolean
  scrollingUp: boolean
  armed: boolean
}): boolean {
  return input.atTop && input.scrollingUp && input.armed
}
