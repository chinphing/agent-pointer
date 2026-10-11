export const MESSAGE_VIRTUAL_ROW_ESTIMATE = 180
/**
 * Rows kept mounted beyond the viewport on each side. Every overscan row is laid
 * out and measured on each patch, so this trades per-frame layout cost against
 * blank stripes during a fast fling — watch `v.blank*` / `v.slack` / `v.overlap`
 * in the render HUD when changing it.
 */
export const MESSAGE_VIRTUAL_OVERSCAN = 4
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
  getItemKey: (index: number) => string
) {
  return {
    count,
    estimateSize: () => MESSAGE_VIRTUAL_ROW_ESTIMATE,
    getItemKey,
    overscan: MESSAGE_VIRTUAL_OVERSCAN,
    paddingStart: MESSAGE_VIRTUAL_PADDING_START,
    paddingEnd: MESSAGE_VIRTUAL_PADDING_END
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
