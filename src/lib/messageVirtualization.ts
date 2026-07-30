export const MESSAGE_VIRTUAL_ROW_ESTIMATE = 180
export const MESSAGE_VIRTUAL_OVERSCAN = 8
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
  if (spacingClass === 'mt-1.5') return 6
  if (spacingClass === 'mt-1') return 4
  if (spacingClass === 'mt-0.5') return 2
  return 0
}
