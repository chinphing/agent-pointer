export function shouldShowMessageListPlaceholder(
  hasCurrentConversation: boolean,
  isHydratingMessages: boolean,
  deferMainPane: boolean,
  loadedMessageCount = 0
): boolean {
  // Force tail reload sets the hydrating flag while messages are already in
  // memory. Unmounting MessageList then remounts it, and onMounted jump-to-latest
  // fires the same messages?limitTurns=8 request in a loop.
  // Conversation switches remount via `:key="currentId"` instead — do not use
  // this gate to keep a shared scroller across sessions (inherited scrollTop
  // blanks the virtualizer).
  if (loadedMessageCount > 0) return false
  // Before chat.init() selects the first persisted conversation, rendering the
  // welcome screen causes a visible refresh flash. Keep the loading surface up.
  return !hasCurrentConversation || isHydratingMessages || deferMainPane
}

export function shouldShowWelcomeHome(
  hasCurrentConversation: boolean,
  messageCount: number,
  showPlaceholder: boolean,
  persistedMessageCount = 0
): boolean {
  // A failed around-window must not look like a brand-new empty chat.
  if (persistedMessageCount > 0 && messageCount === 0) return false
  return !showPlaceholder && hasCurrentConversation && messageCount === 0
}
