export function shouldShowMessageListPlaceholder(
  hasCurrentConversation: boolean,
  isHydratingMessages: boolean,
  deferMainPane: boolean
): boolean {
  // Before chat.init() selects the first persisted conversation, rendering the
  // welcome screen causes a visible refresh flash. Keep the loading surface up.
  return !hasCurrentConversation || isHydratingMessages || deferMainPane
}

export function shouldShowWelcomeHome(
  hasCurrentConversation: boolean,
  messageCount: number,
  showPlaceholder: boolean
): boolean {
  return !showPlaceholder && hasCurrentConversation && messageCount === 0
}
