/**
 * Keep the visible message list steady when rows are prepended (load-older)
 * or removed above the viewport (history trim).
 *
 * Virtual lists cannot rely on `scrollHeight` deltas: new rows start at an
 * estimate and remeasure over several frames. Anchor by stable turn id + the
 * turn's pixel offset inside the scroller viewport instead.
 */

export type MessageListScrollAnchor = {
  turnId: string
  /** Distance from the scroller's top edge to the anchored turn's top edge. */
  offsetPx: number
}

/** Target `scrollTop` so `turnStartPx` stays `offsetPx` below the scroller top. */
export function scrollTopForAnchor(turnStartPx: number, offsetPx: number): number {
  return Math.max(0, turnStartPx - offsetPx)
}

/**
 * Viewport offset of a turn element relative to its scroll parent.
 * Returns null when the turn is not mounted (off-screen / not measured yet).
 */
export function turnOffsetInScroller(
  scrollerEl: HTMLElement,
  turnEl: HTMLElement
): number {
  return turnEl.getBoundingClientRect().top - scrollerEl.getBoundingClientRect().top
}
