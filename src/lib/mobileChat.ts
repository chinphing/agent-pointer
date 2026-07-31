/** Mobile UI creates a fresh conversation only after a long user-anchored thread. */
export const MOBILE_NEW_CONVERSATION_TURN_THRESHOLD = 30

export function shouldShowMobileNewConversationButton(
  isMobileViewport: boolean,
  turnCount: number
): boolean {
  return isMobileViewport && turnCount > MOBILE_NEW_CONVERSATION_TURN_THRESHOLD
}
